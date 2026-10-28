//! Eligibility and recording-scoped state for continuing an earlier meeting.
use super::meeting_folder::{guarded_meeting_folder, is_plain_file};
use super::recording_commands::{is_recording_now, recordings_started};
use super::recording_preferences::get_default_recordings_folder;
use crate::database::repositories::meeting::MeetingsRepository;
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const CONTINUE_WINDOW: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContinueMeeting {
    pub meeting_id: String,
    pub title: String,
}

#[derive(Serialize)]
pub struct ContinueStatus {
    pub offer: Option<ContinueMeeting>,
    pub target: Option<ContinueMeeting>,
}

pub(crate) struct Eligible {
    pub folder: PathBuf,
    pub title: String,
}

static LAST: Mutex<Option<(ContinueMeeting, Instant)>> = Mutex::new(None);
static TARGET: Mutex<Option<(u64, ContinueMeeting)>> = Mutex::new(None);

fn imported(folder: &Path) -> bool {
    std::fs::read(folder.join("metadata.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|metadata| {
            metadata.get("source").and_then(|source| source.as_str()) == Some("import")
        })
}

/// Called only for folders validated by `guarded_meeting_folder`.
pub(crate) fn resumable_folder(folder: &Path) -> bool {
    is_plain_file(&folder.join("audio.mp4")) && !imported(folder)
}

pub(crate) async fn eligible(
    pool: &SqlitePool,
    meeting_id: &str,
    base: &Path,
) -> Result<Eligible, String> {
    if meeting_id.trim().is_empty() {
        return Err("The earlier meeting no longer exists.".into());
    }
    let meeting = MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .map_err(|e| format!("Could not read the earlier meeting: {e}."))?
        .ok_or("The earlier meeting no longer exists.")?;
    let folder = meeting
        .folder_path
        .as_deref()
        .and_then(|path| guarded_meeting_folder(path, base))
        .ok_or("The earlier meeting has no recording folder.")?;
    if meeting.audio_deleted_at.is_some() {
        return Err("The earlier meeting's audio was deleted.".into());
    }
    if imported(&folder) {
        return Err("Imported meetings cannot be continued.".into());
    }
    if !is_plain_file(&folder.join("audio.mp4")) {
        return Err("The earlier meeting's audio.mp4 is missing.".into());
    }
    Ok(Eligible {
        folder,
        title: meeting.title,
    })
}

pub(crate) fn remember_saved(m: ContinueMeeting, at: Instant) {
    *LAST.lock().unwrap_or_else(PoisonError::into_inner) = Some((m, at));
}

pub(crate) fn offer_at(now: Instant) -> Option<ContinueMeeting> {
    LAST.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|(_, at)| now.saturating_duration_since(*at) < CONTINUE_WINDOW)
        .map(|(m, _)| m.clone())
}

pub(crate) fn offer() -> Option<ContinueMeeting> {
    offer_at(Instant::now())
}

pub(crate) fn set_target_for(recording: u64, m: ContinueMeeting) {
    *TARGET.lock().unwrap_or_else(PoisonError::into_inner) = Some((recording, m));
}

pub(crate) fn current_target(recording: u64) -> Option<ContinueMeeting> {
    TARGET
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .filter(|(number, _)| *number == recording)
        .map(|(_, m)| m.clone())
}

pub(crate) fn take_target(recording: u64) -> Option<ContinueMeeting> {
    TARGET
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .filter(|(number, _)| *number == recording)
        .map(|(_, m)| m)
}

pub(crate) async fn set_target<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    meeting_id: &str,
) -> Result<ContinueMeeting, String> {
    if !is_recording_now() {
        return Err("Start a recording first.".into());
    }
    let recording = recordings_started();
    let earlier = eligible(pool, meeting_id, &get_default_recordings_folder()).await?;
    // Eligibility awaits the database: never attach a request to a replacement recording.
    if !is_recording_now() || recordings_started() != recording {
        return Err("Start a recording first.".into());
    }
    let m = ContinueMeeting {
        meeting_id: meeting_id.to_owned(),
        title: earlier.title,
    };
    set_target_for(recording, m.clone());
    app.emit("continue-target-changed", &m)
        .map_err(|e| format!("Could not announce the continuation target: {e}."))?;
    Ok(m)
}

#[tauri::command]
pub async fn continue_status() -> ContinueStatus {
    ContinueStatus {
        offer: offer(),
        target: if is_recording_now() {
            current_target(recordings_started())
        } else {
            None
        },
    }
}

