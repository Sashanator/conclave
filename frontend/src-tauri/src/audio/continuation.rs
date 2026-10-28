//! Eligibility and recording-scoped state for continuing an earlier meeting.
use super::meeting_folder::{
    guarded_meeting_folder, is_plain_file, recycle, remove_checked, Remover,
};
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
static ACTIVE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

struct Claim(PathBuf);

impl Drop for Claim {
    fn drop(&mut self) {
        ACTIVE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|folder| folder != &self.0);
    }
}

fn claim(folder: &Path) -> Option<Claim> {
    let mut active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    if active.iter().any(|path| path == folder) {
        return None;
    }
    let folder = folder.to_path_buf();
    active.push(folder.clone());
    Some(Claim(folder))
}

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

/// Only called with a canonical folder validated by `guarded_meeting_folder`.
/// Keep ambiguous states intact so recovery never discards the only original audio.
pub(crate) fn repair_interrupted_swap(folder: &Path, remove: Remover<'_>) {
    let Some(_claim) = claim(folder) else {
        log::warn!("Interrupted continuation swap left for the running continuation.");
        return;
    };
    let audio = folder.join("audio.mp4");
    let prev = folder.join(".audio.prev.mp4");
    match prev.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) if is_plain_file(&prev) => {
            let repaired = match audio.symlink_metadata() {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::rename(&prev, &audio).is_ok()
                }
                Ok(meta)
                    if is_plain_file(&audio)
                        && prev.symlink_metadata().is_ok_and(|p| meta.len() >= p.len()) =>
                {
                    remove_checked(&prev, folder, remove).is_ok()
                }
                _ => false,
            };
            if !repaired {
                log::warn!("Interrupted continuation swap needs recovery.");
                return;
            }
        }
        _ => {
            log::warn!("Interrupted continuation swap needs recovery.");
            return;
        }
    }
    let tmp = folder.join(".audio.merge.tmp.mp4");
    if is_plain_file(&audio) && is_plain_file(&tmp) {
        remove_merge_tmp(&tmp);
    }
}

pub(crate) async fn eligible(
    pool: &SqlitePool,
    meeting_id: &str,
    base: &Path,
) -> Result<Eligible, String> {
    eligible_with_remover(pool, meeting_id, base, &recycle).await
}

async fn eligible_with_remover(
    pool: &SqlitePool,
    meeting_id: &str,
    base: &Path,
    remove: Remover<'_>,
) -> Result<Eligible, String> {
    if meeting_id.trim().is_empty() {
        return Err("The earlier meeting no longer exists.".into());
    }
    let meeting = MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .map_err(|_| "Could not read the earlier meeting.".to_string())?
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
    repair_interrupted_swap(&folder, remove);
    if !is_plain_file(&folder.join("audio.mp4")) {
        return Err("The earlier meeting's audio.mp4 is missing.".into());
    }
    Ok(Eligible {
        folder,
        title: meeting.title,
    })
}

pub(crate) fn parse_ffmpeg_duration(stderr: &str) -> Option<f64> {
    let duration = stderr.split_once("Duration:")?.1.split(',').next()?.trim();
    let mut fields = duration.split(':');
    let hours: f64 = fields.next()?.parse().ok()?;
    let minutes: f64 = fields.next()?.parse().ok()?;
    let seconds: f64 = fields.next()?.parse().ok()?;
    if fields.next().is_some() {
        return None;
    }
    let total = hours * 3600.0 + minutes * 60.0 + seconds;
    total.is_finite().then_some(total)
}

pub(crate) fn join_levels(
    old: Option<Vec<crate::speakers::levels::LevelPair>>,
    offset_secs: f64,
    new: Option<Vec<crate::speakers::levels::LevelPair>>,
) -> Option<Vec<crate::speakers::levels::LevelPair>> {
    if old.is_none() && new.is_none() {
        return None;
    }
    let mut joined = old.unwrap_or_default();
    joined.resize(
        (offset_secs / crate::speakers::levels::SLICE_SECS).round() as usize,
        (0.0, 0.0),
    );
    joined.extend(new.unwrap_or_default());
    Some(joined)
}

fn duration_matches(old: f64, new: Option<f64>, merged: Option<f64>) -> bool {
    let (Some(new), Some(merged)) = (new, merged) else {
        return false;
    };
    let total = old + new;
    [old, new, merged, total]
        .iter()
        .all(|d| d.is_finite() && *d > 0.0)
        && (merged - total).abs() <= 2.0 + 0.005 * total
}

