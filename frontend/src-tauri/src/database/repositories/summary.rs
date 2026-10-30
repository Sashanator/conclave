use crate::database::models::SummaryProcess;
use crate::database::repositories::meeting::MeetingsRepository;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::SqlitePool;
use tracing::{error, info as log_info};

pub struct SummaryProcessesRepository;

impl SummaryProcessesRepository {
    /// Run at startup: a summary still pending after a crash or quit never finishes. Marks it failed
    /// and restores the backed-up summary, as a cancelled regeneration would. Returns the row count.
    pub async fn fail_interrupted(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
        let now = Utc::now();
        let done = sqlx::query(
            "UPDATE summary_processes SET status = 'failed', error = 'Interrupted', updated_at = ?, end_time = ?, result = COALESCE(result_backup, result), result_backup = NULL, result_backup_timestamp = NULL WHERE LOWER(status) = 'pending'",
        )
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// Retrieves the current summary process state for a given meeting ID.
    pub async fn get_summary_data(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>("SELECT * FROM summary_processes WHERE meeting_id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
    }

    pub async fn update_meeting_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        summary: &Value,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = pool.begin().await?;

        let meeting_exists: bool = sqlx::query("SELECT 1 FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(&mut *transaction)
            .await?
            .is_some();

        if !meeting_exists {
            log_info!(
                "Attempted to save summary for a non-existent meeting_id: {}",
                meeting_id
            );
            transaction.rollback().await?;
            return Ok(false);
        }

        let existing: Option<String> =
            sqlx::query_scalar("SELECT result FROM summary_processes WHERE meeting_id = ?")
                .bind(meeting_id)
                .fetch_optional(&mut *transaction)
                .await?
                .flatten();
        let merged = carry_summary_metadata(existing.as_deref(), summary.clone());
        let result_json = serde_json::to_string(&merged);
        if result_json.is_err() {
            error!("Can't convert the json to string for saving to Database");
            transaction.rollback().await?;
            return Ok(false);
        }
        let now = Utc::now();
        let update = sqlx::query("UPDATE summary_processes SET result = ?, updated_at = ? WHERE meeting_id = ?")
            .bind(&result_json.unwrap())
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;
        if update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }

        sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;

        log_info!(
            "Successfully updated summary and timestamp for meeting_id: {}",
            meeting_id
        );
        Ok(true)
    }

    pub async fn get_summary_data_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>(
            "SELECT p.* FROM summary_processes p JOIN transcript_chunks t ON p.meeting_id = t.meeting_id WHERE p.meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn create_or_reset_process(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        log_info!(
            "Creating or resetting summary process for meeting_id: {}",
            meeting_id
        );
        sqlx::query(
            r#"
            INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, start_time, result, error)
            VALUES (?, 'PENDING', ?, ?, ?, NULL, NULL)
            ON CONFLICT(meeting_id) DO UPDATE SET
                status = 'PENDING',
                updated_at = excluded.updated_at,
                start_time = excluded.start_time,
                result_backup = result,
                result_backup_timestamp = excluded.updated_at,
                result = result,
                error = NULL
            "#
        )
        .bind(meeting_id)
        .bind(started_at)
        .bind(started_at)
        .bind(started_at)
        .execute(pool)
        .await?;
        log_info!(
            "Backed up existing summary before regeneration for meeting_id: {}",
            meeting_id
        );
        Ok(())
    }

    /// Marks the process completed. A summary's `name` renames a default-titled meeting in the same
    /// transaction, so a poll that sees `completed` also sees the final title. Nothing is renamed
    /// when the completion is stale.
    pub async fn update_process_completed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        result: Value,
        chunk_count: i64,
        processing_time: f64,
        name: Option<&str>,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();
        let result_str = serde_json::to_string(&result)
            .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize result: {}", e)))?;

        let mut transaction = pool.begin().await?;
        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET status = 'completed', result = ?, updated_at = ?, end_time = ?, chunk_count = ?, processing_time = ?, error = NULL, result_backup = NULL, result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#
        )
        .bind(result_str)
        .bind(now)
        .bind(now)
        .bind(chunk_count)
        .bind(processing_time)
        .bind(meeting_id)
        .bind(started_at)
        .execute(&mut *transaction)
        .await?;
        let completed = update.rows_affected() == 1;
        if let (true, Some(name)) = (completed, name) {
            MeetingsRepository::rename_if_default(&mut *transaction, meeting_id, name).await?;
        }
        transaction.commit().await?;
        Ok(completed)
    }

    pub async fn update_process_failed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        error: &str,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'failed',
                error = ?,
                updated_at = ?,
                end_time = ?,
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#,
        )
        .bind(error)
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?;
        Ok(update.rows_affected() == 1)
    }

    pub async fn update_process_cancelled(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        let update = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'cancelled',
                updated_at = ?,
                end_time = ?,
                error = 'Generation was cancelled by user',
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?;
        Ok(update.rows_affected() == 1)
    }
}

