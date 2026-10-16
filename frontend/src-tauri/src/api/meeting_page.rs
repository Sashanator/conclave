//! Commands for the meeting page: marks, notes, the "Keep audio" pin (spec §5.3).
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::meeting_notes::{validate_mark, validate_notes, MeetingNote, MeetingNotesRepository};
use crate::database::repositories::transcript::TranscriptsRepository;
use crate::state::AppState;
use sqlx::SqlitePool;

pub(crate) async fn set_mark_inner(pool: &SqlitePool, transcript_id: &str, mark: Option<String>) -> Result<(), String> {
    validate_mark(mark.as_deref())?;
    match TranscriptsRepository::set_mark(pool, transcript_id, mark.as_deref()).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!("Transcript line not found: {transcript_id}")),
        Err(e) => Err(format!("Failed to save mark: {e}")),
    }
}

pub(crate) async fn save_notes_inner(pool: &SqlitePool, meeting_id: &str, notes: Vec<MeetingNote>) -> Result<Vec<MeetingNote>, String> {
    let notes = validate_notes(&notes)?;
    let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    if exists.is_none() {
        return Err(format!("Meeting not found: {meeting_id}"));
    }
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    MeetingNotesRepository::save(&mut conn, meeting_id, &notes).await.map_err(|e| format!("Failed to save notes: {e}"))?;
    drop(conn);
    MeetingNotesRepository::get(pool, meeting_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_transcript_mark(state: tauri::State<'_, AppState>, transcript_id: String, mark: Option<String>) -> Result<(), String> {
    set_mark_inner(state.db_manager.pool(), &transcript_id, mark).await
}

#[tauri::command]
pub async fn get_meeting_notes(state: tauri::State<'_, AppState>, meeting_id: String) -> Result<Vec<MeetingNote>, String> {
    MeetingNotesRepository::get(state.db_manager.pool(), &meeting_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_meeting_notes(state: tauri::State<'_, AppState>, meeting_id: String, notes: Vec<MeetingNote>) -> Result<Vec<MeetingNote>, String> {
    save_notes_inner(state.db_manager.pool(), &meeting_id, notes).await
}

#[tauri::command]
pub async fn set_keep_audio(state: tauri::State<'_, AppState>, meeting_id: String, keep: bool) -> Result<(), String> {
    match MeetingsRepository::set_keep_audio(state.db_manager.pool(), &meeting_id, keep).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!("Meeting not found: {meeting_id}")),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;

    async fn meeting(pool: &SqlitePool) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z')")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('t1', 'm1', 'hi', 'x')")
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn set_mark_validates_and_reports_missing_line() {
        let pool = test_pool().await;
        meeting(&pool).await;
        assert!(set_mark_inner(&pool, "t1", Some("bogus".into())).await.is_err());
        assert!(set_mark_inner(&pool, "missing", Some("action".into())).await.is_err());
        set_mark_inner(&pool, "t1", Some("action".into())).await.unwrap();
        set_mark_inner(&pool, "t1", None).await.unwrap();
    }

    #[tokio::test]
    async fn save_notes_rejects_bad_notes_and_unknown_meeting() {
        let pool = test_pool().await;
        meeting(&pool).await;
        let bad = vec![MeetingNote { id: "n".into(), at: None, text: " ".into() }];
        assert!(save_notes_inner(&pool, "m1", bad).await.is_err());
        let good = vec![MeetingNote { id: "n".into(), at: None, text: " later ".into() }];
        assert!(save_notes_inner(&pool, "nope", good.clone()).await.is_err());
        let stored = save_notes_inner(&pool, "m1", good).await.unwrap();
        assert_eq!(stored[0].text, "later");
    }
}
