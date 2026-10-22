//! Tasks from summaries (auto) and by hand, and the re-extraction rule that keeps the user's own edits.

use crate::knowledge::extract::Extraction;
use crate::knowledge::MAX_TASK_CHARS;
use crate::state::AppState;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashSet;
use tauri::State;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    pub id: String,
    pub meeting_id: Option<String>,
    pub meeting_title: Option<String>,
    pub meeting_date: Option<String>,
    pub text: String,
    pub owner: String,
    pub due: Option<String>,
    pub at: Option<f64>,
    pub status: String,
    pub source: String,
    pub created_at: String,
}

/// Lowercase, trimmed, with runs of whitespace collapsed: the key for "the same task".
pub(crate) fn normalise(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The current time as a fixed-width UTC string, so the text compares in time order.
pub(crate) fn stamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Replaces this meeting's untouched auto tasks with the extraction (spec 6.2), in one transaction.
/// Returns false, and writes nothing, when the run was superseded: a newer pending mark exists (see `set_extract`).
pub(crate) async fn apply_extraction(
    pool: &SqlitePool,
    meeting_id: &str,
    ex: &Extraction,
    started: &str,
) -> Result<bool, sqlx::Error> {
    let decisions =
        serde_json::to_string(&ex.decisions).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let requests =
        serde_json::to_string(&ex.requests).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM tasks WHERE meeting_id = ? AND source = 'auto' AND status = 'open' AND edited = 0")
        .bind(meeting_id)
        .execute(&mut *tx)
        .await?;
    let kept: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT text, origin_norm FROM tasks WHERE meeting_id = ?")
            .bind(meeting_id)
            .fetch_all(&mut *tx)
            .await?;
    // Keys of the kept rows: their text, and the text they were first extracted with (so an edit does not duplicate).
    // Also catches a task the model repeats within one reply.
    let mut seen: HashSet<String> = kept
        .iter()
        .flat_map(|(text, origin)| std::iter::once(normalise(text)).chain(origin.clone()))
        .collect();
    let now = stamp();
    for task in &ex.tasks {
        let key = normalise(&task.text);
        if !seen.insert(key.clone()) {
            continue;
        }
        // Guarded: a meeting deleted mid-run gets no task.
        sqlx::query(
            "INSERT INTO tasks (id, meeting_id, text, owner, due, at, status, source, edited, created_at, origin_norm) \
             SELECT ?, ?, ?, ?, ?, ?, 'open', 'auto', 0, ?, ? WHERE EXISTS (SELECT 1 FROM meetings WHERE id = ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(meeting_id)
        .bind(&task.text)
        .bind(&task.owner)
        .bind(&task.due)
        .bind(task.at)
        .bind(&now)
        .bind(&key)
        .bind(meeting_id)
        .execute(&mut *tx)
        .await?;
    }
    let written = set_extract(
        &mut *tx,
        meeting_id,
        "done",
        None,
        Some((&decisions, &requests)),
        Some(started),
    )
    .await?;
    if written == 0 {
        // Dropping the transaction rolls back the deletes and inserts above.
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}

/// Sets an extraction's status and returns the rows written. `lists` replaces the decisions and requests JSON; None
/// keeps them (a new row gets empty lists). Guarded: a meeting that no longer exists gets no row. With `started`, the
/// write is skipped (0 rows) when a pending mark was set after `started`, so a summary saved mid-run is not lost.
pub(crate) async fn set_extract<'c>(
    exec: impl sqlx::SqliteExecutor<'c>,
    meeting_id: &str,
    status: &str,
    error: Option<&str>,
    lists: Option<(&str, &str)>,
    started: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let (decisions, requests) = lists.unwrap_or(("[]", "[]"));
    let keep = lists.is_none();
    let guard = if started.is_some() {
        " WHERE NOT (meeting_extracts.status = 'pending' AND meeting_extracts.extracted_at > ?)"
    } else {
        ""
    };
    let sql = format!(
        "INSERT INTO meeting_extracts (meeting_id, decisions_json, requests_json, status, error, extracted_at) \
         SELECT ?, ?, ?, ?, ?, ? WHERE EXISTS (SELECT 1 FROM meetings WHERE id = ?) \
         ON CONFLICT(meeting_id) DO UPDATE SET \
           decisions_json = CASE WHEN ? THEN meeting_extracts.decisions_json ELSE excluded.decisions_json END, \
           requests_json = CASE WHEN ? THEN meeting_extracts.requests_json ELSE excluded.requests_json END, \
           status = excluded.status, error = excluded.error, extracted_at = excluded.extracted_at{guard}"
    );
    let mut query = sqlx::query(&sql)
        .bind(meeting_id)
        .bind(decisions)
        .bind(requests)
        .bind(status)
        .bind(error)
        .bind(stamp())
        .bind(meeting_id)
        .bind(keep)
        .bind(keep);
    if let Some(started) = started {
        query = query.bind(started);
    }
    Ok(query.execute(exec).await?.rows_affected())
}

/// Trims a task text and checks it is 1 to MAX_TASK_CHARS characters (chars, not bytes).
fn checked_text(text: &str) -> anyhow::Result<String> {
    let text = text.trim();
    let len = text.chars().count();
    anyhow::ensure!(
        (1..=MAX_TASK_CHARS).contains(&len),
        "task text must be 1 to {MAX_TASK_CHARS} characters"
    );
    Ok(text.to_string())
}

/// Errors when an UPDATE or DELETE matched no row.
fn found(rows: u64) -> anyhow::Result<()> {
    anyhow::ensure!(rows > 0, "task not found");
    Ok(())
}

/// Open tasks (meeting date desc, then extraction order, manual last) or done tasks (latest done first).
pub(crate) async fn list(pool: &SqlitePool, status: &str) -> anyhow::Result<Vec<TaskRow>> {
    let order = match status {
        "open" => "t.meeting_id IS NULL, m.created_at DESC, t.created_at, t.rowid",
        "done" => "t.done_at DESC, t.rowid DESC",
        _ => anyhow::bail!("unknown task list"),
    };
    let sql = format!(
        "SELECT t.id, t.meeting_id, m.title AS meeting_title, m.created_at AS meeting_date, t.text, t.owner, \
         t.due, t.at, t.status, t.source, t.created_at \
         FROM tasks t LEFT JOIN meetings m ON m.id = t.meeting_id \
         WHERE t.status = ? ORDER BY {order}"
    );
    Ok(sqlx::query_as(&sql).bind(status).fetch_all(pool).await?)
}

pub(crate) async fn set_status(pool: &SqlitePool, id: &str, status: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(status, "open" | "done" | "dismissed"),
        "unknown task status"
    );
    // Any user change marks the task as touched, so a re-extraction never deletes it.
    let done_at = (status == "done").then(stamp);
    let rows = sqlx::query("UPDATE tasks SET status = ?, done_at = ?, edited = 1 WHERE id = ?")
        .bind(status)
        .bind(done_at)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    found(rows)
}

pub(crate) async fn edit(pool: &SqlitePool, id: &str, text: &str) -> anyhow::Result<()> {
    let text = checked_text(text)?;
    let rows = sqlx::query("UPDATE tasks SET text = ?, edited = 1 WHERE id = ?")
        .bind(text)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    found(rows)
}

pub(crate) async fn claim(pool: &SqlitePool, id: &str) -> anyhow::Result<()> {
    let rows = sqlx::query("UPDATE tasks SET owner = 'me', edited = 1 WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    found(rows)
}

pub(crate) async fn add(pool: &SqlitePool, text: &str) -> anyhow::Result<TaskRow> {
    let row = TaskRow {
        id: Uuid::new_v4().to_string(),
        meeting_id: None,
        meeting_title: None,
        meeting_date: None,
        text: checked_text(text)?,
        owner: "me".to_string(),
        due: None,
        at: None,
        status: "open".to_string(),
        source: "manual".to_string(),
        created_at: stamp(),
    };
    sqlx::query(
        "INSERT INTO tasks (id, meeting_id, text, owner, status, source, edited, created_at) \
         VALUES (?, NULL, ?, ?, ?, ?, 0, ?)",
    )
    .bind(&row.id)
    .bind(&row.text)
    .bind(&row.owner)
    .bind(&row.status)
    .bind(&row.source)
    .bind(&row.created_at)
    .execute(pool)
    .await?;
    Ok(row)
}

/// An auto task is dismissed, not deleted, so re-extraction does not bring it back. A manual task is deleted.
pub(crate) async fn remove(pool: &SqlitePool, id: &str) -> anyhow::Result<()> {
    let dismissed = sqlx::query(
        "UPDATE tasks SET status = 'dismissed', edited = 1 WHERE id = ? AND source = 'auto'",
    )
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    if dismissed > 0 {
        return Ok(());
    }
    let deleted = sqlx::query("DELETE FROM tasks WHERE id = ? AND source = 'manual'")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    found(deleted)
}

#[tauri::command]
pub async fn tasks_list(
    state: State<'_, AppState>,
    status: String,
) -> Result<Vec<TaskRow>, String> {
    list(state.db_manager.pool(), &status)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tasks_set_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<(), String> {
    set_status(state.db_manager.pool(), &id, &status)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tasks_edit(
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> Result<(), String> {
    edit(state.db_manager.pool(), &id, &text)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tasks_claim(state: State<'_, AppState>, id: String) -> Result<(), String> {
    claim(state.db_manager.pool(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tasks_add(state: State<'_, AppState>, text: String) -> Result<TaskRow, String> {
    add(state.db_manager.pool(), &text)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tasks_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    remove(state.db_manager.pool(), &id)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use crate::knowledge::extract::ExtractedTask;

    async fn insert_meeting(pool: &SqlitePool, id: &str) {
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'M', 't', 't')",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_task(
        pool: &SqlitePool,
        id: &str,
        meeting: Option<&str>,
        text: &str,
        status: &str,
        source: &str,
        edited: i64,
    ) {
        sqlx::query(
            "INSERT INTO tasks (id, meeting_id, text, owner, status, source, edited, created_at) \
             VALUES (?, ?, ?, 'me', ?, ?, ?, 't')",
        )
        .bind(id)
        .bind(meeting)
        .bind(text)
        .bind(status)
        .bind(source)
        .bind(edited)
        .execute(pool)
        .await
        .unwrap();
    }

    fn task(text: &str) -> ExtractedTask {
        ExtractedTask {
            text: text.to_string(),
            owner: "me".to_string(),
            due: None,
            at: None,
        }
    }

    fn extraction(tasks: Vec<ExtractedTask>) -> Extraction {
        Extraction {
            tasks,
            decisions: vec!["Ship it".to_string()],
            requests: vec![],
        }
    }

    #[test]
    fn normalise_folds_case_and_whitespace() {
        assert_eq!(normalise("  Купить   МОЛОКО \n"), "купить молоко");
    }

    #[tokio::test]
    async fn reextract_keeps_user_touched_tasks() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_task(&pool, "a", Some("M"), "A", "open", "auto", 0).await;
        insert_task(&pool, "b", Some("M"), "B", "done", "auto", 0).await;
        insert_task(&pool, "c", Some("M"), "C", "open", "auto", 1).await;
        insert_task(&pool, "d", Some("M"), "D", "dismissed", "auto", 0).await;

        let ex = extraction(vec![task("A2"), task("b"), task("C"), task("E")]);
        assert!(apply_extraction(&pool, "M", &ex, &stamp()).await.unwrap());

        // (id, text, status, edited): the kept rows must keep their ids, which proves they were not touched.
        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT id, text, status, edited FROM tasks WHERE meeting_id = 'M' ORDER BY text",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let kept: Vec<(&str, &str, &str, i64)> = rows
            .iter()
            .filter(|r| ["b", "c", "d"].contains(&r.0.as_str()))
            .map(|r| (r.0.as_str(), r.1.as_str(), r.2.as_str(), r.3))
            .collect();
        assert_eq!(
            kept,
            vec![
                ("b", "B", "done", 0),
                ("c", "C", "open", 1),
                ("d", "D", "dismissed", 0),
            ]
        );
        let texts: Vec<(&str, &str, i64)> = rows
            .iter()
            .map(|r| (r.1.as_str(), r.2.as_str(), r.3))
            .collect();
        assert_eq!(
            texts,
            vec![
                ("A2", "open", 0),
                ("B", "done", 0),
                ("C", "open", 1),
                ("D", "dismissed", 0),
                ("E", "open", 0),
            ]
        );
    }

    #[tokio::test]
    async fn apply_extraction_writes_nothing_for_a_deleted_meeting() {
        let pool = test_pool().await;
        apply_extraction(&pool, "gone", &extraction(vec![task("A")]), &stamp())
            .await
            .unwrap();

        let (tasks,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tasks")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (extracts,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meeting_extracts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((tasks, extracts), (0, 0));
    }

    async fn id_of(pool: &SqlitePool, text: &str) -> String {
        sqlx::query_scalar("SELECT id FROM tasks WHERE text = ?")
            .bind(text)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn texts(rows: &[TaskRow]) -> Vec<&str> {
        rows.iter().map(|r| r.text.as_str()).collect()
    }

    #[tokio::test]
    async fn list_open_orders_by_meeting_date_then_manual() {
        let pool = test_pool().await;
        insert_meeting(&pool, "old").await;
        insert_meeting(&pool, "new").await;
        sqlx::query("UPDATE meetings SET created_at = '2026-10-01' WHERE id = 'old'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE meetings SET created_at = '2026-10-20' WHERE id = 'new'")
            .execute(&pool)
            .await
            .unwrap();
        insert_task(&pool, "o1", Some("old"), "old task", "open", "auto", 0).await;
        insert_task(&pool, "n1", Some("new"), "new first", "open", "auto", 0).await;
        insert_task(&pool, "n2", Some("new"), "new second", "open", "auto", 0).await;
        // Same meeting: created_at decides, so n2 (created earlier) comes first.
        sqlx::query("UPDATE tasks SET created_at = 'b' WHERE id = 'n1'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE tasks SET created_at = 'a' WHERE id = 'n2'")
            .execute(&pool)
            .await
            .unwrap();
        insert_task(&pool, "d", Some("new"), "done one", "done", "auto", 0).await;
        add(&pool, "manual").await.unwrap();

        let open = list(&pool, "open").await.unwrap();
        assert_eq!(
            texts(&open),
            vec!["new second", "new first", "old task", "manual"]
        );
        assert!(list(&pool, "dismissed").await.is_err());
    }

    #[tokio::test]
    async fn delete_auto_dismisses_delete_manual_removes() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_task(&pool, "a", Some("M"), "auto", "open", "auto", 0).await;
        let manual = add(&pool, "mine").await.unwrap();

        remove(&pool, "a").await.unwrap();
        remove(&pool, &manual.id).await.unwrap();

        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, status FROM tasks ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows, vec![("a".to_string(), "dismissed".to_string())]);
        assert!(remove(&pool, "missing").await.is_err());
    }

    #[tokio::test]
    async fn edit_and_claim_mark_edited() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_task(&pool, "a", Some("M"), "A", "open", "auto", 0).await;
        insert_task(&pool, "b", Some("M"), "B", "open", "auto", 0).await;
        sqlx::query("UPDATE tasks SET owner = 'unassigned'")
            .execute(&pool)
            .await
            .unwrap();

        edit(&pool, "a", "  A edited  ").await.unwrap();
        claim(&pool, "b").await.unwrap();

        let rows: Vec<(String, String, String, i64)> =
            sqlx::query_as("SELECT id, text, owner, edited FROM tasks ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "a".to_string(),
                    "A edited".to_string(),
                    "unassigned".to_string(),
                    1
                ),
                ("b".to_string(), "B".to_string(), "me".to_string(), 1),
            ]
        );
        assert!(edit(&pool, "missing", "x").await.is_err());
        assert!(claim(&pool, "missing").await.is_err());
    }

    #[tokio::test]
    async fn done_stamps_and_reopen_clears() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_task(&pool, "a", Some("M"), "A", "open", "auto", 0).await;
        insert_task(&pool, "b", Some("M"), "B", "open", "auto", 0).await;

        set_status(&pool, "a", "done").await.unwrap();
        set_status(&pool, "b", "done").await.unwrap();
        let (stamped,): (Option<String>,) =
            sqlx::query_as("SELECT done_at FROM tasks WHERE id = 'a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(stamped.is_some());
        sqlx::query("UPDATE tasks SET done_at = '2026-10-20' WHERE id = 'a'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE tasks SET done_at = '2026-10-21' WHERE id = 'b'")
            .execute(&pool)
            .await
            .unwrap();
        let done = list(&pool, "done").await.unwrap();
        assert_eq!(texts(&done), vec!["B", "A"]);

        set_status(&pool, "a", "open").await.unwrap();
        let (cleared,): (Option<String>,) =
            sqlx::query_as("SELECT done_at FROM tasks WHERE id = 'a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cleared, None);
        assert_eq!(texts(&list(&pool, "open").await.unwrap()), vec!["A"]);
        assert!(set_status(&pool, "a", "archived").await.is_err());
        assert!(set_status(&pool, "missing", "done").await.is_err());
    }

    #[tokio::test]
    async fn add_validates_length() {
        let pool = test_pool().await;
        assert!(add(&pool, "   ").await.is_err());
        assert!(add(&pool, &"x".repeat(MAX_TASK_CHARS + 1)).await.is_err());
        // 500 Cyrillic letters are 1000 bytes: the limit counts chars.
        let longest = "я".repeat(MAX_TASK_CHARS);
        assert_eq!(add(&pool, &longest).await.unwrap().text, longest);

        let row = add(&pool, "  Купить молоко  ").await.unwrap();
        assert_eq!(row.text, "Купить молоко");
        assert_eq!(row.source, "manual");
        assert_eq!(row.meeting_id, None);
        assert_eq!(row.status, "open");
    }

    #[tokio::test]
    async fn reextract_after_user_actions_keeps_them() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        apply_extraction(
            &pool,
            "M",
            &extraction(vec![task("A"), task("B"), task("C")]),
            &stamp(),
        )
        .await
        .unwrap();
        let (a, b, c) = (
            id_of(&pool, "A").await,
            id_of(&pool, "B").await,
            id_of(&pool, "C").await,
        );
        remove(&pool, &a).await.unwrap();
        edit(&pool, &b, "B edited").await.unwrap();
        set_status(&pool, &c, "done").await.unwrap();

        // The regenerated summary repeats A and drops B; D is new.
        let ex = extraction(vec![task("A"), task("C"), task("D")]);
        assert!(apply_extraction(&pool, "M", &ex, &stamp()).await.unwrap());

        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT id, text, status, edited FROM tasks WHERE meeting_id = 'M' ORDER BY text",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (a, "A".to_string(), "dismissed".to_string(), 1),
                (b, "B edited".to_string(), "open".to_string(), 1),
                (c, "C".to_string(), "done".to_string(), 1),
                (
                    id_of(&pool, "D").await,
                    "D".to_string(),
                    "open".to_string(),
                    0
                ),
            ]
        );
    }

    #[tokio::test]
    async fn reextract_after_edit_does_not_duplicate() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        apply_extraction(&pool, "M", &extraction(vec![task("A")]), &stamp())
            .await
            .unwrap();
        edit(&pool, &id_of(&pool, "A").await, "A changed")
            .await
            .unwrap();

        apply_extraction(&pool, "M", &extraction(vec![task("A")]), &stamp())
            .await
            .unwrap();

        let rows: Vec<(String,)> = sqlx::query_as("SELECT text FROM tasks WHERE meeting_id = 'M'")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(rows, vec![("A changed".to_string(),)]);
    }

    #[tokio::test]
    async fn reopened_task_survives_reextraction() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        apply_extraction(&pool, "M", &extraction(vec![task("X")]), &stamp())
            .await
            .unwrap();
        let x = id_of(&pool, "X").await;
        set_status(&pool, &x, "done").await.unwrap();
        set_status(&pool, &x, "open").await.unwrap();

        apply_extraction(&pool, "M", &extraction(vec![]), &stamp())
            .await
            .unwrap();

        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, status FROM tasks WHERE meeting_id = 'M'")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows, vec![(x, "open".to_string())]);
    }
}
