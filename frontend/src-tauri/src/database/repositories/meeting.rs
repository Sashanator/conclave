use crate::api::{MeetingDetails, MeetingTranscript};
use crate::database::models::{MeetingModel, Transcript};
use crate::knowledge::feed;
use chrono::{DateTime, Utc};
use sqlx::{Connection, Error as SqlxError, SqliteConnection, SqlitePool};
use tracing::{error, info};

pub struct MeetingsRepository;

/// Where a new meeting's title came from. The calendar fields are kept only for source `calendar`.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleOrigin {
    pub source: String,
    pub calendar_uid: Option<String>,
    pub calendar_start: Option<String>,
}

impl TitleOrigin {
    /// An unknown source counts as `default`, so a bad value from the frontend never fails a save.
    pub fn new(source: Option<&str>, calendar_uid: Option<String>, calendar_start: Option<String>) -> Self {
        let source = match source {
            Some(s @ ("calendar" | "user")) => s,
            _ => "default",
        };
        let calendar = source == "calendar";
        Self {
            source: source.to_string(),
            calendar_uid: if calendar { calendar_uid } else { None },
            calendar_start: if calendar { calendar_start } else { None },
        }
    }
}

impl Default for TitleOrigin {
    fn default() -> Self {
        Self::new(None, None, None)
    }
}

/// The one SQL expression for "which date a meeting has" (spec 6.1). `alias` is the meetings table alias ("" for none).
/// The columns mix formats (space and T separators), so compare and order through `julianday(..)`, never as text.
pub fn meeting_date_sql(alias: &str) -> String {
    let p = if alias.is_empty() { String::new() } else { format!("{alias}.") };
    format!("COALESCE({p}started_at, {p}calendar_start, {p}created_at)")
}

impl MeetingsRepository {
    pub async fn get_meetings(pool: &SqlitePool) -> Result<Vec<MeetingModel>, sqlx::Error> {
        let sql = format!("SELECT * FROM meetings ORDER BY julianday({}) DESC, rowid DESC", meeting_date_sql(""));
        let meetings = sqlx::query_as::<_, MeetingModel>(&sql).fetch_all(pool).await?;
        Ok(meetings)
    }

    /// The meeting's date (`meeting_date_sql`); `None` when there is no such meeting.
    pub async fn meeting_date(pool: &SqlitePool, id: &str) -> Result<Option<DateTime<Utc>>, SqlxError> {
        let sql = format!("SELECT {} FROM meetings WHERE id = ?", meeting_date_sql(""));
        Ok(sqlx::query_as::<_, (crate::database::models::DateTimeUtc,)>(&sql)
            .bind(id)
            .fetch_optional(pool)
            .await?
            .map(|(d,)| d.0))
    }

