//! Tasks from summaries (auto) and by hand, and the re-extraction rule that keeps the user's own edits.

use crate::knowledge::extract::Extraction;
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Serialize, Clone, Debug)]
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
    let kept: Vec<(String,)> = sqlx::query_as("SELECT text FROM tasks WHERE meeting_id = ?")
        .bind(meeting_id)
        .fetch_all(&mut *tx)
        .await?;
    // Also catches a task the model repeats within one reply.
    let mut seen: HashSet<String> = kept.iter().map(|(text,)| normalise(text)).collect();
    let now = stamp();
    for task in &ex.tasks {
        if !seen.insert(normalise(&task.text)) {
            continue;
        }
        // Guarded: a meeting deleted mid-run gets no task.
        sqlx::query(
            "INSERT INTO tasks (id, meeting_id, text, owner, due, at, status, source, edited, created_at) \
             SELECT ?, ?, ?, ?, ?, ?, 'open', 'auto', 0, ? WHERE EXISTS (SELECT 1 FROM meetings WHERE id = ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(meeting_id)
        .bind(&task.text)
        .bind(&task.owner)
        .bind(&task.due)
        .bind(task.at)
        .bind(&now)
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
}
