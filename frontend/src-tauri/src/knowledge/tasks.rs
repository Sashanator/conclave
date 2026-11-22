//! Tasks from summaries (auto) and by hand, and the re-extraction rule that keeps the user's own edits.

use crate::knowledge::extract::Extraction;
use crate::knowledge::MAX_TASK_CHARS;
use crate::state::AppState;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashSet;
use tauri::{Emitter, State};
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
    pub review: String,
    pub removed_by: Option<String>,
    pub dest_id: Option<String>,
    pub due_date: Option<String>,
    pub notion_url: Option<String>,
    pub sent_at: Option<String>,
    pub send_error: Option<String>,
    pub send_ambiguous: bool,
    pub template_id: Option<String>,
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
    // A "Delete recording" mid-run empties the meeting: a stale result must not refill it.
    let has_summary: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM summary_processes WHERE meeting_id = ? LIMIT 1")
        .bind(meeting_id)
        .fetch_optional(&mut *tx)
        .await?;
    if has_summary.is_none() {
        return Ok(false);
    }
    // Only untouched AI output goes: a task the user kept, removed, sent or edited stays (R7).
    sqlx::query(
        "DELETE FROM tasks WHERE meeting_id = ? AND source = 'auto' AND status = 'open' AND edited = 0 \
         AND (review = 'open' OR (review = 'drop' AND removed_by = 'ai'))",
    )
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
    let dest = default_dest_for_meeting(&mut *tx, meeting_id).await?;
    let now = stamp();
    for task in &ex.tasks {
        let key = normalise(&task.text);
        if !seen.insert(key.clone()) {
            continue;
        }
        // Guarded: a meeting deleted mid-run gets no task.
        let unassigned = task.owner == "unassigned";
        sqlx::query(
            "INSERT INTO tasks (id, meeting_id, text, owner, due, at, status, source, edited, created_at, origin_norm, \
             review, removed_by, dest_id, due_date) \
             SELECT ?, ?, ?, ?, ?, ?, 'open', 'auto', 0, ?, ?, ?, ?, ?, ? WHERE EXISTS (SELECT 1 FROM meetings WHERE id = ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(meeting_id)
        .bind(&task.text)
        .bind(&task.owner)
        .bind(&task.due)
        .bind(task.at)
        .bind(&now)
        .bind(&key)
        .bind(if unassigned { "drop" } else { "open" })
        .bind(unassigned.then_some("ai"))
        .bind(&dest)
        .bind(&task.due_date)
        .bind(meeting_id)
        .execute(&mut *tx)
        .await?;
    }
    // Nothing left to review: the meeting counts as reviewed (spec 5.7). Never cleared.
    sqlx::query(
        "UPDATE meetings SET reviewed = 1 WHERE id = ? \
         AND NOT EXISTS (SELECT 1 FROM tasks WHERE meeting_id = ? AND review = 'open')",
    )
    .bind(meeting_id)
    .bind(meeting_id)
    .execute(&mut *tx)
    .await?;
    if let Some(gist) = &ex.gist {
        sqlx::query("UPDATE meetings SET gist = ? WHERE id = ?")
            .bind(gist)
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

/// The destination the meeting's category (template) defaults to; None for "Ask me", no default, or a deleted destination.
pub(crate) async fn default_dest_for_meeting<'c>(
    exec: impl sqlx::SqliteExecutor<'c>,
    meeting_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT d.dest_id FROM meetings m \
         JOIN notion_category_defaults d ON d.template_key = COALESCE(m.template_id, '') \
         JOIN notion_destinations n ON n.id = d.dest_id WHERE m.id = ?",
    )
    .bind(meeting_id)
    .fetch_optional(exec)
    .await
    .map(Option::flatten)
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
           AND EXISTS (SELECT 1 FROM summary_processes WHERE meeting_id = ?) \
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

/// The shared SELECT for `TaskRow`; callers append WHERE/ORDER. The meeting's title and date fall back to the copies
/// kept on the task once its meeting link is cleared.
pub(crate) fn select_sql() -> String {
    let d = crate::database::repositories::meeting::meeting_date_sql("m");
    format!(
        "SELECT t.id, t.meeting_id, COALESCE(m.title, t.meeting_title) AS meeting_title, \
         COALESCE({d}, t.meeting_date) AS meeting_date, t.text, t.owner, t.due, t.at, t.status, t.source, \
         t.created_at, t.review, t.removed_by, t.dest_id, t.due_date, t.notion_url, t.sent_at, t.send_error, \
         t.send_ambiguous, m.template_id \
         FROM tasks t LEFT JOIN meetings m ON m.id = t.meeting_id"
    )
}

/// Open tasks (meeting date desc, then extraction order, manual last), done tasks (latest done first) or dismissed ones (newest first).
pub(crate) async fn list(pool: &SqlitePool, status: &str) -> anyhow::Result<Vec<TaskRow>> {
    let d = crate::database::repositories::meeting::meeting_date_sql("m");
    let open = format!("t.meeting_id IS NULL, julianday({d}) DESC, t.created_at, t.rowid");
    let order = match status {
        "open" => open.as_str(),
        "done" => "t.done_at DESC, t.rowid DESC",
        "dismissed" => "t.rowid DESC",
        _ => anyhow::bail!("unknown task list"),
    };
    let sql = format!("{} WHERE t.status = ? ORDER BY {order}", select_sql());
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
    let rows = sqlx::query(&format!("UPDATE tasks SET text = ?, edited = 1 WHERE id = ? AND {EDITABLE}"))
        .bind(text)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    found_unsent(rows, "task not found or already sent")
}

/// The event the Tasks screen and the sidebar badge listen for (R10).
pub(crate) const TASKS_CHANGED: &str = "tasks-changed";

pub(crate) fn tasks_changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Err(e) = app.emit(TASKS_CHANGED, ()) {
        log::warn!("knowledge: could not emit {TASKS_CHANGED}: {e}");
    }
}