fn join_parts(
    ffmpeg: &Path,
    old: &Path,
    new: &Path,
    tmp: &Path,
    verify: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<f64, String> {
    use super::incremental_saver::{concat_args, concat_line};
    // Concat may silently skip a corrupt part and produce a valid old-only file.
    // Check both inputs before any probing, concat-list writes, or output replacement.
    verify(old).map_err(|_| "The earlier meeting's audio is unreadable.")?;
    verify(new).map_err(|_| "The new recording's audio is unreadable.")?;
    let command = || {
        let mut cmd = std::process::Command::new(ffmpeg);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd
    };
    let probe = |path: &Path| -> Result<Option<f64>, String> {
        // A nonzero exit is expected when probing without an output.
        let output = command()
            .args(["-hide_banner", "-i"])
            .arg(path)
            .output()
            .map_err(|e| format!("Could not run the audio duration probe ({:?}).", e.kind()))?;
        Ok(parse_ffmpeg_duration(&String::from_utf8_lossy(
            &output.stderr,
        )))
    };
    let offset = probe(old)?.ok_or("Could not read the earlier audio's duration.")?;
    let new_duration = probe(new)?.ok_or("Merged audio does not match the two recordings.")?;
    let checkpoints = new
        .parent()
        .ok_or("Missing recording folder.")?
        .join(".checkpoints");
    if checkpoints.symlink_metadata().is_ok() && !super::meeting_folder::is_plain_dir(&checkpoints)
    {
        return Err("Invalid checkpoints folder.".into());
    }
    std::fs::create_dir_all(&checkpoints)
        .map_err(|e| format!("Could not create the checkpoints folder ({:?}).", e.kind()))?;
    let list = checkpoints.join("concat_list.txt");
    if list.symlink_metadata().is_ok() && !is_plain_file(&list) {
        return Err("Invalid concat list.".into());
    }
    std::fs::write(&list, concat_line(old) + &concat_line(new))
        .map_err(|e| format!("Could not write the concat list ({:?}).", e.kind()))?;
    let output = command()
        .args(concat_args(&list.to_string_lossy(), &tmp.to_string_lossy()))
        .output()
        .map_err(|e| format!("Could not run ffmpeg concat ({:?}).", e.kind()))?;
    if !output.status.success() {
        return Err(match output.status.code() {
            Some(code) => format!("ffmpeg concat failed (exit {code})"),
            None => "ffmpeg concat failed".into(),
        });
    }
    verify(tmp).map_err(|_| "The merged audio is unreadable.")?;
    if !duration_matches(offset, Some(new_duration), probe(tmp)?) {
        return Err("Merged audio does not match the two recordings.".into());
    }
    Ok(offset)
}

async fn ffmpeg_join(old: PathBuf, new: PathBuf, tmp: PathBuf) -> Result<f64, String> {
    tokio::task::spawn_blocking(move || {
        let ffmpeg = super::ffmpeg::find_ffmpeg_path().ok_or("ffmpeg was not found.")?;
        join_parts(&ffmpeg, &old, &new, &tmp, &|p| {
            super::incremental_saver::verify_chunk(&ffmpeg, p)
                .map_err(|_| "Audio verification failed.".into())
        })
    })
    .await
    .map_err(|_| "Audio merge worker failed.".to_string())?
}

fn remove_merge_tmp(tmp: &Path) {
    if let Err(e) = std::fs::remove_file(tmp) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!(
                "Could not remove audio merge temporary file ({:?}).",
                e.kind()
            );
        }
    }
}

fn read_levels(folder: &Path) -> Option<Vec<crate::speakers::levels::LevelPair>> {
    match std::fs::read(folder.join("levels.bin")) {
        Ok(bytes) => match crate::speakers::levels::decode(&bytes) {
            Ok(levels) => Some(levels),
            Err(e) => {
                log::warn!("Could not decode continuation levels: {e}");
                None
            }
        },
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("Could not read continuation levels ({:?}).", e.kind());
            }
            None
        }
    }
}

