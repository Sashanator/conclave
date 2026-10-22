use crate::api::{MeetingDetails, MeetingTranscript};
use crate::database::models::{MeetingModel, Transcript};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqliteConnection, SqlitePool};
use tracing::{error, info};

pub struct MeetingsRepository;

impl MeetingsRepository {
    pub async fn get_meetings(pool: &SqlitePool) -> Result<Vec<MeetingModel>, sqlx::Error> {
        let meetings =
            sqlx::query_as::<_, MeetingModel>("SELECT * FROM meetings ORDER BY created_at DESC")
                .fetch_all(pool)
                .await?;
        Ok(meetings)
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
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
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

    pub async fn update_meeting_name(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        let mut transaction = pool.begin().await?;
        let now = Utc::now();

        // Update meetings table
        let meeting_update =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;

        if meeting_update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false); // Meeting not found
        }

        // Update transcript_chunks table
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(new_title)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;
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