    pub async fn delete_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        match delete_meeting_with_transaction(&mut transaction, meeting_id).await {
            Ok(success) => {
                if success {
                    transaction.commit().await?;
                    info!(
                        "Successfully deleted meeting {} and all associated data",
                        meeting_id
                    );
                    Ok(true)
                } else {
                    transaction.rollback().await?;
                    Ok(false)
                }
            }
            Err(e) => {
                let _ = transaction.rollback().await;
                error!("Failed to delete meeting {}: {}", meeting_id, e);
                Err(e)
            }
        }
    }

    pub async fn get_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingDetails>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        // Get meeting details
        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(&mut *transaction)
                .await?;

        if meeting.is_none() {
            transaction.rollback().await?;
            return Err(SqlxError::RowNotFound);
        }

        if let Some(meeting) = meeting {
            // Get all transcripts for this meeting
            let transcripts =
                sqlx::query_as::<_, Transcript>("SELECT * FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time ASC, id ASC")
                    .bind(meeting_id)
                    .fetch_all(&mut *transaction)
                    .await?;

            transaction.commit().await?;

            // Convert Transcript to MeetingTranscript
            let meeting_transcripts = transcripts
                .into_iter()
                .map(|t| MeetingTranscript {
                    id: t.id,
                    text: t.transcript,
                    timestamp: t.timestamp,
                    audio_start_time: t.audio_start_time,
                    audio_end_time: t.audio_end_time,
                    duration: t.duration,
                    mark: t.mark,
                    speaker_idx: t.speaker_idx,
                })
                .collect::<Vec<_>>();

            Ok(Some(MeetingDetails {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                transcripts: meeting_transcripts,
            }))
        } else {
            transaction.rollback().await?;
            Ok(None)
        }
    }

    /// Get meeting metadata without transcripts (for pagination)
    pub async fn get_meeting_metadata(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingModel>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path, keep_audio, audio_deleted_at FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;

        Ok(meeting)
    }

    pub async fn get_title_source(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<String>, SqlxError> {
        sqlx::query_scalar("SELECT title_source FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
    }

    /// When the title_source migration ran here; user titles of older meetings were set by it.
    pub async fn title_source_installed_on(
        pool: &SqlitePool,
    ) -> Result<Option<DateTime<Utc>>, SqlxError> {
        let installed_on: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT CAST(strftime('%s', installed_on) AS INTEGER) FROM _sqlx_migrations WHERE version = ?",
        )
        .bind(20261022000003_i64)
        .fetch_optional(pool)
        .await?;
        Ok(installed_on.flatten().and_then(|secs| DateTime::from_timestamp(secs, 0)))
    }

    /// Pins or unpins a meeting's audio ("Keep audio"). Returns false if the meeting does not exist.
    pub async fn set_keep_audio(pool: &SqlitePool, meeting_id: &str, keep: bool) -> Result<bool, SqlxError> {
        let r = sqlx::query("UPDATE meetings SET keep_audio = ? WHERE id = ?")
            .bind(keep)
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Get meeting transcripts with pagination support
    pub async fn get_meeting_transcripts_paginated(
        pool: &SqlitePool,
        meeting_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Transcript>, i64), SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        // Get total count of transcripts for this meeting
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?"
        )
        .bind(meeting_id)
        .fetch_one(pool)
        .await?;

        // Get paginated transcripts ordered by audio_start_time
        let transcripts = sqlx::query_as::<_, Transcript>(
            "SELECT * FROM transcripts
             WHERE meeting_id = ?
             ORDER BY audio_start_time ASC, id ASC
             LIMIT ? OFFSET ?"
        )
        .bind(meeting_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok((transcripts, total.0))
    }

    pub async fn update_meeting_title(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now().naive_utc();

        let rows_affected =
            sqlx::query("UPDATE meetings SET title = ?, title_source = 'user', updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;
        if rows_affected.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    /// The rename on a caller's connection or transaction, so a summary's completion and its name commit together.
    pub async fn rename_if_default(
        conn: &mut SqliteConnection,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        let now = Utc::now();

        // Update meetings table
        let meeting_update = sqlx::query(
            "UPDATE meetings SET title = ?, title_source = 'summary', updated_at = ? WHERE id = ? AND title_source = 'default'",
        )
        .bind(new_title)
        .bind(now)
        .bind(meeting_id)
        .execute(&mut *conn)
        .await?;

        if meeting_update.rows_affected() == 0 {
            return Ok(false); // Not found, or not a default title
        }

        // Update transcript_chunks table
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(new_title)
            .bind(meeting_id)
            .execute(&mut *conn)
            .await?;
        Ok(true)
    }
}

async fn delete_meeting_with_transaction(
    transaction: &mut SqliteConnection,
    meeting_id: &str,
) -> Result<bool, SqlxError> {
    // Check if meeting exists
    let meeting_exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?;

    if meeting_exists.is_none() {
        error!("Meeting {} not found for deletion", meeting_id);
        return Ok(false);
    }

    // Delete from related tables in proper order
    // 1. Delete from transcript_chunks
    sqlx::query("DELETE FROM transcript_chunks WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 2. Delete from summary_processes
    sqlx::query("DELETE FROM summary_processes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // Explicit, not only the FK cascade (spec §5.4).
    sqlx::query("DELETE FROM meeting_notes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // Explicit, not only the FK (spec §9)
    sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // Explicit, not only the FK (spec §9)
    sqlx::query("DELETE FROM speaker_jobs WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // Explicit, not only the FK (spec §9)
    sqlx::query("UPDATE voiceprints SET source_meeting_id = NULL WHERE source_meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // Explicit: llm_usage has no FK, so its rows are deleted here only.
    sqlx::query("DELETE FROM llm_usage WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // Explicit: tasks and extracts have no FK, so they are deleted here only. Manual tasks (NULL meeting) stay.
    sqlx::query("DELETE FROM tasks WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    sqlx::query("DELETE FROM meeting_extracts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 3. Delete from transcripts
    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // Explicit: a brief's feed item goes with it, in this transaction (spec 6.2). The brief goes with its previous meeting.
    let brief_ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM briefs WHERE previous_meeting_id = ?")
        .bind(meeting_id)
        .fetch_all(&mut *transaction)
        .await?;
    for (brief_id,) in &brief_ids {
        feed::delete_for_target(&mut *transaction, "brief", brief_id).await?;
    }
    sqlx::query("DELETE FROM briefs WHERE previous_meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 4. Finally, delete the meeting
    let result = sqlx::query("DELETE FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use crate::database::repositories::transcript::TranscriptsRepository;
    use sqlx::migrate::Migrator;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::path::Path;

    const SPEAKERS_VERSION: i64 = 20261017000000;

    async fn insert_meeting(pool: &SqlitePool, id: &str) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'M', '2026-10-17 12:00:00', '2026-10-17 12:00:00')")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn insert_line(pool: &SqlitePool, id: &str, meeting_id: &str, start: f64) {
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, ?, 'hi', '2026-10-17', ?)")
            .bind(id)
            .bind(meeting_id)
            .bind(start)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn migration_creates_self_person() {
        let pool = test_pool().await;
        let (id, is_self): (String, i64) = sqlx::query_as("SELECT id, is_self FROM people")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((id.as_str(), is_self), ("self", 1));
        // second self is rejected
        assert!(sqlx::query("INSERT INTO people (id,name,is_self,created_at) VALUES ('x','X',1,'t')")
            .execute(&pool)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn loaders_order_by_start_then_id() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_line(&pool, "b", "M", 2.0).await;
        insert_line(&pool, "c", "M", 1.0).await;
        insert_line(&pool, "a", "M", 1.0).await;

        let full = MeetingsRepository::get_meeting(&pool, "M").await.unwrap().unwrap();
        let full_ids: Vec<&str> = full.transcripts.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(full_ids, ["a", "c", "b"]);

        let (page, total) = MeetingsRepository::get_meeting_transcripts_paginated(&pool, "M", 10, 0)
            .await
            .unwrap();
        let page_ids: Vec<&str> = page.iter().map(|t| t.id.as_str()).collect();
        assert_eq!((page_ids, total), (vec!["a", "c", "b"], 3));
    }

    #[tokio::test]
    async fn delete_meeting_removes_speaker_rows_and_keeps_voiceprints() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_line(&pool, "t1", "M", 1.0).await;
        sqlx::query("INSERT INTO people (id,name,is_self,created_at) VALUES ('p1','Anna',0,'t')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meeting_speakers (meeting_id,idx,person_id,name,centroid,seconds) VALUES ('M',1,'p1','Anna',NULL,10.0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO speaker_jobs (meeting_id,status,run_token,updated_at) VALUES ('M','done','tok','t')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO voiceprints (id,person_id,source_meeting_id,source_idx,vector,seconds,created_at) VALUES ('v1','p1','M',1,x'00',1.0,'t')")
            .execute(&pool)
            .await
            .unwrap();

        assert!(MeetingsRepository::delete_meeting(&pool, "M").await.unwrap());

        let (speakers,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meeting_speakers WHERE meeting_id = 'M'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (jobs,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM speaker_jobs WHERE meeting_id = 'M'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (source,): (Option<String>,) = sqlx::query_as("SELECT source_meeting_id FROM voiceprints WHERE id = 'v1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((speakers, jobs, source), (0, 0, None));
    }

    #[tokio::test]
    async fn delete_meeting_deletes_usage() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_meeting(&pool, "N").await;
        sqlx::query("INSERT INTO llm_usage (at, provider, model, purpose, meeting_id) VALUES ('t','claude','m','ask','M'), ('t','claude','m','ask','N'), ('t','claude','m','ask',NULL)")
            .execute(&pool)
            .await
            .unwrap();

        assert!(MeetingsRepository::delete_meeting(&pool, "M").await.unwrap());

        let (for_m,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM llm_usage WHERE meeting_id = 'M'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (left,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM llm_usage")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((for_m, left), (0, 2));
    }

    #[tokio::test]
    async fn delete_meeting_deletes_tasks_and_extract() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_meeting(&pool, "N").await;
        sqlx::query(
            "INSERT INTO tasks (id, meeting_id, text, owner, status, source, edited, created_at) VALUES \
             ('t1', 'M', 'open', 'me', 'open', 'auto', 0, 't'), ('t2', 'M', 'done', 'me', 'done', 'auto', 0, 't'), \
             ('t3', 'M', 'edited', 'me', 'open', 'auto', 1, 't'), ('t4', NULL, 'manual', 'me', 'open', 'manual', 0, 't'), \
             ('t5', 'N', 'other', 'me', 'open', 'auto', 0, 't')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO meeting_extracts (meeting_id, decisions_json, requests_json, status, extracted_at) VALUES \
             ('M', '[]', '[]', 'done', 't'), ('N', '[]', '[]', 'done', 't')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(MeetingsRepository::delete_meeting(&pool, "M").await.unwrap());

        let left_tasks: Vec<(String,)> = sqlx::query_as("SELECT id FROM tasks ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        let left_extracts: Vec<(String,)> = sqlx::query_as("SELECT meeting_id FROM meeting_extracts")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(left_tasks, vec![("t4".to_string(),), ("t5".to_string(),)]);
        assert_eq!(left_extracts, vec![("N".to_string(),)]);
    }

    const TITLE_SOURCE_VERSION: i64 = 20261022000003;

    async fn insert_titled(pool: &SqlitePool, id: &str, source: &str) {
        insert_meeting(pool, id).await;
        sqlx::query("UPDATE meetings SET title_source = ? WHERE id = ?")
            .bind(source)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn get_title_source_reads_the_column() {
        let pool = test_pool().await;
        insert_titled(&pool, "C", "calendar").await;
        assert_eq!(
            MeetingsRepository::get_title_source(&pool, "C").await.unwrap(),
            Some("calendar".to_string())
        );
        assert_eq!(
            MeetingsRepository::get_title_source(&pool, "missing").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn title_source_installed_on_reads_the_migration_row() {
        let pool = test_pool().await;
        let installed_on = MeetingsRepository::title_source_installed_on(&pool)
            .await
            .unwrap()
            .unwrap();
        let age = Utc::now() - installed_on;
        assert!(age >= chrono::Duration::zero());
        assert!(age < chrono::Duration::hours(1));
    }

    async fn title_source(pool: &SqlitePool, id: &str) -> (String, Option<String>, Option<String>) {
        sqlx::query_as("SELECT title_source, calendar_uid, calendar_start FROM meetings WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn summary_renames_default_only() {
        let pool = test_pool().await;
        insert_titled(&pool, "D", "default").await;
        insert_titled(&pool, "C", "calendar").await;
        insert_titled(&pool, "U", "user").await;
        sqlx::query("INSERT INTO transcript_chunks (meeting_id, meeting_name, transcript_text, model, model_name, created_at) VALUES ('D', 'M', 't', 'm', 'm', 't')")
            .execute(&pool)
            .await
            .unwrap();

        let mut conn = pool.acquire().await.unwrap();
        assert!(MeetingsRepository::rename_if_default(&mut conn, "D", "Summary name").await.unwrap());
        assert!(!MeetingsRepository::rename_if_default(&mut conn, "C", "Summary name").await.unwrap());
        assert!(!MeetingsRepository::rename_if_default(&mut conn, "U", "Summary name").await.unwrap());
        // Once renamed by a summary, a later summary does not rename again.
        assert!(!MeetingsRepository::rename_if_default(&mut conn, "D", "Second").await.unwrap());
        drop(conn); // the in-memory test pool has one connection

        assert_eq!(title_source(&pool, "D").await.0, "summary");
        assert_eq!(title_source(&pool, "C").await.0, "calendar");
        assert_eq!(title_source(&pool, "U").await.0, "user");
        let (title,): (String,) = sqlx::query_as("SELECT title FROM meetings WHERE id = 'D'").fetch_one(&pool).await.unwrap();
        let (chunk_name,): (String,) = sqlx::query_as("SELECT meeting_name FROM transcript_chunks WHERE meeting_id = 'D'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((title.as_str(), chunk_name.as_str()), ("Summary name", "Summary name"));
    }

    #[tokio::test]
    async fn user_rename_sets_user() {
        let pool = test_pool().await;
        insert_titled(&pool, "C", "calendar").await;
        assert!(MeetingsRepository::update_meeting_title(&pool, "C", "Mine").await.unwrap());
        assert_eq!(title_source(&pool, "C").await.0, "user");
    }

    #[tokio::test]
    async fn save_transcript_stores_calendar_fields() {
        let pool = test_pool().await;
        let start = "2026-10-22T09:00:00+00:00";
        let cal = TitleOrigin::new(Some("calendar"), Some("uid-1".into()), Some(start.into()));
        let id = TranscriptsRepository::save_transcript(&pool, "Standup", &[], None, &[], false, &cal).await.unwrap();
        let got = title_source(&pool, &id).await;
        assert_eq!(got, ("calendar".into(), Some("uid-1".into()), Some(start.into())));

        // Calendar fields are kept only for the calendar source.
        let user = TitleOrigin::new(Some("user"), Some("uid-2".into()), Some(start.into()));
        let id = TranscriptsRepository::save_transcript(&pool, "Mine", &[], None, &[], false, &user).await.unwrap();
        assert_eq!(title_source(&pool, &id).await, ("user".into(), None, None));

        // An unknown source is stored as default; the save still succeeds.
        let bogus = TitleOrigin::new(Some("bogus"), Some("uid-3".into()), Some(start.into()));
        let id = TranscriptsRepository::save_transcript(&pool, "Other", &[], None, &[], false, &bogus).await.unwrap();
        assert_eq!(title_source(&pool, &id).await, ("default".into(), None, None));
    }

    #[tokio::test]
    async fn existing_rows_migrate_as_user() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let mut before = Migrator::new(Path::new("./migrations")).await.unwrap();
        before.migrations = before
            .migrations
            .iter()
            .filter(|m| m.version < TITLE_SOURCE_VERSION)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        before.run(&pool).await.unwrap();
        insert_meeting(&pool, "old").await;

        Migrator::new(Path::new("./migrations"))
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();

        assert_eq!(title_source(&pool, "old").await, ("user".into(), None, None));
    }

    #[tokio::test]
    async fn delete_meeting_deletes_briefs_and_feed_items() {
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_meeting(&pool, "N").await;
        sqlx::query(
            "INSERT INTO briefs (id, event_uid, event_start, title, template_id, previous_meeting_id, content_json, created_at) VALUES \
             ('b1', 'u1', 's1', 'One', 'one_on_one', 'M', '{}', 't'), ('b2', 'u2', 's2', 'Two', 'one_on_one', 'N', '{}', 't')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO feed_items (id, kind, title, target_id, created_at) VALUES \
             ('f1', 'brief', 'Brief: One', 'b1', 't'), ('f2', 'brief', 'Brief: Two', 'b2', 't'), ('f3', 'digest', 'Digest', 'b1', 't')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(MeetingsRepository::delete_meeting(&pool, "M").await.unwrap());

        let briefs: Vec<(String,)> = sqlx::query_as("SELECT id FROM briefs ORDER BY id").fetch_all(&pool).await.unwrap();
        let feed_ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM feed_items ORDER BY id").fetch_all(&pool).await.unwrap();
        assert_eq!(briefs, vec![("b2".to_string(),)]);
        // The brief's feed item goes; a digest item that points at the same id stays.
        assert_eq!(feed_ids, vec![("f2".to_string(),), ("f3".to_string(),)]);
    }

    #[tokio::test]
    async fn pre_feature_rows_survive_migration() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // Apply every migration before the speaker one, then insert a pre-feature row.
        let mut before = Migrator::new(Path::new("./migrations")).await.unwrap();
        before.migrations = before
            .migrations
            .iter()
            .filter(|m| m.version < SPEAKERS_VERSION)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        before.run(&pool).await.unwrap();
        insert_meeting(&pool, "M").await;
        insert_line(&pool, "t1", "M", 1.5).await;

        Migrator::new(Path::new("./migrations"))
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();

        let (text, start, idx, sentences): (String, Option<f64>, Option<i64>, Option<String>) =
            sqlx::query_as("SELECT transcript, audio_start_time, speaker_idx, sentences FROM transcripts WHERE id = 't1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((text.as_str(), start, idx, sentences), ("hi", Some(1.5), None, None));
    }
}

#[cfg(test)]
mod redesign_migration_tests {
    use super::*;
    use sqlx::migrate::Migrator;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::path::Path;

    const REDESIGN_VERSION: i64 = 20261116000001;

    async fn exec(pool: &SqlitePool, sql: &str) {
        sqlx::query(sql).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn redesign_migration_on_previous_schema() {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        let mut m = Migrator::new(Path::new("./migrations")).await.unwrap();
        let all = m.migrations.clone();
        m.migrations = all.iter().filter(|x| x.version < REDESIGN_VERSION).cloned().collect::<Vec<_>>().into();
        m.run(&pool).await.unwrap();

        exec(&pool, "INSERT INTO meetings (id,title,created_at,updated_at) VALUES ('old','Old','2026-11-01T10:00:00+00:00','2026-11-01T10:00:00+00:00'),('legacy','Legacy','2026-10-05 12:00:00','2026-10-05 12:00:00')").await;
        exec(&pool, "INSERT INTO meetings (id,title,created_at,updated_at,calendar_uid,calendar_start) VALUES ('cal','Cal','2026-11-02T10:00:00+00:00','2026-11-02T10:00:00+00:00','u1','2026-11-20T09:00:00+00:00')").await;
        exec(&pool, "INSERT INTO transcripts (id,meeting_id,transcript,timestamp,audio_end_time) VALUES ('t1','old','a','x',1200.0),('t2','old','b','x',2700.0),('t3','legacy','c','x',NULL)").await;
        exec(&pool, "INSERT INTO summary_processes (meeting_id,status,created_at,updated_at,result) VALUES ('old','completed','t','t','{\"markdown\":\"## Summary\\nWe agreed. More.\",\"template_id\":\"interview\"}'),('legacy','completed','t','t','not json')").await;
        exec(&pool, "INSERT INTO meeting_preps (event_uid,event_start,event_title,created_at,updated_at) VALUES ('u4','2026-11-23T09:00:00+00:00','T','c','u')").await;
        for (uid, start) in [("u1", "2026-11-20T09:00:00+00:00"), ("u2", "2026-11-21T09:00:00+00:00"), ("u3", "2026-11-22T09:00:00+00:00")] {
            sqlx::query("INSERT INTO meeting_preps (event_uid,event_start,event_title,template_id,material,chat_json,items_json,approved_at,created_at,updated_at) VALUES (?,?,'T','one_on_one','mat','[{\"a\":1}]','[{\"b\":2}]','2026-11-15T00:00:00Z','c','u')")
                .bind(uid).bind(start).execute(&pool).await.unwrap();
        }
        exec(&pool, "DELETE FROM meeting_preps WHERE event_uid='u3'").await;
        exec(&pool, "INSERT INTO meetings (id,title,created_at,updated_at) VALUES ('frac','Frac','2026-11-01T10:00:00.123456789+00:00','2026-11-01T10:00:00.123456789+00:00')").await;
        exec(&pool, "INSERT INTO transcripts (id,meeting_id,transcript,timestamp,audio_end_time) VALUES ('t4','frac','a','x',300.0),('t5','frac','b','x',600.0)").await;
        type Prep = (i64, Option<String>, Option<String>, Option<String>, Option<String>, String, String, String, Option<String>, String, String);
        let prep_sql = "SELECT id,event_uid,event_start,event_title,template_id,material,chat_json,items_json,approved_at,created_at,updated_at FROM meeting_preps ORDER BY id";
        let before: Vec<Prep> = sqlx::query_as(prep_sql).fetch_all(&pool).await.unwrap();
        assert_eq!(before.len(), 3);
        assert!(before[0].4.is_none() && before[0].8.is_none()); // u4: NULL template_id and approved_at
        let (seq_before,): (i64,) = sqlx::query_as("SELECT seq FROM sqlite_sequence WHERE name='meeting_preps'").fetch_one(&pool).await.unwrap();
        assert!(seq_before > before[2].0); // u3 was the newest row when deleted

        Migrator::new(Path::new("./migrations")).await.unwrap().run(&pool).await.unwrap();

        let (n_unreviewed,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meetings WHERE reviewed <> 1").fetch_one(&pool).await.unwrap();
        assert_eq!(n_unreviewed, 0);
        type Row = (Option<String>, Option<String>, Option<String>, String);
        let row = |id: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query_as::<_, Row>("SELECT template_id, started_at, ended_at, created_at FROM meetings WHERE id=?")
                    .bind(id).fetch_one(&pool).await.unwrap()
            }
        };
        let old = row("old").await;
        assert_eq!(old.0.as_deref(), Some("interview"));
        assert_eq!(old.2.as_deref(), Some(old.3.as_str()));
        let started = chrono::DateTime::parse_from_rfc3339(old.1.as_deref().unwrap()).unwrap();
        assert_eq!(started.with_timezone(&Utc).to_rfc3339(), "2026-11-01T09:15:00+00:00");
        let legacy = row("legacy").await;
        let frac = row("frac").await;
        let started = chrono::DateTime::parse_from_rfc3339(frac.1.as_deref().unwrap()).unwrap();
        let created = chrono::DateTime::parse_from_rfc3339(&frac.3).unwrap();
        assert_eq!((created - chrono::Duration::seconds(600) - started).num_milliseconds().abs(), 0);
        assert_eq!(legacy.0, None);
        assert_eq!(legacy.1.as_deref(), Some(legacy.3.as_str()));

        let after: Vec<Prep> = sqlx::query_as(prep_sql).fetch_all(&pool).await.unwrap();
        assert_eq!(after, before);
        let (linked,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meeting_preps WHERE meeting_id IS NOT NULL").fetch_one(&pool).await.unwrap();
        assert_eq!(linked, 0);
        let (seq_after,): (i64,) = sqlx::query_as("SELECT seq FROM sqlite_sequence WHERE name='meeting_preps'").fetch_one(&pool).await.unwrap();
        assert_eq!(seq_after, seq_before);
        let (leftover,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sqlite_sequence WHERE name='meeting_preps_new'").fetch_one(&pool).await.unwrap();
        assert_eq!(leftover, 0);

        let ins = "INSERT INTO meeting_preps (created_at,updated_at,meeting_id) VALUES ('c','u',?)";
        sqlx::query(ins).bind("cal").execute(&pool).await.unwrap();
        let (new_id,): (i64,) = sqlx::query_as("SELECT MAX(id) FROM meeting_preps").fetch_one(&pool).await.unwrap();
        assert!(new_id > seq_before);
        sqlx::query(ins).bind("old").execute(&pool).await.unwrap(); // second NULL-event prep
        assert!(sqlx::query(ins).bind("cal").execute(&pool).await.is_err());

        let views: Vec<(String, i64, i64)> = sqlx::query_as("SELECT id, position, builtin FROM meeting_views ORDER BY position").fetch_all(&pool).await.unwrap();
        assert_eq!(views, vec![("all".into(), 0, 1), ("week".into(), 1, 1), ("unreviewed".into(), 2, 1)]);
        let dis = "INSERT INTO dismissed_occurrences (calendar_uid,original_start,dismissed_at) VALUES ('u1','s','n')";
        exec(&pool, dis).await;
        assert!(sqlx::query(dis).execute(&pool).await.is_err());
    }
}


#[cfg(test)]
mod meeting_date_tests {
    use super::*;
    use crate::database::repositories::test_pool;

    async fn row(pool: &SqlitePool, id: &str, created: &str, started: Option<&str>, cal: Option<&str>) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, started_at, calendar_start) VALUES (?, 'T', ?, ?, ?, ?)")
            .bind(id).bind(created).bind(created).bind(started).bind(cal)
            .execute(pool).await.unwrap();
    }

    #[test]
    fn the_expression_follows_the_alias() {
        assert_eq!(meeting_date_sql("m"), "COALESCE(m.started_at, m.calendar_start, m.created_at)");
        assert_eq!(meeting_date_sql(""), "COALESCE(started_at, calendar_start, created_at)");
    }

    #[tokio::test]
    async fn meetings_order_and_date_follow_started_at_then_calendar_then_created() {
        let pool = test_pool().await;
        row(&pool, "moved", "2026-11-01 10:00:00.000+00:00", Some("2026-11-09T10:00:00.000+00:00"), None).await;
        row(&pool, "mid", "2026-11-05 10:00:00.000+00:00", None, None).await;
        row(&pool, "cal", "2026-11-02 10:00:00.000+00:00", None, Some("2026-11-07T10:00:00+00:00")).await;
        // Same day, mixed formats: the T-format 09:00 is earlier than the space-format 10:00 (text order would say later).
        row(&pool, "t9", "2026-10-20 08:00:00.000+00:00", Some("2026-10-20T09:00:00.000+00:00"), None).await;
        row(&pool, "s10", "2026-10-20 10:00:00.000+00:00", None, None).await;
        let ids: Vec<String> = MeetingsRepository::get_meetings(&pool).await.unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["moved", "cal", "mid", "s10", "t9"]);
        let d = MeetingsRepository::meeting_date(&pool, "moved").await.unwrap().unwrap();
        assert_eq!(d.to_rfc3339(), "2026-11-09T10:00:00+00:00");
        let d = MeetingsRepository::meeting_date(&pool, "cal").await.unwrap().unwrap();
        assert_eq!(d.to_rfc3339(), "2026-11-07T10:00:00+00:00");
        assert!(MeetingsRepository::meeting_date(&pool, "nope").await.unwrap().is_none());
    }
}