pub(crate) async fn continue_into_with<J, Fut>(
    pool: &SqlitePool,
    target_id: &str,
    new_folder: Option<&str>,
    transcripts: &[crate::api::TranscriptSegment],
    notes: &[crate::database::repositories::meeting_notes::MeetingNote],
    speakers_active: bool,
    base: &Path,
    remove: super::meeting_folder::Remover<'_>,
    join: J,
) -> Result<ContinueMeeting, String>
where
    J: FnOnce(PathBuf, PathBuf, PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<f64, String>>,
{
    use super::meeting_folder::{delete_meeting_folder, remove_checked};
    use crate::database::repositories::transcript::TranscriptsRepository;
    let target = eligible_with_remover(pool, target_id, base, remove).await?;
    let _claim = claim(&target.folder)
        .ok_or("Merging the audio failed: an earlier merge needs recovery.")?;
    let new = new_folder
        .and_then(|p| guarded_meeting_folder(p, base))
        .filter(|p| p != &target.folder && is_plain_file(&p.join("audio.mp4")))
        .ok_or("This recording has no audio file to add.")?;
    let old_audio = target.folder.join("audio.mp4");
    let new_audio = new.join("audio.mp4");
    let tmp = target.folder.join(".audio.merge.tmp.mp4");
    let prev = target.folder.join(".audio.prev.mp4");
    // The backup may hold the original; only a stale merge output is disposable.
    let absent = |path: &Path| {
        path.symlink_metadata()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    };
    if !absent(&prev) {
        return Err("Merging the audio failed: an earlier merge needs recovery.".into());
    }
    if !absent(&tmp) {
        if is_plain_file(&old_audio) {
            remove_merge_tmp(&tmp);
        }
        if !absent(&tmp) {
            return Err("Merging the audio failed: an earlier merge needs recovery.".into());
        }
    }
    let merge = async {
        let offset = join(old_audio.clone(), new_audio.clone(), tmp.clone()).await?;
        if !offset.is_finite() || offset <= 0.0 {
            return Err("Invalid earlier audio duration.".to_string());
        }
        if !is_plain_file(&tmp) {
            return Err("Merged audio is missing or is not a plain file.".into());
        }
        let len = |p: &Path| {
            std::fs::metadata(p)
                .map(|m| m.len())
                .map_err(|e| format!("Could not read audio size ({:?}).", e.kind()))
        };
        let inputs = len(&old_audio)?
            .checked_add(len(&new_audio)?)
            .ok_or("Audio size overflow.")?;
        if !super::incremental_saver::merged_is_complete(len(&tmp)?, inputs) {
            return Err("Merged audio is incomplete.".into());
        }
        Ok(offset)
    }
    .await;
    let offset = match merge {
        Ok(offset) => offset,
        Err(e) => {
            remove_merge_tmp(&tmp);
            return Err(format!("Merging the audio failed: {e}"));
        }
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            remove_merge_tmp(&tmp);
            return Err("Could not begin continuation save.".into());
        }
    };
    if TranscriptsRepository::append_in_tx(
        &mut tx,
        target_id,
        transcripts,
        offset,
        notes,
        speakers_active,
    )
    .await
    .is_err()
    {
        if tx.rollback().await.is_err() {
            log::error!("Continuation rollback failed.");
        }
        remove_merge_tmp(&tmp);
        return Err("Could not append the recording.".into());
    }
    let mut restore_failed = false;
    let swap = std::fs::rename(&old_audio, &prev).and_then(|()| {
        std::fs::rename(&tmp, &old_audio).inspect_err(|_| {
            if let Err(e) = std::fs::rename(&prev, &old_audio) {
                restore_failed = true;
                log::error!(
                    "Could not restore original audio; original kept as .audio.prev.mp4 ({:?}).",
                    e.kind()
                );
            }
        })
    });
    if let Err(e) = swap {
        if tx.rollback().await.is_err() {
            log::error!("Continuation rollback failed.");
        }
        remove_merge_tmp(&tmp);
        if restore_failed {
            return Err("The earlier meeting's audio could not be swapped; it will be restored the next time you open that meeting.".into());
        }
        return Err(format!(
            "Could not replace the meeting audio ({:?}).",
            e.kind()
        ));
    }
    if tx.commit().await.is_err() {
        // Windows rename cannot replace an existing file: move our merged output out of the way first.
        let moved_merged = std::fs::rename(&old_audio, &tmp);
        let merged_still_in_place = moved_merged.is_err();
        let restored = moved_merged.and_then(|()| std::fs::rename(&prev, &old_audio));
        let restore_failed = restored.is_err();
        if let Err(restore) = restored {
            log::error!(
                "Could not restore original audio; original kept as .audio.prev.mp4 ({:?}).",
                restore.kind()
            );
        }
        remove_merge_tmp(&tmp);
        if restore_failed {
            if merged_still_in_place {
                return Err("The earlier meeting's audio could not be put back after a failed save. It now also contains the new recording, which was saved as a separate meeting.".into());
            }
            return Err("The earlier meeting's audio could not be swapped; it will be restored the next time you open that meeting.".into());
        }
        return Err("Could not commit the continued recording.".into());
    }
    if let Some(levels) = join_levels(read_levels(&target.folder), offset, read_levels(&new)) {
        if let Err(e) = crate::speakers::levels::write_levels_file(&target.folder, &levels) {
            log::warn!(
                "Could not write continued recording levels ({:?}).",
                e.kind()
            );
        }
    }
    if remove_checked(&prev, &target.folder, remove).is_err() {
        log::warn!("Could not recycle previous meeting audio.");
    }
    if delete_meeting_folder(&new.to_string_lossy(), base, remove).is_err() {
        log::warn!("Could not recycle continued recording folder.");
    }
    Ok(ContinueMeeting {
        meeting_id: target_id.to_owned(),
        title: target.title,
    })
}