/// Keys written by summary generation. An edit sends only the summary body, so these
/// are copied from the stored result when the edit does not set them.
const CARRIED_SUMMARY_KEYS: [&str; 6] = [
    "template_id",
    "template_name",
    "template_source",
    "summary_language",
    "language_fallback",
    "reasoning_stripped",
];

fn carry_summary_metadata(old_result: Option<&str>, mut new: Value) -> Value {
    // Only the stored result may supply the generated baseline, never an edit.
    if let Some(new_object) = new.as_object_mut() {
        new_object.remove("generated_markdown");
    }
    let Some(old) = old_result.and_then(|raw| serde_json::from_str::<Value>(raw).ok()) else {
        return new;
    };
    if let (Some(old), Some(new_object)) = (old.as_object(), new.as_object_mut()) {
        if let Some(value) = old.get("generated_markdown") {
            new_object.insert("generated_markdown".to_string(), value.clone());
        }
        for key in CARRIED_SUMMARY_KEYS {
            if let Some(value) = old.get(key).filter(|_| !new_object.contains_key(key)) {
                new_object.insert(key.to_string(), value.clone());
            }
        }
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn generated_metadata() -> Value {
        json!({
            "markdown": "old body",
            "template_id": "german_lessons",
            "template_name": "German lessons",
            "template_source": "auto",
            "summary_language": "de",
            "language_fallback": true,
            "reasoning_stripped": true
        })
    }

    #[test]
    fn carry_summary_metadata_copies_missing_keys() {
        let old = generated_metadata().to_string();
        let merged = carry_summary_metadata(Some(&old), json!({"markdown": "edited"}));
        assert_eq!(merged["markdown"], "edited");
        assert_eq!(merged["template_id"], "german_lessons");
        assert_eq!(merged["template_name"], "German lessons");
        assert_eq!(merged["template_source"], "auto");
        assert_eq!(merged["summary_language"], "de");
        assert_eq!(merged["language_fallback"], true);
        assert_eq!(merged["reasoning_stripped"], true);
    }

    #[test]
    fn carry_summary_metadata_keeps_keys_the_edit_sets() {
        let old = generated_metadata().to_string();
        let merged = carry_summary_metadata(
            Some(&old),
            json!({"markdown": "edited", "template_id": "standard_meeting", "language_fallback": false}),
        );
        assert_eq!(merged["template_id"], "standard_meeting");
        assert_eq!(merged["language_fallback"], false);
        assert_eq!(merged["template_name"], "German lessons");
    }

    #[test]
    fn carry_summary_metadata_leaves_old_row_without_keys_unchanged() {
        let old = json!({"markdown": "old body"}).to_string();
        let new = json!({"markdown": "edited"});
        assert_eq!(carry_summary_metadata(Some(&old), new.clone()), new);
        assert_eq!(carry_summary_metadata(None, new.clone()), new);
    }

    #[test]
    fn carry_summary_metadata_ignores_invalid_old_json() {
        let new = json!({"markdown": "edited"});
        assert_eq!(carry_summary_metadata(Some("not json {"), new.clone()), new);
    }

    #[test]
    fn edit_keeps_stored_generated_markdown() {
        let merged = carry_summary_metadata(
            Some(r#"{"markdown":"g","generated_markdown":"g"}"#),
            json!({"markdown": "edited", "generated_markdown": "stale"}),
        );
        assert_eq!(merged["generated_markdown"], "g");
        assert_eq!(merged["markdown"], "edited");
    }

    #[test]
    fn edit_on_old_row_has_no_generated_markdown() {
        let merged = carry_summary_metadata(
            Some(r#"{"markdown":"g"}"#),
            json!({"markdown": "e", "generated_markdown": "x"}),
        );
        assert!(merged.get("generated_markdown").is_none());
        assert_eq!(merged["markdown"], "e");
    }

    #[test]
    fn carry_summary_metadata_drops_generated_markdown_without_readable_old_row() {
        for old in [None, Some("not json {")] {
            let merged = carry_summary_metadata(
                old,
                json!({"markdown": "edited", "generated_markdown": "stale"}),
            );
            assert!(merged.get("generated_markdown").is_none());
            assert_eq!(merged["markdown"], "edited");
        }
    }

    #[tokio::test]
    async fn generated_markdown_survives_save_and_is_replaced_on_regeneration() {
        let pool = crate::database::repositories::test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m', 'Meeting', '2026-10-30 09:00:00', '2026-10-30 09:00:00')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) VALUES ('m', 'completed', '2026-10-30 09:00:00', '2026-10-30 09:00:00', ?)")
            .bind(json!({"markdown": "g1", "generated_markdown": "g1"}).to_string())
            .execute(&pool).await.unwrap();

        assert!(SummaryProcessesRepository::update_meeting_summary(
            &pool, "m", &json!({"markdown": "edited"}),
        ).await.unwrap());
        let saved = SummaryProcessesRepository::get_summary_data(&pool, "m")
            .await.unwrap().unwrap();
        let saved: Value = serde_json::from_str(&saved.result.unwrap()).unwrap();
        assert_eq!(saved["generated_markdown"], "g1");
        assert_eq!(saved["markdown"], "edited");

        let started = Utc::now();
        SummaryProcessesRepository::create_or_reset_process(&pool, "m", started)
            .await.unwrap();
        assert!(SummaryProcessesRepository::update_process_completed(
            &pool, "m", started, json!({"markdown": "g2", "generated_markdown": "g2"}),
            1, 1.0, None,
        ).await.unwrap());
        let regenerated = SummaryProcessesRepository::get_summary_data(&pool, "m")
            .await.unwrap().unwrap();
        let regenerated: Value = serde_json::from_str(&regenerated.result.unwrap()).unwrap();
        assert_eq!(regenerated["generated_markdown"], "g2");
        assert_eq!(regenerated["markdown"], "g2");
    }

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE summary_processes (
                meeting_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                created_at TEXT,
                updated_at TEXT,
                start_time TEXT,
                result TEXT,
                result_backup TEXT,
                result_backup_timestamp TEXT,
                end_time TEXT,
                chunk_count INTEGER,
                processing_time REAL,
                error TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn seed_pending(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        result: Option<&str>,
        result_backup: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO summary_processes (
                meeting_id, status, created_at, updated_at, start_time, result, result_backup
            ) VALUES (?, 'PENDING', ?, ?, ?, ?, ?)",
        )
        .bind(meeting_id)
        .bind(started_at)
        .bind(started_at)
        .bind(started_at)
        .bind(result)
        .bind(result_backup)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn startup_fails_interrupted_summaries_and_restores_backup() {
        let pool = test_pool().await;
        let started = Utc::now();
        seed_pending(&pool, "regen", started, Some("partial"), Some("old")).await;
        seed_pending(&pool, "first", started, None, None).await;
        sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) VALUES ('done', 'completed', 't', 't', 'kept')")
            .execute(&pool).await.unwrap();

        assert_eq!(SummaryProcessesRepository::fail_interrupted(&pool).await.unwrap(), 2);

        let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT meeting_id, status, error, result, result_backup FROM summary_processes ORDER BY meeting_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows, vec![
            ("done".into(), "completed".into(), None, Some("kept".into()), None),
            ("first".into(), "failed".into(), Some("Interrupted".into()), None, None),
            ("regen".into(), "failed".into(), Some("Interrupted".into()), Some("old".into()), None),
        ]);
        // The failed rows are stamped with the time of the sweep, as update_process_failed does
        let stamps: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT meeting_id, updated_at, end_time FROM summary_processes WHERE status = 'failed' ORDER BY meeting_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        for (id, updated, ended) in stamps {
            for stamp in [updated, ended] {
                let at = DateTime::parse_from_rfc3339(&stamp.expect("timestamp set")).unwrap().with_timezone(&Utc);
                assert!(at >= started, "{id} stamped before the sweep");
            }
        }
    }

    #[tokio::test]
    async fn completion_renames_only_a_default_title_in_the_same_write() {
        // The migrated schema: this test needs the meetings table too.
        let pool = crate::database::repositories::test_pool().await;
        for (id, source) in [("default-m", "default"), ("calendar-m", "calendar"), ("stale-m", "default")] {
            sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, title_source) VALUES (?, 'Original', '2026-10-22 09:00:00', '2026-10-22 09:00:00', ?)")
                .bind(id)
                .bind(source)
                .execute(&pool)
                .await
                .unwrap();
        }
        let started = Utc::now();
        seed_pending(&pool, "default-m", started, None, None).await;
        seed_pending(&pool, "calendar-m", started, None, None).await;

        assert!(SummaryProcessesRepository::update_process_completed(
            &pool, "default-m", started, json!({"markdown": "x"}), 1, 1.0, Some("Summary name"),
        ).await.unwrap());
        assert!(SummaryProcessesRepository::update_process_completed(
            &pool, "calendar-m", started, json!({"markdown": "x"}), 1, 1.0, Some("Summary name"),
        ).await.unwrap());
        // No pending row for stale-m: the completion is stale, so nothing is renamed.
        assert!(!SummaryProcessesRepository::update_process_completed(
            &pool, "stale-m", started, json!({"markdown": "x"}), 1, 1.0, Some("Summary name"),
        ).await.unwrap());

        let titles: Vec<(String, String)> = sqlx::query_as("SELECT id, title FROM meetings ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(
            titles,
            vec![
                ("calendar-m".to_string(), "Original".to_string()),
                ("default-m".to_string(), "Summary name".to_string()),
                ("stale-m".to_string(), "Original".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn terminal_compare_and_set_preserves_first_writer() {
        let pool = test_pool().await;
        let completed_start = Utc::now();
        seed_pending(&pool, "completed-first", completed_start, None, None).await;
        assert!(
            SummaryProcessesRepository::update_process_completed(
                &pool,
                "completed-first",
                completed_start,
                json!({"markdown": "completed"}),
                1,
                1.0,
                None,
            )
            .await
            .unwrap()
        );
        assert!(
            !SummaryProcessesRepository::update_process_cancelled(
                &pool,
                "completed-first",
                completed_start,
            )
            .await
            .unwrap()
        );
        let completed_status: String = sqlx::query_scalar(
            "SELECT status FROM summary_processes WHERE meeting_id = 'completed-first'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(completed_status, "completed");

        let cancelled_start = completed_start + chrono::Duration::nanoseconds(1);
        let previous = r#"{"markdown":"previous"}"#;
        seed_pending(
            &pool,
            "cancelled-first",
            cancelled_start,
            Some(previous),
            Some(previous),
        )
        .await;
        assert!(
            SummaryProcessesRepository::update_process_cancelled(
                &pool,
                "cancelled-first",
                cancelled_start,
            )
            .await
            .unwrap()
        );
        assert!(
            !SummaryProcessesRepository::update_process_completed(
                &pool,
                "cancelled-first",
                cancelled_start,
                json!({"markdown": "completed"}),
                1,
                1.0,
                None,
            )
            .await
            .unwrap()
        );
        let cancelled: (String, String) = sqlx::query_as(
            "SELECT status, result FROM summary_processes WHERE meeting_id = 'cancelled-first'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cancelled, ("cancelled".to_string(), previous.to_string()));
    }

    #[tokio::test]
    async fn cancelled_compare_and_set_rejects_stale_start_time() {
        let pool = test_pool().await;
        let current_start = Utc::now();
        seed_pending(&pool, "stale-cancel", current_start, None, None).await;
        assert!(
            !SummaryProcessesRepository::update_process_cancelled(
                &pool,
                "stale-cancel",
                current_start - chrono::Duration::nanoseconds(1),
            )
            .await
            .unwrap()
        );
        let status: String =
            sqlx::query_scalar("SELECT status FROM summary_processes WHERE meeting_id = 'stale-cancel'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "PENDING");
    }
}