#[tauri::command]
pub async fn continue_set_target<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
) -> Result<ContinueMeeting, String> {
    let state = app
        .try_state::<crate::state::AppState>()
        .ok_or("Database not ready")?;
    set_target(&app, state.db_manager.pool(), &meeting_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use std::fs;
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};

    fn state_lock() -> MutexGuard<'static, ()> {
        static TURNS: Mutex<()> = Mutex::new(());
        TURNS.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn meeting() -> ContinueMeeting {
        ContinueMeeting {
            meeting_id: "recorded".into(),
            title: "Earlier meeting".into(),
        }
    }

    async fn insert(
        pool: &sqlx::SqlitePool,
        id: &str,
        folder: Option<&std::path::Path>,
        deleted: bool,
    ) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path, audio_deleted_at) VALUES (?, ?, '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z', ?, ?)")
            .bind(id).bind("Earlier meeting")
            .bind(folder.map(|p| p.to_string_lossy().to_string()))
            .bind(deleted.then_some("2026-10-16T00:00:00Z"))
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn eligibility_rejects_imported_deleted_and_folderless() {
        let pool = test_pool().await;
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        for name in ["imported", "deleted", "missing", "recorded"] {
            let folder = base.path().join(name);
            fs::create_dir(&folder).unwrap();
            if name != "missing" {
                fs::write(folder.join("audio.mp4"), "audio").unwrap();
            }
            if name == "imported" {
                fs::write(folder.join("metadata.json"), r#"{"source":"import"}"#).unwrap();
            }
            insert(&pool, name, Some(&folder), name == "deleted").await;
        }
        insert(&pool, "folderless", None, false).await;
        fs::write(outside.path().join("audio.mp4"), "audio").unwrap();
        insert(&pool, "outside", Some(outside.path()), false).await;
        for (id, error) in [
            ("nonexistent", "The earlier meeting no longer exists."),
            (" ", "The earlier meeting no longer exists."),
            (
                "recorded' OR 1=1 --",
                "The earlier meeting no longer exists.",
            ),
            ("folderless", "The earlier meeting has no recording folder."),
            ("outside", "The earlier meeting has no recording folder."),
            ("deleted", "The earlier meeting's audio was deleted."),
            ("imported", "Imported meetings cannot be continued."),
            ("missing", "The earlier meeting's audio.mp4 is missing."),
        ] {
            assert_eq!(
                eligible(&pool, id, base.path()).await.err().as_deref(),
                Some(error)
            );
        }
        let result = eligible(&pool, "recorded", base.path()).await.unwrap();
        assert_eq!(result.title, "Earlier meeting");
        assert_eq!(
            result.folder,
            dunce::canonicalize(base.path().join("recorded")).unwrap()
        );
    }

    #[test]
    fn target_is_taken_only_by_its_recording() {
        let _turn = state_lock();
        let m = meeting();
        set_target_for(5, m.clone());
        assert_eq!(current_target(5), Some(m.clone()));
        assert_eq!(current_target(6), None);
        assert_eq!(take_target(6), None);
        assert_eq!(take_target(5), None);
        set_target_for(5, m.clone());
        assert_eq!(take_target(5), Some(m));
        assert_eq!(current_target(5), None);
    }

    #[test]
    fn offer_expires_after_the_window() {
        let _turn = state_lock();
        let t0 = Instant::now();
        let m = meeting();
        remember_saved(m.clone(), t0);
        assert_eq!(offer_at(t0 + Duration::from_secs(119)), Some(m));
        assert_eq!(offer_at(t0 + Duration::from_secs(120)), None);
    }

    #[test]
    fn resumable_folder_requires_plain_mp4_and_non_import_metadata() {
        let base = tempfile::tempdir().unwrap();
        let folder = base.path();
        assert!(!resumable_folder(folder));
        fs::write(folder.join("audio.mp4"), "audio").unwrap();
        assert!(resumable_folder(folder));
        for metadata in ["invalid json", "{}", r#"{"source":"recording"}"#] {
            fs::write(folder.join("metadata.json"), metadata).unwrap();
            assert!(resumable_folder(folder));
        }
        fs::write(folder.join("metadata.json"), r#"{"source":"import"}"#).unwrap();
        assert!(!resumable_folder(folder));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn eligibility_rejects_linked_audio_and_folder() {
        let pool = test_pool().await;
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("audio.mp4"), "audio").unwrap();
        let folder = base.path().join("recorded");
        fs::create_dir(&folder).unwrap();
        std::os::unix::fs::symlink(outside.path().join("audio.mp4"), folder.join("audio.mp4"))
            .unwrap();
        insert(&pool, "linked-audio", Some(&folder), false).await;
        assert_eq!(
            eligible(&pool, "linked-audio", base.path())
                .await
                .err()
                .as_deref(),
            Some("The earlier meeting's audio.mp4 is missing.")
        );
        assert!(!resumable_folder(&folder));
        let linked = base.path().join("linked");
        std::os::unix::fs::symlink(outside.path(), &linked).unwrap();
        insert(&pool, "linked-folder", Some(&linked), false).await;
        assert_eq!(
            eligible(&pool, "linked-folder", base.path())
                .await
                .err()
                .as_deref(),
            Some("The earlier meeting has no recording folder.")
        );
    }
}