pub(crate) async fn continue_into(
    pool: &SqlitePool,
    target_id: &str,
    new_folder: Option<&str>,
    transcripts: &[crate::api::TranscriptSegment],
    notes: &[crate::database::repositories::meeting_notes::MeetingNote],
    speakers_active: bool,
    base: &Path,
) -> Result<ContinueMeeting, String> {
    continue_into_with(
        pool,
        target_id,
        new_folder,
        transcripts,
        notes,
        speakers_active,
        base,
        &super::meeting_folder::recycle,
        ffmpeg_join,
    )
    .await
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
    let mut target = TARGET.lock().unwrap_or_else(PoisonError::into_inner);
    if !is_recording_now() || recordings_started() != recording {
        return Err("Start a recording first.".into());
    }
    let m = ContinueMeeting {
        meeting_id: meeting_id.to_owned(),
        title: earlier.title,
    };
    *target = Some((recording, m.clone()));
    drop(target);
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

    use crate::api::TranscriptSegment;
    use crate::database::repositories::meeting::TitleOrigin;
    use crate::database::repositories::meeting_notes::{MeetingNote, MeetingNotesRepository};
    use crate::database::repositories::transcript::TranscriptsRepository;

    fn segment() -> TranscriptSegment {
        TranscriptSegment {
            id: "line".into(),
            text: "new line".into(),
            timestamp: "t".into(),
            audio_start_time: Some(1.0),
            audio_end_time: Some(2.0),
            duration: Some(1.0),
            mark: None,
            sentences: None,
            speaker_idx: None,
        }
    }

    fn note() -> MeetingNote {
        MeetingNote {
            id: "note".into(),
            at: Some(1.0),
            text: "a note".into(),
        }
    }

    struct FakeBin(Mutex<Vec<PathBuf>>);
    impl FakeBin {
        fn remove(&self, paths: &[PathBuf]) -> Result<(), String> {
            for p in paths {
                if p.is_dir() {
                    fs::remove_dir(p)
                } else {
                    fs::remove_file(p)
                }
                .map_err(|e| e.to_string())?;
                self.0.lock().unwrap().push(p.clone());
            }
            Ok(())
        }
    }

    async fn setup_merge() -> (SqlitePool, tempfile::TempDir, PathBuf, PathBuf, String) {
        let pool = test_pool().await;
        let base = tempfile::tempdir().unwrap();
        let old = base.path().join("old");
        let new = base.path().join("new");
        fs::create_dir(&old).unwrap();
        fs::create_dir(&new).unwrap();
        fs::write(old.join("audio.mp4"), vec![1; 100]).unwrap();
        fs::write(new.join("audio.mp4"), vec![2; 100]).unwrap();
        let id = TranscriptsRepository::save_transcript(
            &pool,
            "Earlier meeting",
            &[segment()],
            Some(old.to_string_lossy().into_owned()),
            &[note()],
            false,
            &TitleOrigin::default(),
        )
        .await
        .unwrap();
        (pool, base, old, new, id)
    }

    async fn fake_join(old: PathBuf, new: PathBuf, tmp: PathBuf) -> Result<f64, String> {
        let mut bytes = fs::read(old).unwrap();
        bytes.extend(fs::read(new).unwrap());
        fs::write(tmp, bytes).unwrap();
        Ok(10.0)
    }

    #[tokio::test]
    async fn interrupted_swap_restores_missing_audio_on_meeting_read() {
        let (pool, base, old, _, id) = setup_merge().await;
        fs::rename(old.join("audio.mp4"), old.join(".audio.prev.mp4")).unwrap();
        fs::write(old.join(".audio.merge.tmp.mp4"), "stale merge").unwrap();
        let audio = super::super::meeting_folder::meeting_audio_inner(&pool, &id, base.path())
            .await
            .unwrap();
        assert!(audio.path.is_some());
        assert!(audio.resumable);
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
        assert!(!old.join(".audio.prev.mp4").exists());
        assert!(!old.join(".audio.merge.tmp.mp4").exists());
    }

    #[tokio::test]
    async fn interrupted_swap_recycles_backup_and_allows_continue() {
        let (pool, base, old, new, id) = setup_merge().await;
        fs::write(old.join(".audio.prev.mp4"), vec![1; 100]).unwrap();
        fs::write(old.join("audio.mp4"), vec![1; 150]).unwrap();
        fs::write(old.join(".audio.merge.tmp.mp4"), "stale merge").unwrap();
        let bin = FakeBin(Mutex::new(vec![]));
        continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[],
            false,
            base.path(),
            &|p| bin.remove(p),
            fake_join,
        )
        .await
        .unwrap();
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap().len(), 250);
        assert_eq!(
            bin.0
                .lock()
                .unwrap()
                .iter()
                .filter(|p| p.ends_with(".audio.prev.mp4"))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn interrupted_swap_restores_before_eligibility_checks_audio() {
        let (pool, base, old, _, id) = setup_merge().await;
        fs::rename(old.join("audio.mp4"), old.join(".audio.prev.mp4")).unwrap();
        let result =
            eligible_with_remover(&pool, &id, base.path(), &|_| panic!("must not recycle")).await;
        assert!(result.is_ok());
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
    }

    #[test]
    fn interrupted_swap_waits_for_the_folder_claim() {
        for has_audio in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let folder = dunce::canonicalize(dir.path()).unwrap();
            let prev = folder.join(".audio.prev.mp4");
            let audio = folder.join("audio.mp4");
            let tmp = folder.join(".audio.merge.tmp.mp4");
            fs::write(&prev, vec![1; 100]).unwrap();
            if has_audio {
                fs::write(&audio, vec![2; 150]).unwrap();
            }
            fs::write(&tmp, "in-flight merge").unwrap();

            let held = claim(&folder).unwrap();
            assert!(claim(&folder).is_none());
            repair_interrupted_swap(&folder, &|_| panic!("must not recycle claimed audio"));
            assert_eq!(fs::read(&prev).unwrap(), vec![1; 100]);
            assert_eq!(fs::read(&tmp).unwrap(), b"in-flight merge");
            if has_audio {
                assert_eq!(fs::read(&audio).unwrap(), vec![2; 150]);
            } else {
                assert!(!audio.exists());
            }

            drop(held);
            let bin = FakeBin(Mutex::new(vec![]));
            repair_interrupted_swap(&folder, &|paths| bin.remove(paths));
            assert!(!prev.exists());
            assert!(!tmp.exists());
            assert_eq!(
                fs::read(&audio).unwrap(),
                if has_audio {
                    vec![2; 150]
                } else {
                    vec![1; 100]
                }
            );
            assert_eq!(
                *bin.0.lock().unwrap(),
                if has_audio { vec![prev] } else { vec![] }
            );
            assert!(claim(&folder).is_some());
        }
    }

    #[test]
    fn interrupted_swap_keeps_ambiguous_files_and_tmp() {
        for kind in [
            "smaller",
            "directory",
            "backup-directory",
            "recycle-failure",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let folder = dunce::canonicalize(dir.path()).unwrap();
            let prev = folder.join(".audio.prev.mp4");
            let audio = folder.join("audio.mp4");
            let tmp = folder.join(".audio.merge.tmp.mp4");
            if kind == "backup-directory" {
                fs::create_dir(&prev).unwrap();
            } else {
                fs::write(&prev, vec![1; 100]).unwrap();
            }
            if kind == "directory" {
                fs::create_dir(&audio).unwrap();
            } else {
                fs::write(&audio, vec![2; if kind == "smaller" { 50 } else { 100 }]).unwrap();
            }
            fs::write(&tmp, "keep temporary output").unwrap();
            repair_interrupted_swap(&folder, &|_| {
                assert_eq!(kind, "recycle-failure");
                Err("Recycle failed".into())
            });
            assert!(prev.exists(), "{kind}");
            assert!(audio.exists(), "{kind}");
            assert_eq!(fs::read(&tmp).unwrap(), b"keep temporary output", "{kind}");
            if kind != "backup-directory" {
                assert_eq!(fs::read(&prev).unwrap(), vec![1; 100], "{kind}");
            }
            if kind != "directory" {
                assert_eq!(
                    fs::read(&audio).unwrap(),
                    vec![2; if kind == "smaller" { 50 } else { 100 }],
                    "{kind}"
                );
            }
        }
    }

    #[test]
    fn corrupt_new_part_fails_before_concat() {
        let base = tempfile::tempdir().unwrap();
        let old = base.path().join("old.mp4");
        let new = base.path().join("new.mp4");
        let tmp = base.path().join("tmp.mp4");
        let checked = Mutex::new(Vec::new());
        let result = join_parts(
            &base.path().join("nonexistent-ffmpeg"),
            &old,
            &new,
            &tmp,
            &|p| {
                checked.lock().unwrap().push(p.to_path_buf());
                if p == new {
                    Err(format!("unreadable {}", p.display()))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(
            result.unwrap_err(),
            "The new recording's audio is unreadable."
        );
        assert_eq!(*checked.lock().unwrap(), vec![old, new]);
        assert!(!base.path().join(".checkpoints").exists());
        assert!(!tmp.exists());
    }

    #[tokio::test]
    async fn corrupt_new_part_keeps_the_old_meeting_and_its_folder() {
        let (pool, base, old, new, id) = setup_merge().await;
        let bin = FakeBin(Mutex::new(vec![]));
        let result = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[],
            false,
            base.path(),
            &|p| bin.remove(p),
            |a, b, tmp| async move {
                join_parts(
                    &tmp.with_file_name("nonexistent-ffmpeg"),
                    &a,
                    &b,
                    &tmp,
                    &|p| {
                        if p == b {
                            Err(format!("unreadable {}", p.display()))
                        } else {
                            Ok(())
                        }
                    },
                )
            },
        )
        .await;
        let error = result.unwrap_err();
        assert!(error.starts_with("Merging the audio failed"));
        assert!(!error.contains(base.path().to_str().unwrap()));
        assert!(!error.contains("Earlier meeting"));
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert!(new.exists());
        assert!(bin.0.lock().unwrap().iter().all(|p| !p.starts_with(&new)));
        assert!(!old.join(".audio.merge.tmp.mp4").exists());
    }

    #[test]
    fn duration_mismatch_is_rejected() {
        assert!(duration_matches(100.0, Some(20.0), Some(120.0)));
        assert!(duration_matches(100.0, Some(20.0), Some(122.0)));
        assert!(!duration_matches(100.0, Some(20.0), Some(100.0)));
        assert!(!duration_matches(100.0, None, Some(120.0)));
        assert!(!duration_matches(100.0, Some(20.0), None));
        assert!(!duration_matches(f64::NAN, Some(20.0), Some(120.0)));
        assert!(!duration_matches(100.0, Some(f64::INFINITY), Some(120.0)));
        assert!(!duration_matches(100.0, Some(20.0), Some(f64::NAN)));
        assert!(!duration_matches(100.0, Some(-20.0), Some(80.0)));
    }

    #[tokio::test]
    async fn leftover_merge_tmp_is_removed_when_audio_is_intact() {
        let (pool, base, old, new, id) = setup_merge().await;
        let tmp = old.join(".audio.merge.tmp.mp4");
        fs::write(&tmp, "leftover bytes").unwrap();
        let bin = FakeBin(Mutex::new(vec![]));
        continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[],
            false,
            base.path(),
            &|p| bin.remove(p),
            |a, b, tmp| async move {
                assert!(!tmp.exists());
                fake_join(a, b, tmp).await
            },
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read(old.join("audio.mp4")).unwrap(),
            [vec![1; 100], vec![2; 100]].concat()
        );
        assert!(!tmp.exists());
    }

    #[tokio::test]
    async fn leftover_prev_still_blocks() {
        let (pool, base, old, new, id) = setup_merge().await;
        let prev = old.join(".audio.prev.mp4");
        fs::write(&prev, vec![3; 200]).unwrap();
        let result = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[],
            false,
            base.path(),
            &|_| panic!("must not recycle"),
            |_, _, _| async {
                panic!("must not join");
                #[allow(unreachable_code)]
                Ok(10.0)
            },
        )
        .await;
        assert_eq!(
            result.unwrap_err(),
            "Merging the audio failed: an earlier merge needs recovery."
        );
        assert_eq!(fs::read(prev).unwrap(), vec![3; 200]);
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
    }

    #[test]
    fn parse_ffmpeg_duration_reads_the_container_line() {
        assert_eq!(
            parse_ffmpeg_duration("  Duration: 01:02:03.45, start: 0.000000"),
            Some(3723.45)
        );
        for input in [
            "Duration: N/A",
            "",
            "Duration: 00:00:inf",
            "Duration: 00:00:NaN",
        ] {
            assert_eq!(parse_ffmpeg_duration(input), None);
        }
    }

    #[test]
    fn join_levels_pads_or_trims_the_old_track() {
        let new = vec![(0.3, 0.4); 2];
        let joined = join_levels(Some(vec![(0.1, 0.2); 3]), 0.25, Some(new.clone())).unwrap();
        assert_eq!(
            joined,
            [vec![(0.1, 0.2); 3], vec![(0.0, 0.0); 2], new.clone()].concat()
        );
        assert_eq!(
            join_levels(Some(vec![(0.1, 0.2); 10]), 0.25, Some(new.clone())).unwrap(),
            [vec![(0.1, 0.2); 5], new.clone()].concat()
        );
        assert_eq!(join_levels(None, 0.25, None), None);
        assert_eq!(
            join_levels(None, 0.25, Some(new.clone())).unwrap(),
            [vec![(0.0, 0.0); 5], new].concat()
        );
    }

    #[tokio::test]
    async fn continue_appends_and_replaces_audio_after_commit() {
        let (pool, base, old, new, id) = setup_merge().await;
        crate::speakers::levels::write_levels_file(&old, &[(0.1, 0.2)]).unwrap();
        crate::speakers::levels::write_levels_file(&new, &[(0.3, 0.4)]).unwrap();
        let bin = FakeBin(Mutex::new(vec![]));
        let result = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[],
            false,
            base.path(),
            &|p| bin.remove(p),
            fake_join,
        )
        .await
        .unwrap();
        assert_eq!(result.meeting_id, id);
        assert_eq!(result.title, "Earlier meeting");
        assert_eq!(
            fs::read(old.join("audio.mp4")).unwrap(),
            [vec![1; 100], vec![2; 100]].concat()
        );
        let times: Vec<(f64,)> = sqlx::query_as("SELECT audio_start_time FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time")
            .bind(&id).fetch_all(&pool).await.unwrap();
        assert_eq!(times, vec![(1.0,), (11.0,)]);
        let removed = bin.0.lock().unwrap();
        assert_eq!(removed[0], old.join(".audio.prev.mp4"));
        assert!(removed[1..].iter().all(|p| p.starts_with(&new)));
        assert!(!new.exists());
        assert!(!old.join(".audio.merge.tmp.mp4").exists());
        let levels =
            crate::speakers::levels::decode(&fs::read(old.join("levels.bin")).unwrap()).unwrap();
        assert_eq!(levels.len(), 201);
        assert_eq!(levels[200], (0.3, 0.4));
    }

    #[tokio::test]
    async fn continue_with_speakers_off_keeps_finished_job() {
        let (pool, base, _, new, id) = setup_merge().await;
        let mut tx = pool.begin().await.unwrap();
        crate::database::repositories::speakers::SpeakersRepository::enqueue_in_tx(&mut tx, &id)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("UPDATE speaker_jobs SET status = 'done' WHERE meeting_id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        let bin = FakeBin(Mutex::new(vec![]));
        continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[],
            false,
            base.path(),
            &|p| bin.remove(p),
            fake_join,
        )
        .await
        .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT status FROM speaker_jobs WHERE meeting_id = ?")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            "done"
        );
    }

    #[tokio::test]
    async fn failed_join_leaves_the_old_meeting_untouched() {
        let (pool, base, old, new, id) = setup_merge().await;
        let result = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[note()],
            false,
            base.path(),
            &|_| panic!("must not recycle"),
            |_, _, tmp| async move {
                fs::write(tmp, "partial").unwrap();
                Err("join failed".into())
            },
        )
        .await;
        assert!(result.unwrap_err().starts_with("Merging the audio failed"));
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transcripts")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            MeetingNotesRepository::get(&pool, &id).await.unwrap(),
            vec![note()]
        );
        assert!(new.exists());
        assert!(!old.join(".audio.merge.tmp.mp4").exists());
    }

    #[tokio::test]
    async fn incomplete_merge_is_rejected_and_tmp_removed() {
        let (pool, base, old, new, id) = setup_merge().await;
        let result = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[],
            &[],
            false,
            base.path(),
            &|_| panic!("must not recycle"),
            |_, _, tmp| async move {
                fs::write(tmp, vec![0; 10]).unwrap();
                Ok(10.0)
            },
        )
        .await;
        assert!(result.is_err());
        assert!(!old.join(".audio.merge.tmp.mp4").exists());
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
    }

    #[tokio::test]
    async fn invalid_offsets_remove_tmp_and_preserve_both_recordings() {
        for offset in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let (pool, base, old, new, id) = setup_merge().await;
            let result = continue_into_with(
                &pool,
                &id,
                new.to_str(),
                &[segment()],
                &[],
                false,
                base.path(),
                &|_| panic!("must not recycle"),
                |a, b, tmp| async move {
                    fake_join(a, b, tmp).await?;
                    Ok(offset)
                },
            )
            .await;
            assert!(result.unwrap_err().starts_with("Merging the audio failed"));
            assert!(!old.join(".audio.merge.tmp.mp4").exists());
            assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
            assert!(new.join("audio.mp4").exists());
        }
    }

    #[tokio::test]
    async fn failed_audio_rename_rolls_back_appended_rows() {
        let (pool, base, old, new, id) = setup_merge().await;
        let result = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[note()],
            false,
            base.path(),
            &|_| panic!("must not recycle"),
            |a, b, tmp| async {
                let offset = fake_join(a, b, tmp).await?;
                // Force the first rename to fail without touching the original file.
                fs::create_dir(old.join(".audio.prev.mp4")).unwrap();
                Ok(offset)
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transcripts")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            MeetingNotesRepository::get(&pool, &id).await.unwrap(),
            vec![note()]
        );
        assert!(!old.join(".audio.merge.tmp.mp4").exists());
    }

    #[tokio::test]
    async fn failed_commit_restores_original_audio_and_rows() {
        let (pool, base, old, new, id) = setup_merge().await;
        sqlx::query("CREATE TABLE continuation_commit_guard (meeting_id TEXT REFERENCES meetings(id) DEFERRABLE INITIALLY DEFERRED)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TRIGGER fail_continuation_commit AFTER INSERT ON transcripts BEGIN INSERT INTO continuation_commit_guard VALUES ('missing'); END")
            .execute(&pool).await.unwrap();
        let result = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[note()],
            false,
            base.path(),
            &|_| panic!("must not recycle"),
            fake_join,
        )
        .await;
        assert!(result.unwrap_err().contains("commit"));
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
        assert!(!old.join(".audio.prev.mp4").exists());
        assert!(!old.join(".audio.merge.tmp.mp4").exists());
        assert!(new.join("audio.mp4").exists());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transcripts")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            MeetingNotesRepository::get(&pool, &id).await.unwrap(),
            vec![note()]
        );
    }

    #[tokio::test]
    async fn recycling_failure_after_commit_still_returns_success() {
        let (pool, base, old, new, id) = setup_merge().await;
        let result = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[],
            false,
            base.path(),
            &|_| Err("bin unavailable".into()),
            fake_join,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap().len(), 200);
        assert!(old.join(".audio.prev.mp4").exists());
        assert!(new.exists());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transcripts")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn missing_target_row_rolls_back_and_restores() {
        let (pool, base, old, new, id) = setup_merge().await;
        let result = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[],
            false,
            base.path(),
            &|_| panic!("must not recycle"),
            |a, b, tmp| async {
                sqlx::query("DELETE FROM meetings WHERE id = ?")
                    .bind(&id)
                    .execute(&pool)
                    .await
                    .unwrap();
                fake_join(a, b, tmp).await
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
        assert!(!old.join(".audio.prev.mp4").exists());
        assert!(!old.join(".audio.merge.tmp.mp4").exists());
    }

    #[tokio::test]
    async fn ineligible_target_saves_a_new_meeting_with_continue_error() {
        let (pool, base, old, new, id) = setup_merge().await;
        sqlx::query("UPDATE meetings SET audio_deleted_at = 'deleted' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        let continue_error = continue_into_with(
            &pool,
            &id,
            new.to_str(),
            &[segment()],
            &[],
            false,
            base.path(),
            &|_| panic!("must not recycle"),
            fake_join,
        )
        .await
        .unwrap_err();
        assert!(continue_error.contains("deleted"));
        let saved = TranscriptsRepository::save_transcript(
            &pool,
            "New",
            &[segment()],
            Some(new.to_string_lossy().into_owned()),
            &[],
            false,
            &TitleOrigin::default(),
        )
        .await
        .unwrap();
        assert_ne!(saved, id);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            MeetingNotesRepository::get(&pool, &id).await.unwrap(),
            vec![note()]
        );
        assert_eq!(fs::read(old.join("audio.mp4")).unwrap(), vec![1; 100]);
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