/// Every task with its meeting context, newest meeting first, for the review queue.
pub(crate) async fn queue_list(pool: &SqlitePool) -> anyhow::Result<Vec<TaskRow>> {
    let d = crate::database::repositories::meeting::meeting_date_sql("m");
    let sql = format!(
        "{} ORDER BY julianday(COALESCE({d}, t.meeting_date)) DESC, t.created_at, t.rowid",
        select_sql()
    );
    Ok(sqlx::query_as(&sql).fetch_all(pool).await?)
}

/// Queue writes touch only rows that are not being sent or sent: the send engine owns those (spec 5.1).
const EDITABLE: &str = "review IN ('open', 'keep', 'drop')";

fn found_unsent(rows: u64, what: &str) -> anyhow::Result<()> {
    anyhow::ensure!(rows > 0, "{what}");
    Ok(())
}

pub(crate) async fn set_review(pool: &SqlitePool, id: &str, review: &str) -> anyhow::Result<()> {
    let set = match review {
        "keep" => "review = 'keep', owner = 'me', edited = 1, status = CASE WHEN status = 'dismissed' THEN 'open' ELSE status END, removed_by = NULL",
        "drop" => "review = 'drop', removed_by = 'user', status = 'dismissed', edited = 1",
        "open" => "review = 'open', status = 'open', removed_by = NULL, edited = 1",
        _ => anyhow::bail!("unknown review state"),
    };
    let rows = sqlx::query(&format!("UPDATE tasks SET {set} WHERE id = ? AND {EDITABLE}"))
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    found_unsent(rows, "task not found or already sent")
}

