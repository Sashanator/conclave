use crate::api::{TranscriptSearchResult, TranscriptSegment};
use crate::speakers::sentences::{is_valid, Sentence};
use crate::database::repositories::meeting_notes::{
    sanitize_mark, sanitize_notes, MeetingNote, MeetingNotesRepository,
};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        notes: &[MeetingNote],
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting '{}': {}", meeting_title, e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            // Sentences that do not match the saved line are dropped; the line itself still saves
            let sentences = match (&segment.sentences, segment.audio_start_time, segment.audio_end_time) {
                (Some(s), Some(start), Some(end)) if is_valid(s, &segment.text, start, end) => serde_json::to_string(s).ok(),
                _ => None,
            };
            let result = sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, mark, speaker_idx, sentences)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .bind(sanitize_mark(segment.mark.as_deref()))
            .bind(segment.speaker_idx)
            .bind(sentences)
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!(
                    "Failed to save transcript segment for meeting {}: {}",
                    meeting_id, e
                );
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        // 3. Save the notes in the same transaction; a note never fails the save
        let notes = sanitize_notes(notes);
        if !notes.is_empty() {
            if let Err(e) = MeetingNotesRepository::save(&mut *transaction, &meeting_id, &notes).await {
                error!("Failed to save notes for meeting {}: {}", meeting_id, e);
                transaction.rollback().await?;
                return Err(e);
            }
        }

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// Sets or clears the mark on one transcript line. Returns false if the line does not exist.
    pub async fn set_mark(
        pool: &SqlitePool,
        transcript_id: &str,
        mark: Option<&str>,
    ) -> Result<bool, SqlxError> {
        let r = sqlx::query("UPDATE transcripts SET mark = ? WHERE id = ?")
            .bind(mark)
            .bind(transcript_id)
            .execute(pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Searches for a query string within the transcripts.
    /// It returns a list of matching transcripts with context.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let search_query = format!("%{}%", query.to_lowercase());

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT m.id, m.title, t.transcript, t.timestamp
             FROM meetings m
             JOIN transcripts t ON m.id = t.meeting_id
             WHERE LOWER(t.transcript) LIKE ?",
        )
        .bind(&search_query)
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|(id, title, transcript, timestamp)| {
                let match_context = Self::get_match_context(&transcript, query);
                TranscriptSearchResult {
                    id,
                    title,
                    match_context,
                    timestamp,
                }
            })
            .collect();

        Ok(results)
    }

    /// Helper function to extract a snippet of text around the first match of a query.
    fn get_match_context(transcript: &str, query: &str) -> String {
        let transcript_lower = transcript.to_lowercase();
        let query_lower = query.to_lowercase();

        match transcript_lower.find(&query_lower) {
            Some(match_index) => {
                let start_index = match_index.saturating_sub(100);
                let end_index = (match_index + query.len() + 100).min(transcript.len());

                let mut context = String::new();
                if start_index > 0 {
                    context.push_str("...");
                }
                context.push_str(&transcript[start_index..end_index]);
                if end_index < transcript.len() {
                    context.push_str("...");
                }
                context
            }
            None => transcript.chars().take(200).collect(), // Fallback to the start of the transcript
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::meeting_notes::{MeetingNote, MeetingNotesRepository};

    fn seg(text: &str, start: f64, mark: Option<&str>) -> TranscriptSegment {
        TranscriptSegment {
            id: "x".into(), text: text.into(), timestamp: "t".into(),
            audio_start_time: Some(start), audio_end_time: Some(start + 1.0), duration: Some(1.0),
            mark: mark.map(str::to_string), sentences: None, speaker_idx: None,
        }
    }

    #[tokio::test]
    async fn save_transcript_stores_sentences_and_self_label() {
        let pool = crate::database::repositories::test_pool().await;
        let mut line = seg("Hi.", 0.0, None);
        line.sentences = Some(vec![Sentence(0.0, 1.0, "Hi.".into())]);
        line.speaker_idx = Some(0);
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[line], None, &[]).await.unwrap();
        let (idx, sentences): (Option<i64>, Option<String>) = sqlx::query_as("SELECT speaker_idx, sentences FROM transcripts WHERE meeting_id = ?")
            .bind(&id).fetch_one(&pool).await.unwrap();
        assert_eq!(idx, Some(0));
        assert_eq!(sentences.as_deref(), Some(r#"[[0.0,1.0,"Hi."]]"#));
    }

    #[tokio::test]
    async fn save_transcript_drops_bad_sentences_and_speaker_idx() {
        let pool = crate::database::repositories::test_pool().await;
        // Malformed sentences and an out-of-range speaker from the webview are dropped while parsing
        let malformed: TranscriptSegment = serde_json::from_value(serde_json::json!({
            "id": "x", "text": "a", "timestamp": "t", "audio_start_time": 0.0, "audio_end_time": 1.0, "duration": 1.0,
            "sentences": [[1, "x", "a"]], "speaker_idx": 7
        })).unwrap();
        // Well-formed sentences whose text does not match the line are dropped by save_transcript
        let mut mismatched = seg("a", 2.0, None);
        mismatched.sentences = Some(vec![Sentence(2.0, 3.0, "Other.".into())]);
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[malformed, mismatched], None, &[]).await.unwrap();
        let rows: Vec<(Option<i64>, Option<String>)> = sqlx::query_as("SELECT speaker_idx, sentences FROM transcripts WHERE meeting_id = ?")
            .bind(&id).fetch_all(&pool).await.unwrap();
        assert_eq!(rows, vec![(None, None), (None, None)]);
    }

    #[tokio::test]
    async fn save_transcript_stores_marks_and_notes() {
        let pool = crate::database::repositories::test_pool().await;
        let notes = vec![MeetingNote { id: "note-1".into(), at: Some(1.5), text: "ask".into() }];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, Some("action")), seg("b", 2.0, None)], None, &notes).await.unwrap();
        let marks: Vec<(Option<String>,)> = sqlx::query_as("SELECT mark FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time")
            .bind(&id).fetch_all(&pool).await.unwrap();
        assert_eq!(marks, vec![(Some("action".into()),), (None,)]);
        assert_eq!(MeetingNotesRepository::get(&pool, &id).await.unwrap(), notes);
    }

    #[tokio::test]
    async fn save_transcript_sanitizes_bad_notes_and_marks() {
        let pool = crate::database::repositories::test_pool().await;
        let notes = vec![
            MeetingNote { id: "note-1".into(), at: Some(f64::NAN), text: "  ".into() },
            MeetingNote { id: "note-2".into(), at: None, text: "z".repeat(5000) },
        ];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, Some("nonsense"))], None, &notes).await.unwrap();
        let (mark,): (Option<String>,) = sqlx::query_as("SELECT mark FROM transcripts WHERE meeting_id = ?").bind(&id).fetch_one(&pool).await.unwrap();
        assert_eq!(mark, None);
        let saved = MeetingNotesRepository::get(&pool, &id).await.unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].text.chars().count(), 2000);
    }

    #[tokio::test]
    async fn set_mark_and_keep_audio_round_trip() {
        let pool = crate::database::repositories::test_pool().await;
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, None)], None, &[]).await.unwrap();
        let (tid,): (String,) = sqlx::query_as("SELECT id FROM transcripts WHERE meeting_id = ?").bind(&id).fetch_one(&pool).await.unwrap();
        assert!(TranscriptsRepository::set_mark(&pool, &tid, Some("important")).await.unwrap());
        assert!(!TranscriptsRepository::set_mark(&pool, "nope", Some("important")).await.unwrap());
        let (m,): (Option<String>,) = sqlx::query_as("SELECT mark FROM transcripts WHERE id = ?").bind(&tid).fetch_one(&pool).await.unwrap();
        assert_eq!(m.as_deref(), Some("important"));
        assert!(crate::database::repositories::meeting::MeetingsRepository::set_keep_audio(&pool, &id, true).await.unwrap());
        let meta = crate::database::repositories::meeting::MeetingsRepository::get_meeting_metadata(&pool, &id).await.unwrap().unwrap();
        assert!(meta.keep_audio);
    }

    #[tokio::test]
    async fn delete_meeting_removes_notes() {
        let pool = crate::database::repositories::test_pool().await;
        let notes = vec![MeetingNote { id: "note-1".into(), at: None, text: "x".into() }];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, None)], None, &notes).await.unwrap();
        assert!(crate::database::repositories::meeting::MeetingsRepository::delete_meeting(&pool, &id).await.unwrap());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meeting_notes").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0);
    }
}
