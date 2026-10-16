//! Old-audio review (spec §8.1): lists recordings older than N days and moves exactly the
//! confirmed ones to the Recycle Bin. Never runs by itself; never touches transcripts,
//! summaries, notes or the JSON files in the folder.
use crate::audio::meeting_folder::{audio_files, checkpoint_files, ensure_still_guarded, guarded_meeting_folder, recycle, remove_checked, remove_if_empty, Remover};
use crate::audio::recording_preferences::{get_default_recordings_folder, load_recording_preferences};
use crate::database::models::DateTimeUtc;
use chrono::{DateTime, Duration, Utc};
use log::{info, warn};
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

/// Checked before each meeting is recycled; `&is_recording_now` in the commands.
pub type RecordingCheck<'a> = &'a (dyn Fn() -> bool + Sync);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OldAudio {
    pub meeting_id: String,
    pub title: String,
    pub created_at: String,
    pub folder: String,
    pub bytes: u64,
    pub files: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct DeleteResult {
    pub meetings: u32,
    pub bytes: u64,
    pub failed: Vec<String>,
}

/// A checked subtraction: a user-set day count too large for a date is "never old", not a panic.
pub fn is_old(created_at: DateTime<Utc>, days: u32, now: DateTime<Utc>) -> bool {
    now.checked_sub_signed(Duration::days(i64::from(days))).is_some_and(|cutoff| created_at < cutoff)
}

/// Everything the review may delete in one folder, checkpoint files first and audio last.
pub fn audio_paths(folder: &Path) -> Vec<PathBuf> {
    [checkpoint_files(&folder.join(".checkpoints")), audio_files(folder)].concat()
}

fn size_of(path: &Path) -> u64 {
    fs::symlink_metadata(path).map(|m| m.len()).unwrap_or(0)
}

#[derive(sqlx::FromRow)]
struct MeetingRow {
    id: String,
    title: String,
    created_at: DateTimeUtc,
    folder_path: String,
    keep_audio: bool,
}

/// Every meeting that has a folder. Read once per call; a read failure is an error, never a partial list.
async fn load_meetings(pool: &SqlitePool) -> Result<Vec<MeetingRow>, String> {
    sqlx::query_as("SELECT id, title, created_at, folder_path, keep_audio FROM meetings WHERE folder_path IS NOT NULL ORDER BY created_at")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
}

/// How many meetings point at each canonical folder. A folder with more than one is never offered.
fn meetings_per_folder(meetings: &[MeetingRow]) -> HashMap<PathBuf, usize> {
    let mut counts = HashMap::new();
    for m in meetings {
        if let Ok(folder) = dunce::canonicalize(&m.folder_path) {
            *counts.entry(folder).or_default() += 1;
        }
    }
    counts
}

/// The offers for `meetings` (only `only`, if given): not pinned, old enough, a guarded folder
/// that no other meeting uses, and audio still there.
fn offers(meetings: &[MeetingRow], days: u32, now: DateTime<Utc>, base: &Path, only: Option<&str>) -> Vec<OldAudio> {
    let counts = meetings_per_folder(meetings);
    meetings
        .iter()
        .filter(|m| only.is_none_or(|id| m.id == id))
        .filter(|m| !m.keep_audio && is_old(m.created_at.0, days, now))
        .filter_map(|m| {
            let folder = guarded_meeting_folder(&m.folder_path, base)?;
            if counts.get(&folder).copied().unwrap_or(0) > 1 {
                return None;
            }
            if audio_files(&folder).is_empty() {
                return None;
            }
            let files = audio_paths(&folder);
            Some(OldAudio {
                meeting_id: m.id.clone(),
                title: m.title.clone(),
                created_at: m.created_at.0.to_rfc3339(),
                folder: folder.to_string_lossy().to_string(),
                bytes: files.iter().map(|f| size_of(f)).sum(),
                files: files.iter().map(|f| f.to_string_lossy().to_string()).collect(),
            })
        })
        .collect()
}

pub async fn list_old_audio(
    pool: &SqlitePool,
    days: Option<u32>,
    now: DateTime<Utc>,
    recording: bool,
    base: &Path,
) -> Result<Vec<OldAudio>, String> {
    let Some(days) = days.filter(|d| *d > 0) else { return Ok(Vec::new()) };
    if recording {
        return Ok(Vec::new());
    }
    Ok(offers(&load_meetings(pool).await?, days, now, base, None))
}

/// Moves exactly the confirmed meetings' audio to the Recycle Bin. Before each meeting it checks
/// recording and the meeting's eligibility again; before each file it re-checks the file.
/// Confirmed ids that are not (or no longer) listed are reported in `failed`, never silently skipped.
pub async fn delete_old_audio_inner(
    pool: &SqlitePool,
    confirmed: &[String],
    days: Option<u32>,
    now: DateTime<Utc>,
    is_recording: RecordingCheck<'_>,
    base: &Path,
    remove: Remover<'_>,
) -> Result<DeleteResult, String> {
    if is_recording() {
        return Err("Stop the recording first.".into());
    }
    let days = days.filter(|d| *d > 0);
    let mut result = DeleteResult::default();
    let mut seen = HashSet::new();
    for id in confirmed.iter().filter(|id| seen.insert(id.as_str())) {
        // A Windows Bin prompt can keep this waiting for minutes, so the state is re-read per meeting.
        if is_recording() {
            result.failed.push("Stopped: a recording started. The remaining meetings were not deleted.".into());
            break;
        }
        let meetings = match load_meetings(pool).await {
            Ok(meetings) => meetings,
            Err(e) => {
                result.failed.push(format!("Stopped: could not re-check the list: {e}"));
                break;
            }
        };
        let Some(item) = days.and_then(|d| offers(&meetings, d, now, base, Some(id)).pop()) else {
            result.failed.push(format!("{id}: not listed for deletion (pinned, too recent, shared or no audio left)"));
            continue;
        };
        // Re-guard this folder now: earlier recycles may have waited minutes on a Windows prompt.
        let Some(folder) = guarded_meeting_folder(&item.folder, base) else {
            result.failed.push(format!("{}: the folder is no longer inside the recordings folder", item.title));
            continue;
        };
        let mut error = None;
        for f in audio_paths(&folder) {
            let size = size_of(&f);
            match remove_checked(&f, &folder, remove) {
                Ok(()) => result.bytes += size,
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }
        // The file loop may have waited on the Bin: the folder can have been swapped for a link since the check above.
        if let Err(e) = ensure_still_guarded(&folder, base, "Recording folder") {
            result.failed.push(format!("{}: {e}", item.title));
            continue;
        }
        if let Err(e) = remove_if_empty(&folder.join(".checkpoints"), remove) {
            warn!("Old audio: could not recycle the empty .checkpoints of {}: {e}", item.meeting_id);
            result.failed.push(format!("{}: could not move its empty .checkpoints folder: {e}", item.title));
        }
        if audio_files(&folder).is_empty() {
            match sqlx::query("UPDATE meetings SET audio_deleted_at = ? WHERE id = ?")
                .bind(now.to_rfc3339())
                .bind(&item.meeting_id)
                .execute(pool)
                .await
            {
                Ok(_) => result.meetings += 1,
                Err(e) => {
                    warn!("Old audio: could not record the deletion for {}: {e}", item.meeting_id);
                    result.failed.push(format!("{}: moved to the Recycle Bin, but the deletion was not recorded: {e}", item.title));
                }
            }
        }
        if let Some(e) = error {
            warn!("Old audio: {} not fully moved: {e}", item.meeting_id);
            result.failed.push(format!("{}: {e}", item.title));
        }
    }
    info!("Old audio: moved {} recordings ({} MB) to the Recycle Bin", result.meetings, result.bytes / 1_000_000);
    Ok(result)
}

#[tauri::command]
pub async fn old_audio_list<R: Runtime>(app: AppHandle<R>) -> Result<Vec<OldAudio>, String> {
    let state = app.try_state::<crate::state::AppState>().ok_or("Database not ready")?;
    let prefs = load_recording_preferences(&app).await.unwrap_or_default();
    let recording = crate::audio::recording_commands::is_recording_now();
    list_old_audio(state.db_manager.pool(), prefs.old_audio_days, Utc::now(), recording, &get_default_recordings_folder()).await
}

#[tauri::command]
pub async fn delete_old_audio<R: Runtime>(app: AppHandle<R>, meeting_ids: Vec<String>) -> Result<DeleteResult, String> {
    let state = app.try_state::<crate::state::AppState>().ok_or("Database not ready")?;
    let prefs = load_recording_preferences(&app).await.unwrap_or_default();
    delete_old_audio_inner(
        state.db_manager.pool(), &meeting_ids, prefs.old_audio_days, Utc::now(),
        &crate::audio::recording_commands::is_recording_now, &get_default_recordings_folder(), &recycle,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use chrono::{Duration, TimeZone, Utc};
    use std::fs;
    use std::sync::Mutex;

    fn now() -> DateTime<Utc> { Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap() }

    /// Recycle Bin stand-in: files, and directories only when empty.
    struct FakeBin(Mutex<Vec<PathBuf>>);
    impl FakeBin {
        fn new() -> Self { FakeBin(Mutex::new(Vec::new())) }
        fn remove(&self, paths: &[PathBuf]) -> Result<(), String> {
            for p in paths {
                if p.is_dir() { fs::remove_dir(p).map_err(|e| e.to_string())? } else { fs::remove_file(p).map_err(|e| e.to_string())? }
                self.0.lock().unwrap().push(p.clone());
            }
            Ok(())
        }
    }

    fn folder(base: &Path, name: &str) -> PathBuf {
        let f = base.join(name);
        fs::create_dir_all(f.join(".checkpoints")).unwrap();
        fs::write(f.join("audio.mp4"), vec![0u8; 1000]).unwrap();
        fs::write(f.join(".checkpoints/audio_chunk_000.mp4"), vec![0u8; 10]).unwrap();
        fs::write(f.join("transcripts.json"), "[]").unwrap();
        fs::write(f.join("metadata.json"), "{}").unwrap();
        f
    }

    async fn insert(pool: &SqlitePool, id: &str, created: DateTime<Utc>, folder: Option<&Path>, keep: bool) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path, keep_audio) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(id).bind(format!("Meeting {id}")).bind(created).bind(created)
            .bind(folder.map(|f| f.to_string_lossy().to_string())).bind(keep)
            .execute(pool).await.unwrap();
    }

    async fn deleted_at(pool: &SqlitePool, id: &str) -> Option<String> {
        let (d,): (Option<String>,) = sqlx::query_as("SELECT audio_deleted_at FROM meetings WHERE id = ?").bind(id).fetch_one(pool).await.unwrap();
        d
    }

    #[test]
    fn is_old_uses_the_cutoff() {
        assert!(is_old(now() - Duration::days(31), 30, now()));
        assert!(!is_old(now() - Duration::days(29), 30, now()));
    }

    #[test]
    fn a_huge_day_count_is_never_old_and_does_not_panic() {
        assert!(!is_old(now(), u32::MAX, now()));
        assert!(!is_old(now() - Duration::days(40), u32::MAX, now()));
    }

    #[tokio::test]
    async fn list_selects_the_right_meetings_and_deletes_nothing() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let old = now() - Duration::days(40);
        let f_old = folder(b.path(), "old");
        let f_new = folder(b.path(), "new");
        let f_pin = folder(b.path(), "pinned");
        let f_shared = folder(b.path(), "shared");
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("audio.mp4"), "x").unwrap();
        insert(&pool, "old", old, Some(&f_old), false).await;
        insert(&pool, "new", now() - Duration::days(5), Some(&f_new), false).await;
        insert(&pool, "pinned", old, Some(&f_pin), true).await;
        insert(&pool, "nofolder", old, None, false).await;
        insert(&pool, "outside", old, Some(outside.path()), false).await;
        insert(&pool, "shared1", old, Some(&f_shared), false).await;
        insert(&pool, "shared2", now() - Duration::days(1), Some(&f_shared), false).await;

        let list = list_old_audio(&pool, Some(30), now(), false, b.path()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].meeting_id, "old");
        assert_eq!(list[0].title, "Meeting old");
        assert_eq!(list[0].bytes, 1010);
        assert_eq!(list[0].files.len(), 2);
        assert!(list[0].files.last().unwrap().ends_with("audio.mp4")); // audio last
        assert!(f_old.join("audio.mp4").exists());
        assert!(list_old_audio(&pool, None, now(), false, b.path()).await.unwrap().is_empty());
        assert!(list_old_audio(&pool, Some(30), now(), true, b.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_removes_only_confirmed_audio_and_marks_the_meeting() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let old = now() - Duration::days(40);
        let f1 = folder(b.path(), "one");
        let f2 = folder(b.path(), "two");
        insert(&pool, "one", old, Some(&f1), false).await;
        insert(&pool, "two", old, Some(&f2), false).await;
        let bin = FakeBin::new();

        let r = delete_old_audio_inner(&pool, &["one".into(), "ghost".into()], Some(30), now(), &|| false, b.path(), &|p| bin.remove(p)).await.unwrap();
        assert_eq!((r.meetings, r.bytes), (1, 1010));
        assert_eq!(r.failed.len(), 1); // the unlisted "ghost" id is reported, not skipped
        assert!(r.failed[0].starts_with("ghost"));
        assert!(!f1.join("audio.mp4").exists());
        assert!(!f1.join(".checkpoints").exists()); // emptied, then recycled
        for keep in ["transcripts.json", "metadata.json"] { assert!(f1.join(keep).exists(), "{keep}"); }
        assert!(f2.join("audio.mp4").exists()); // not confirmed
        assert!(deleted_at(&pool, "one").await.is_some());
        assert!(deleted_at(&pool, "two").await.is_none());
        assert_eq!(bin.0.lock().unwrap().len(), 3); // chunk, audio, then the empty .checkpoints
    }

    #[tokio::test]
    async fn delete_is_refused_while_recording_and_reports_bin_failures() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let f = folder(b.path(), "one");
        insert(&pool, "one", now() - Duration::days(40), Some(&f), false).await;
        let bin = FakeBin::new();
        // the user declines Windows' prompt for the big audio file; the chunk was already moved
        let declines_audio = |p: &[PathBuf]| {
            if p.iter().any(|x| x.file_name().is_some_and(|n| n == "audio.mp4")) { Err("user declined".to_string()) } else { bin.remove(p) }
        };
        assert!(delete_old_audio_inner(&pool, &["one".into()], Some(30), now(), &|| true, b.path(), &declines_audio).await.is_err());
        let r = delete_old_audio_inner(&pool, &["one".into()], Some(30), now(), &|| false, b.path(), &declines_audio).await.unwrap();
        assert_eq!(r.meetings, 0);
        assert_eq!(r.bytes, 10);
        assert_eq!(r.failed.len(), 1);
        assert!(f.join("audio.mp4").exists());
        assert!(deleted_at(&pool, "one").await.is_none()); // audio still there
    }

    #[tokio::test]
    async fn a_folder_another_meeting_spells_differently_is_never_offered_or_deleted() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let f = folder(b.path(), "shared");
        insert(&pool, "a", now() - Duration::days(40), Some(&f), false).await;
        let spelled = format!("{}{}", f.display(), std::path::MAIN_SEPARATOR); // same folder, trailing slash
        insert(&pool, "b", now() - Duration::days(1), Some(Path::new(&spelled)), false).await;
        assert!(list_old_audio(&pool, Some(30), now(), false, b.path()).await.unwrap().is_empty());
        let bin = FakeBin::new();
        let r = delete_old_audio_inner(&pool, &["a".into()], Some(30), now(), &|| false, b.path(), &|p| bin.remove(p)).await.unwrap();
        assert_eq!(r.meetings, 0);
        assert!(f.join("audio.mp4").exists());
        assert!(bin.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_huge_cutoff_offers_nothing_and_deletes_nothing_without_panicking() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let f = folder(b.path(), "one");
        insert(&pool, "one", now() - Duration::days(40), Some(&f), false).await;
        assert!(list_old_audio(&pool, Some(u32::MAX), now(), false, b.path()).await.unwrap().is_empty());
        let bin = FakeBin::new();
        let r = delete_old_audio_inner(&pool, &["one".into()], Some(u32::MAX), now(), &|| false, b.path(), &|p| bin.remove(p)).await.unwrap();
        assert_eq!(r.meetings, 0);
        assert!(f.join("audio.mp4").exists());
    }

    #[tokio::test]
    async fn a_recording_that_starts_mid_delete_stops_before_the_next_meeting() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let old = now() - Duration::days(40);
        let (f1, f2) = (folder(b.path(), "one"), folder(b.path(), "two"));
        insert(&pool, "one", old, Some(&f1), false).await;
        insert(&pool, "two", old, Some(&f2), false).await;
        let bin = FakeBin::new();
        let recording = AtomicBool::new(false);
        let remove = |p: &[PathBuf]| {
            let r = bin.remove(p);
            if p.iter().any(|x| x.ends_with("audio.mp4")) {
                recording.store(true, Ordering::SeqCst); // the user starts a recording during the Bin prompt
            }
            r
        };
        let r = delete_old_audio_inner(&pool, &["one".into(), "two".into()], Some(30), now(), &|| recording.load(Ordering::SeqCst), b.path(), &remove).await.unwrap();
        assert_eq!(r.meetings, 1);
        assert!(!f1.join("audio.mp4").exists());
        assert!(f2.join("audio.mp4").exists());
        assert!(r.failed.iter().any(|m| m.starts_with("Stopped: a recording started")));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_meeting_pinned_mid_delete_is_not_deleted() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let old = now() - Duration::days(40);
        let (f1, f2) = (folder(b.path(), "one"), folder(b.path(), "two"));
        insert(&pool, "one", old, Some(&f1), false).await;
        insert(&pool, "two", old, Some(&f2), false).await;
        let bin = FakeBin::new();
        let remove = |p: &[PathBuf]| {
            let r = bin.remove(p);
            if p.iter().any(|x| x.ends_with("audio.mp4")) {
                // the user pins "two" in the UI while the first Bin call is still open
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(sqlx::query("UPDATE meetings SET keep_audio = 1 WHERE id = 'two'").execute(&pool))
                })
                .unwrap();
            }
            r
        };
        let r = delete_old_audio_inner(&pool, &["one".into(), "two".into()], Some(30), now(), &|| false, b.path(), &remove).await.unwrap();
        assert_eq!(r.meetings, 1);
        assert!(f2.join("audio.mp4").exists());
        assert!(r.failed.iter().any(|m| m.starts_with("two:")));
    }

    #[tokio::test]
    async fn a_checkpoints_folder_that_cannot_be_recycled_is_reported_and_the_meeting_is_still_marked() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let f = folder(b.path(), "one");
        insert(&pool, "one", now() - Duration::days(40), Some(&f), false).await;
        let bin = FakeBin::new();
        let remove = |p: &[PathBuf]| {
            if p.iter().any(|x| x.file_name().is_some_and(|n| n == ".checkpoints")) { Err("declined".to_string()) } else { bin.remove(p) }
        };
        let r = delete_old_audio_inner(&pool, &["one".into()], Some(30), now(), &|| false, b.path(), &remove).await.unwrap();
        assert_eq!(r.meetings, 1);
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].contains(".checkpoints"));
        assert!(deleted_at(&pool, "one").await.is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_folder_swapped_for_a_link_mid_delete_keeps_its_checkpoints_outside() {
        use std::os::unix::fs::symlink;
        use std::sync::atomic::{AtomicBool, Ordering};
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let f = folder(b.path(), "one");
        insert(&pool, "one", now() - Duration::days(40), Some(&f), false).await;
        let moved = outside.path().join("moved");
        let bin = FakeBin::new();
        let swapped = AtomicBool::new(false);
        let remove = |p: &[PathBuf]| {
            let r = bin.remove(p);
            if !swapped.swap(true, Ordering::SeqCst) {
                // The folder leaves the base after the first file; a link takes its place.
                fs::rename(&f, &moved).unwrap();
                symlink(&moved, &f).unwrap();
            }
            r
        };
        let r = delete_old_audio_inner(&pool, &["one".into()], Some(30), now(), &|| false, b.path(), &remove).await.unwrap();
        assert_eq!(r.meetings, 0);
        assert!(r.failed.iter().any(|m| m.starts_with("Meeting one:") && m.contains("Recording folder kept")), "{:?}", r.failed);
        assert!(moved.join(".checkpoints").is_dir(), "a folder outside the base must keep its .checkpoints");
    }

    #[tokio::test]
    async fn a_failed_deletion_record_does_not_count_the_meeting() {
        let pool = test_pool().await;
        sqlx::query("CREATE TRIGGER refuse_audio_marks BEFORE UPDATE ON meetings BEGIN SELECT RAISE(ABORT, 'refused'); END")
            .execute(&pool).await.unwrap();
        let b = tempfile::tempdir().unwrap();
        let f = folder(b.path(), "one");
        insert(&pool, "one", now() - Duration::days(40), Some(&f), false).await;
        let bin = FakeBin::new();
        let r = delete_old_audio_inner(&pool, &["one".into()], Some(30), now(), &|| false, b.path(), &|p| bin.remove(p)).await.unwrap();
        assert_eq!(r.meetings, 0);
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].contains("not recorded"));
        assert!(!f.join("audio.mp4").exists()); // the audio did move to the Recycle Bin
    }

    #[test]
    fn foreign_files_in_checkpoints_are_neither_listed_nor_deleted() {
        let b = tempfile::tempdir().unwrap();
        let f = b.path().join("m");
        fs::create_dir_all(f.join(".checkpoints")).unwrap();
        fs::write(f.join(".checkpoints/mine.txt"), vec![0u8; 500]).unwrap();
        assert!(audio_paths(&f).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn linked_checkpoints_are_not_listed() {
        let b = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join("audio_chunk_000.mp4"), "x").unwrap();
        let f = b.path().join("m");
        fs::create_dir(&f).unwrap();
        std::os::unix::fs::symlink(target.path(), f.join(".checkpoints")).unwrap();
        assert!(audio_paths(&f).is_empty());
    }
}