pub(crate) async fn set_dest(pool: &SqlitePool, id: &str, dest_id: Option<&str>) -> anyhow::Result<()> {
    let rows = sqlx::query(&format!(
        "UPDATE tasks SET dest_id = ?1, edited = 1 WHERE id = ?2 AND {EDITABLE} \
         AND (?1 IS NULL OR EXISTS (SELECT 1 FROM notion_destinations WHERE id = ?1))"
    ))
    .bind(dest_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    found_unsent(rows, "task not found, already sent, or unknown database")
}

pub(crate) async fn set_due_date(pool: &SqlitePool, id: &str, due_date: Option<&str>) -> anyhow::Result<()> {
    let due_date = match due_date {
        Some(d) => Some(crate::knowledge::extract::iso_date(d).ok_or_else(|| anyhow::anyhow!("due date must be YYYY-MM-DD"))?),
        None => None,
    };
    let rows = sqlx::query(&format!("UPDATE tasks SET due_date = ?, edited = 1 WHERE id = ? AND {EDITABLE}"))
        .bind(due_date.as_deref())
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    found_unsent(rows, "task not found or already sent")
}

/// The user confirms a task whose send was cut off is already in Notion.
pub(crate) async fn mark_sent(pool: &SqlitePool, id: &str) -> anyhow::Result<()> {
    let rows = sqlx::query(
        "UPDATE tasks SET review = 'sent', send_ambiguous = 0, send_error = NULL, sent_at = ? \
         WHERE id = ? AND send_ambiguous = 1 AND review = 'keep'",
    )
    .bind(stamp())
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    found_unsent(rows, "only a task that may already be in Notion can be marked sent")
}

#[tauri::command]
pub async fn tasks_set_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<(), String> {
    set_status(state.db_manager.pool(), &id, &status)
        .await
        .map_err(|e| e.to_string())?;
    tasks_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn tasks_edit(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> Result<(), String> {
    edit(state.db_manager.pool(), &id, &text)
        .await
        .map_err(|e| e.to_string())?;
    tasks_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn tasks_set_review(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    review: String,
) -> Result<(), String> {
    set_review(state.db_manager.pool(), &id, &review)
        .await
        .map_err(|e| e.to_string())?;
    tasks_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn tasks_set_dest(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    dest_id: Option<String>,
) -> Result<(), String> {
    set_dest(state.db_manager.pool(), &id, dest_id.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    tasks_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn tasks_set_due_date(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    due_date: Option<String>,
) -> Result<(), String> {
    set_due_date(state.db_manager.pool(), &id, due_date.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    tasks_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn tasks_mark_sent(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    mark_sent(state.db_manager.pool(), &id)
        .await
        .map_err(|e| e.to_string())?;
    tasks_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn tasks_queue_list(state: State<'_, AppState>) -> Result<Vec<TaskRow>, String> {
    queue_list(state.db_manager.pool())
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
        sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at) VALUES (?, 'completed', 't', 't')")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn failed_mark_after_delete_recording_writes_nothing() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        assert!(crate::database::repositories::meeting::MeetingsRepository::delete_recording(&pool, "M").await.unwrap());
        assert_eq!(set_extract(&pool, "M", "failed", Some("x"), None, None).await.unwrap(), 0);
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meeting_extracts").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn apply_extraction_after_delete_recording_refills_nothing() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        assert!(crate::database::repositories::meeting::MeetingsRepository::delete_recording(&pool, "M").await.unwrap());
        let mut ex = extraction(vec![task("A")]);
        ex.gist = Some("g".into());
        assert!(!apply_extraction(&pool, "M", &ex, &stamp()).await.unwrap());
        let (tasks, extracts, gist): (i64, i64, Option<String>) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM tasks), (SELECT COUNT(*) FROM meeting_extracts), (SELECT gist FROM meetings WHERE id = 'M')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((tasks, extracts, gist), (0, 0, None));
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
            due_date: None,
            at: None,
        }
    }

    fn extraction(tasks: Vec<ExtractedTask>) -> Extraction {
        Extraction {
            tasks,
            decisions: vec!["Ship it".to_string()],
            requests: vec![],
            gist: None,
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
    async fn apply_extraction_writes_gist() {
        let pool = crate::database::repositories::test_pool().await;
        insert_meeting(&pool, "M").await;
        sqlx::query("UPDATE meetings SET gist = 'fallback' WHERE id = 'M'").execute(&pool).await.unwrap();
        let mut ex = extraction(vec![]);
        assert!(apply_extraction(&pool, "M", &ex, &stamp()).await.unwrap());
        let gist = || sqlx::query_scalar::<_, Option<String>>("SELECT gist FROM meetings WHERE id = 'M'").fetch_one(&pool);
        assert_eq!(gist().await.unwrap().as_deref(), Some("fallback"));
        ex.gist = Some("New gist".to_string());
        assert!(apply_extraction(&pool, "M", &ex, &stamp()).await.unwrap());
        assert_eq!(gist().await.unwrap().as_deref(), Some("New gist"));
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

    async fn add_dest(pool: &SqlitePool, id: &str, template_key: &str, dest: Option<&str>) {
        sqlx::query("INSERT OR IGNORE INTO notion_destinations (id, name, colour, position, database_id, mapping_json, created_at) VALUES (?, ?, 1, 0, 'db', '{}', 't')")
            .bind(id).bind(id).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO notion_category_defaults (template_key, dest_id) VALUES (?, ?)")
            .bind(template_key).bind(dest).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn category_default_destination() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_meeting(&pool, "S").await;
        sqlx::query("UPDATE meetings SET template_id = 'standup' WHERE id = 'S'").execute(&pool).await.unwrap();
        assert_eq!(default_dest_for_meeting(&pool, "M").await.unwrap(), None);
        add_dest(&pool, "job", "", Some("job")).await;
        assert_eq!(default_dest_for_meeting(&pool, "M").await.unwrap().as_deref(), Some("job"));
        add_dest(&pool, "job", "standup", None).await;
        assert_eq!(default_dest_for_meeting(&pool, "S").await.unwrap(), None);
        sqlx::query("DELETE FROM notion_destinations").execute(&pool).await.unwrap();
        assert_eq!(default_dest_for_meeting(&pool, "M").await.unwrap(), None);
    }

    #[tokio::test]
    async fn new_tasks_get_review_state_and_default_destination() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        add_dest(&pool, "job", "", Some("job")).await;
        let mut theirs = task("Theirs");
        theirs.owner = "unassigned".into();
        theirs.due_date = Some("2026-11-26".into());
        apply_extraction(&pool, "M", &extraction(vec![task("Mine"), theirs]), &stamp()).await.unwrap();
        let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT text, review, removed_by, dest_id, due_date FROM tasks ORDER BY rowid",
        ).fetch_all(&pool).await.unwrap();
        let s = |x: &str| Some(x.to_string());
        assert_eq!(rows, vec![
            ("Mine".into(), "open".into(), None, s("job"), None),
            ("Theirs".into(), "drop".into(), s("ai"), s("job"), s("2026-11-26")),
        ]);
    }

    #[tokio::test]
    async fn empty_extraction_marks_meeting_reviewed() {
        let pool = test_pool().await;
        for m in ["E", "T", "O", "P"] {
            insert_meeting(&pool, m).await;
        }
        sqlx::query("UPDATE meetings SET reviewed = 1 WHERE id = 'P'").execute(&pool).await.unwrap();
        let mut theirs = task("Theirs");
        theirs.owner = "unassigned".into();
        for (m, tasks) in [("E", vec![]), ("T", vec![theirs]), ("O", vec![task("Mine")]), ("P", vec![task("Mine")])] {
            assert!(apply_extraction(&pool, m, &extraction(tasks), &stamp()).await.unwrap());
        }
        let rows: Vec<(String, i64)> = sqlx::query_as("SELECT id, reviewed FROM meetings ORDER BY id").fetch_all(&pool).await.unwrap();
        let r: Vec<(&str, i64)> = rows.iter().map(|r| (r.0.as_str(), r.1)).collect();
        assert_eq!(r, vec![("E", 1), ("O", 0), ("P", 1), ("T", 1)]);
    }

    #[tokio::test]
    async fn reextract_replaces_only_untouched_ai_output() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        for (id, text) in [("k", "K"), ("x", "X"), ("s", "S"), ("o", "O"), ("a", "A"), ("g", "G"), ("r", "R")] {
            insert_task(&pool, id, Some("M"), text, "open", "auto", 0).await;
        }
        for (id, review, by) in [("k", "keep", None), ("x", "drop", Some("user")), ("s", "sent", None), ("a", "drop", Some("ai")), ("g", "sending", None), ("r", "drop", None)] {
            sqlx::query("UPDATE tasks SET review = ?, removed_by = ? WHERE id = ?")
                .bind(review).bind(by).bind(id).execute(&pool).await.unwrap();
        }
        let ex = extraction(vec![task("K"), task("X"), task("S"), task("G"), task("R"), task("N")]);
        assert!(apply_extraction(&pool, "M", &ex, &stamp()).await.unwrap());
        let rows: Vec<(String, String, String)> = sqlx::query_as("SELECT text, review, id FROM tasks ORDER BY text")
            .fetch_all(&pool).await.unwrap();
        let r: Vec<(&str, &str)> = rows.iter().map(|r| (r.0.as_str(), r.1.as_str())).collect();
        assert_eq!(r, vec![("G", "sending"), ("K", "keep"), ("N", "open"), ("R", "drop"), ("S", "sent"), ("X", "drop")]);
        let ids: Vec<&str> = rows.iter().filter(|r| r.0 != "N").map(|r| r.2.as_str()).collect();
        assert_eq!(ids, vec!["g", "k", "r", "s", "x"]);
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
        insert_task(&pool, "m", None, "manual", "open", "manual", 0).await;

        let open = list(&pool, "open").await.unwrap();
        assert_eq!(
            texts(&open),
            vec!["new second", "new first", "old task", "manual"]
        );
        assert!(list(&pool, "bogus").await.is_err());
        insert_task(&pool, "x", Some("new"), "removed one", "dismissed", "auto", 0).await;
        assert_eq!(texts(&list(&pool, "dismissed").await.unwrap()), vec!["removed one"]);
    }

    #[tokio::test]
    async fn edit_marks_edited() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_task(&pool, "a", Some("M"), "A", "open", "auto", 0).await;
        insert_task(&pool, "b", Some("M"), "B", "open", "auto", 0).await;
        sqlx::query("UPDATE tasks SET owner = 'unassigned'")
            .execute(&pool)
            .await
            .unwrap();

        edit(&pool, "a", "  A edited  ").await.unwrap();

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
                ("b".to_string(), "B".to_string(), "unassigned".to_string(), 0),
            ]
        );
        assert!(edit(&pool, "missing", "x").await.is_err());
    }

    #[tokio::test]
    async fn edit_refuses_sending_and_sent_rows() {
        let pool = test_pool().await;
        for (id, source, review) in [("a", "auto", "sending"), ("b", "manual", "sent")] {
            insert_task(&pool, id, None, "T", "open", source, 0).await;
            set_col(&pool, id, &format!("review = '{review}'")).await;
            assert!(edit(&pool, id, "new").await.is_err());
        }
        let rows: Vec<(String, String, String)> =
            sqlx::query_as("SELECT id, text, status FROM tasks ORDER BY id").fetch_all(&pool).await.unwrap();
        assert_eq!(rows, vec![("a".into(), "T".into(), "open".into()), ("b".into(), "T".into(), "open".into())]);
    }

    #[tokio::test]
    async fn chosen_destination_survives_reextraction() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        sqlx::query("INSERT INTO notion_destinations (id, name, colour, position, database_id, mapping_json, created_at) VALUES ('d', 'D', 1, 0, 'db', '{}', 't')")
            .execute(&pool).await.unwrap();
        insert_task(&pool, "a", Some("M"), "A", "open", "auto", 0).await;
        set_dest(&pool, "a", Some("d")).await.unwrap();
        assert!(apply_extraction(&pool, "M", &extraction(vec![task("A")]), &stamp()).await.unwrap());
        let rows: Vec<(String, Option<String>)> = sqlx::query_as("SELECT id, dest_id FROM tasks").fetch_all(&pool).await.unwrap();
        assert_eq!(rows, vec![("a".to_string(), Some("d".to_string()))]);
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
    async fn edit_validates_length() {
        let pool = test_pool().await;
        insert_task(&pool, "a", None, "T", "open", "manual", 0).await;
        assert!(edit(&pool, "a", "   ").await.is_err());
        assert!(edit(&pool, "a", &"x".repeat(MAX_TASK_CHARS + 1)).await.is_err());
        // 500 Cyrillic letters are 1000 bytes: the limit counts chars.
        let longest = "я".repeat(MAX_TASK_CHARS);
        edit(&pool, "a", &longest).await.unwrap();
        edit(&pool, "a", "  Купить молоко  ").await.unwrap();
        let (text,): (String,) = sqlx::query_as("SELECT text FROM tasks WHERE id = 'a'").fetch_one(&pool).await.unwrap();
        assert_eq!(text, "Купить молоко");
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
        set_status(&pool, &a, "dismissed").await.unwrap();
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

    async fn row_state(pool: &SqlitePool, id: &str) -> (String, String, String, i64, Option<String>) {
        sqlx::query_as("SELECT review, status, owner, edited, removed_by FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn set_col(pool: &SqlitePool, id: &str, sql: &str) {
        sqlx::query(&format!("UPDATE tasks SET {sql} WHERE id = ?")).bind(id).execute(pool).await.unwrap();
    }

    async fn insert_dest(pool: &SqlitePool, id: &str) {
        sqlx::query("INSERT INTO notion_destinations (id, name, colour, position, database_id, mapping_json, created_at) VALUES (?, ?, 1, 0, 'db', '{}', 't')")
            .bind(id)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    fn st(s: &str, owner: &str, edited: i64, by: Option<&str>) -> (String, String, String, i64, Option<String>) {
        let (review, status) = match s {
            "keep" => ("keep", "open"),
            "drop" => ("drop", "dismissed"),
            _ => ("open", "open"),
        };
        (review.into(), status.into(), owner.into(), edited, by.map(Into::into))
    }

    #[tokio::test]
    async fn keep_remove_restore() {
        let pool = test_pool().await;
        insert_task(&pool, "a", None, "T", "dismissed", "auto", 0).await;
        set_col(&pool, "a", "owner = 'unassigned', review = 'drop', removed_by = 'ai'").await;
        set_review(&pool, "a", "keep").await.unwrap();
        assert_eq!(row_state(&pool, "a").await, st("keep", "me", 1, None));
        set_review(&pool, "a", "drop").await.unwrap();
        assert_eq!(row_state(&pool, "a").await, st("drop", "me", 1, Some("user")));
        set_review(&pool, "a", "open").await.unwrap();
        assert_eq!(row_state(&pool, "a").await, st("open", "me", 1, None));
        assert!(set_review(&pool, "a", "sent").await.is_err());
        assert!(set_review(&pool, "a", "sending").await.is_err());
        // The send engine owns sending and sent rows: no queue command may touch them.
        insert_dest(&pool, "job").await;
        for state in ["sending", "sent"] {
            set_col(&pool, "a", &format!("review = '{state}', dest_id = NULL")).await;
            assert!(set_review(&pool, "a", "keep").await.is_err());
            assert!(set_review(&pool, "a", "drop").await.is_err());
            assert!(set_dest(&pool, "a", None).await.is_err());
            assert!(set_dest(&pool, "a", Some("job")).await.is_err());
            assert!(set_due_date(&pool, "a", None).await.is_err());
            assert!(set_due_date(&pool, "a", Some("2026-11-26")).await.is_err());
            let (r, d): (String, Option<String>) = sqlx::query_as("SELECT review, dest_id FROM tasks WHERE id = 'a'")
                .fetch_one(&pool).await.unwrap();
            assert_eq!((r.as_str(), d), (state, None));
        }
        assert!(set_review(&pool, "missing", "keep").await.is_err());
    }

    #[tokio::test]
    async fn restore_marks_edited_and_keep_does_not_reopen_done() {
        let pool = test_pool().await;
        insert_task(&pool, "a", None, "T", "dismissed", "auto", 0).await;
        set_col(&pool, "a", "review = 'drop', removed_by = 'ai'").await;
        set_review(&pool, "a", "open").await.unwrap();
        assert_eq!(row_state(&pool, "a").await.3, 1);
        set_col(&pool, "a", "status = 'done'").await;
        set_review(&pool, "a", "keep").await.unwrap();
        assert_eq!(row_state(&pool, "a").await.1, "done");
        set_col(&pool, "a", "status = 'dismissed', review = 'drop'").await;
        set_review(&pool, "a", "keep").await.unwrap();
        assert_eq!(row_state(&pool, "a").await.1, "open");
    }

    #[tokio::test]
    async fn due_date_is_stored_normalised() {
        let pool = test_pool().await;
        insert_task(&pool, "a", None, "T", "open", "auto", 0).await;
        set_due_date(&pool, "a", Some(" 2026-11-20 ")).await.unwrap();
        let (due,): (Option<String>,) = sqlx::query_as("SELECT due_date FROM tasks WHERE id = 'a'").fetch_one(&pool).await.unwrap();
        assert_eq!(due.as_deref(), Some("2026-11-20"));
    }

    #[tokio::test]
    async fn destination_and_due_date() {
        let pool = test_pool().await;
        insert_task(&pool, "a", None, "T", "open", "auto", 0).await;
        insert_dest(&pool, "job").await;
        set_dest(&pool, "a", Some("job")).await.unwrap();
        assert!(set_dest(&pool, "a", Some("nope")).await.is_err());
        let (d,): (Option<String>,) = sqlx::query_as("SELECT dest_id FROM tasks WHERE id = 'a'").fetch_one(&pool).await.unwrap();
        assert_eq!(d.as_deref(), Some("job"));
        set_due_date(&pool, "a", Some("2026-11-26")).await.unwrap();
        assert!(set_due_date(&pool, "a", Some("2026-02-30")).await.is_err());
        let (due, edited): (Option<String>, i64) = sqlx::query_as("SELECT due_date, edited FROM tasks WHERE id = 'a'").fetch_one(&pool).await.unwrap();
        assert_eq!((due.as_deref(), edited), (Some("2026-11-26"), 1));
        set_dest(&pool, "a", None).await.unwrap();
        set_due_date(&pool, "a", None).await.unwrap();
        let (d, due): (Option<String>, Option<String>) = sqlx::query_as("SELECT dest_id, due_date FROM tasks WHERE id = 'a'").fetch_one(&pool).await.unwrap();
        assert_eq!((d, due), (None, None));
    }

    #[tokio::test]
    async fn mark_sent_only_for_ambiguous_rows() {
        let pool = test_pool().await;
        insert_task(&pool, "a", None, "A", "open", "auto", 0).await;
        insert_task(&pool, "b", None, "B", "open", "auto", 0).await;
        insert_task(&pool, "c", None, "C", "open", "auto", 0).await;
        set_col(&pool, "a", "review = 'keep', send_ambiguous = 1, send_error = 'x'").await;
        set_col(&pool, "b", "review = 'keep'").await;
        set_col(&pool, "c", "review = 'sending', send_ambiguous = 1").await;
        mark_sent(&pool, "a").await.unwrap();
        let (r, amb, err, at): (String, i64, Option<String>, Option<String>) =
            sqlx::query_as("SELECT review, send_ambiguous, send_error, sent_at FROM tasks WHERE id = 'a'").fetch_one(&pool).await.unwrap();
        assert_eq!((r.as_str(), amb, err), ("sent", 0, None));
        assert!(at.is_some());
        assert!(mark_sent(&pool, "b").await.is_err());
        assert!(mark_sent(&pool, "c").await.is_err());
        set_col(&pool, "b", "review = 'drop', send_ambiguous = 1").await;
        assert!(mark_sent(&pool, "b").await.is_err());
        assert_eq!(row_state(&pool, "b").await.0, "drop");
        assert_eq!(row_state(&pool, "c").await.0, "sending");
    }

    #[tokio::test]
    async fn queue_lists_every_task_with_meeting_context() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        sqlx::query("UPDATE meetings SET template_id = 'standup' WHERE id = 'M'").execute(&pool).await.unwrap();
        insert_task(&pool, "a", Some("M"), "A", "dismissed", "auto", 0).await;
        insert_task(&pool, "b", None, "B", "open", "auto", 0).await;
        sqlx::query("UPDATE tasks SET meeting_title = 'Gone', meeting_date = '2026-01-01', send_ambiguous = 1 WHERE id = 'b'")
            .execute(&pool).await.unwrap();
        let got: Vec<_> = queue_list(&pool).await.unwrap().into_iter()
            .map(|t| (t.id, t.meeting_title, t.template_id, t.status, t.send_ambiguous)).collect();
        assert_eq!(got, vec![
            ("b".into(), Some("Gone".into()), None, "open".into(), true),
            ("a".into(), Some("M".into()), Some("standup".into()), "dismissed".into(), false),
        ]);
    }
}

#[cfg(test)]
mod meeting_date_tests {
    use super::*;
    use crate::database::repositories::test_pool;

    /// A row created 2026-11-01 (space format) but started 2026-11-09 (T format).
    async fn row(pool: &SqlitePool, id: &str, created: &str, started: Option<&str>, cal: Option<&str>) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, started_at, calendar_start) VALUES (?, 'T', ?, ?, ?, ?)")
            .bind(id).bind(created).bind(created).bind(started).bind(cal)
            .execute(pool).await.unwrap();
    }
    async fn exec(pool: &SqlitePool, sql: &str) { sqlx::query(sql).execute(pool).await.unwrap(); }

    #[tokio::test]
    async fn open_tasks_order_by_meeting_date() {
        let pool = test_pool().await;
        row(&pool, "moved", "2026-11-01 10:00:00.000+00:00", Some("2026-11-09T10:00:00.000+00:00"), None).await;
        row(&pool, "mid", "2026-11-05 10:00:00.000+00:00", None, None).await;
        for (id, m) in [("t1", "mid"), ("t2", "moved")] {
            exec(&pool, &format!("INSERT INTO tasks (id, meeting_id, text, owner, status, source, edited, created_at) VALUES ('{id}', '{m}', 'x', 'me', 'open', 'ai', 0, 'c')")).await;
        }
        let rows = list(&pool, "open").await.unwrap();
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["t2", "t1"]);
        assert_eq!(rows[0].meeting_date.as_deref(), Some("2026-11-09T10:00:00.000+00:00"));
    }
}

#[cfg(test)]
mod notion_migration_tests {
    use sqlx::migrate::Migrator;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::SqlitePool;
    use std::path::Path;

    const NOTION_VERSION: i64 = 20261120000001;

    async fn exec(pool: &SqlitePool, sql: &str) {
        sqlx::query(sql).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn tasks_notion_migration_on_previous_schema() {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        let mut m = Migrator::new(Path::new("./migrations")).await.unwrap();
        let all = m.migrations.clone();
        m.migrations = all.iter().filter(|x| x.version < NOTION_VERSION).cloned().collect::<Vec<_>>().into();
        m.run(&pool).await.unwrap();

        for (id, reviewed) in [("R", 1), ("U", 0), ("Z", 0), ("Y", 0), ("F", 0)] {
            exec(&pool, &format!("INSERT INTO meetings (id,title,created_at,updated_at,reviewed) VALUES ('{id}','T','2026-11-01T10:00:00+00:00','2026-11-01T10:00:00+00:00',{reviewed})")).await;
        }
        for (mid, status) in [("Z", "done"), ("Y", "done"), ("F", "failed")] {
            exec(&pool, &format!("INSERT INTO meeting_extracts (meeting_id, decisions_json, requests_json, status, extracted_at) VALUES ('{mid}','[]','[]','{status}','c')")).await;
        }
        let tasks = [
            ("a", "U", "me", "open", "auto", 0), ("b", "R", "me", "open", "auto", 0),
            ("c", "U", "me", "done", "auto", 0), ("d", "U", "me", "dismissed", "auto", 1),
            ("e", "", "me", "open", "auto", 0), ("f", "", "me", "open", "manual", 0),
            ("g", "U", "unassigned", "open", "auto", 0), ("h", "R", "unassigned", "open", "auto", 0),
            ("i", "U", "unassigned", "done", "auto", 0), ("j", "Y", "unassigned", "open", "auto", 0),
        ];
        for (id, mid, owner, status, source, edited) in tasks {
            let mid = if mid.is_empty() { "NULL".to_string() } else { format!("'{mid}'") };
            exec(&pool, &format!("INSERT INTO tasks (id, meeting_id, text, owner, status, source, edited, created_at) VALUES ('{id}', {mid}, 'x', '{owner}', '{status}', '{source}', {edited}, 'c')")).await;
        }
        m.migrations = all;
        m.run(&pool).await.unwrap();

        let rows: Vec<(String, String, Option<String>, String, Option<String>, i64)> =
            sqlx::query_as("SELECT id, review, removed_by, status, dest_id, send_ambiguous FROM tasks ORDER BY id")
                .fetch_all(&pool).await.unwrap();
        let exp = |id: &str, review: &str, by: Option<&str>, status: &str| {
            (id.to_string(), review.to_string(), by.map(String::from), status.to_string(), None, 0i64)
        };
        let expected = vec![
            exp("a", "open", None, "open"), exp("b", "drop", None, "open"), exp("c", "drop", None, "done"),
            exp("d", "drop", None, "dismissed"), exp("e", "open", None, "open"), exp("f", "drop", None, "open"),
            exp("g", "drop", Some("ai"), "open"), exp("h", "drop", Some("ai"), "open"),
            exp("i", "drop", None, "done"), exp("j", "drop", Some("ai"), "open"),
        ];
        assert_eq!(rows, expected);

        let reviewed: Vec<(String, i64)> = sqlx::query_as("SELECT id, reviewed FROM meetings ORDER BY id").fetch_all(&pool).await.unwrap();
        let want = [("F", 0), ("R", 1), ("U", 0), ("Y", 1), ("Z", 1)].map(|(i, r)| (i.to_string(), r));
        assert_eq!(reviewed, want);

        for t in ["notion_destinations", "notion_category_defaults"] {
            let (n,): (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {t}")).fetch_one(&pool).await.unwrap();
            assert_eq!(n, 0);
        }
        sqlx::query("SELECT last_status, last_message FROM notion_destinations").fetch_all(&pool).await.unwrap();
    }
}
