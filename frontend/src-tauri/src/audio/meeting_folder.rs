//! The one place that decides whether a meeting folder may be read or deleted (spec §8.3).
//! Conclave never deletes outside the recordings folder. Everything the user owns goes to the
//! Recycle Bin (`recycle`); only Conclave's own temporary files are removed permanently (see
//! `delete_checkpoint_files` and `discard_import_folder`).
//! Remaining window: another process running as the user could swap a path between the
//! per-file re-check in `remove_checked` and the Recycle Bin call for that one file.
use crate::audio::recording_preferences::get_default_recordings_folder;
use log::warn;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

/// The import formats (`import.rs` copies a file in as `audio.<ext>`), so every format is found.
pub use crate::audio::constants::AUDIO_EXTENSIONS;

pub const MEETING_FILES: [&str; 6] = ["transcripts.json", "metadata.json", ".transcripts.json.tmp", ".metadata.json.tmp", "levels.bin", ".levels.bin.tmp"];

/// Every deleting function takes this, so tests never touch the real Recycle Bin.
/// `Sync` keeps the async commands that hold it across `.await` `Send`.
pub type Remover<'a> = &'a (dyn Fn(&[PathBuf]) -> Result<(), String> + Sync);

/// Moves paths to the Windows Recycle Bin. With `trash`'s flags Windows asks the user before
/// permanently deleting anything too big for the Bin; declining returns an error here.
/// Runs on its own thread so `trash` gets a fresh COM apartment.
/// shortcut: the join blocks one async worker while Windows shows a prompt; move to spawn_blocking if that ever matters.
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let owned = paths.to_vec();
    std::thread::spawn(move || trash::delete_all(&owned).map_err(|e| format!("Could not move to the Recycle Bin: {}", trash_error_name(&e))))
        .join()
        .map_err(|_| "The Recycle Bin call failed unexpectedly".to_string())?
}

/// The variant name of a `trash` error. Its Display prints the Debug form, which carries the paths.
fn trash_error_name(e: &trash::Error) -> String {
    // The derived Debug starts with the variant name, and the fields follow after `{`, `(` or a space.
    format!("{e:?}").split(['{', '(', ' ']).next().unwrap_or_default().to_string()
}

/// Canonical path of a meeting folder that is a direct child of the recordings base, or `None`.
/// The folder itself must not be a symlink or (on Windows) a junction; the base may be reached
/// through a link, since it is resolved. Meeting folders are always direct children (see
/// `create_meeting_folder`), so a link or a nested folder never qualifies.
pub fn guarded_meeting_folder(folder_path: &str, recordings_base: &Path) -> Option<PathBuf> {
    if folder_path.trim().is_empty() {
        return None;
    }
    // Lexical leaf: a trailing separator would make lstat follow a link, so it is dropped here.
    // std reports junctions as symlinks on Windows (`is_symlink` covers mount points), so
    // `!is_symlink()` refuses them too, without an attribute check that would refuse cloud folders.
    let leaf = Path::new(folder_path);
    let plain_dir = leaf.parent().zip(leaf.file_name()).is_some_and(|(parent, name)| {
        fs::symlink_metadata(parent.join(name)).is_ok_and(|m| m.file_type().is_dir() && !m.file_type().is_symlink())
    });
    let (Ok(folder), Ok(base)) = (dunce::canonicalize(folder_path), dunce::canonicalize(recordings_base)) else {
        warn!("Meeting folder guard: cannot resolve the folder");
        return None;
    };
    if plain_dir && folder.parent() == Some(base.as_path()) {
        Some(folder)
    } else {
        warn!("Meeting folder guard: refusing a folder that is not directly inside the recordings folder");
        None
    }
}

/// A real file, not a symlink/junction (links could point anywhere on disk).
fn is_plain_file(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file())
}

pub(crate) fn is_plain_dir(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
}

/// Position of `name`'s extension in `AUDIO_EXTENSIONS` if it is `audio.<ext>`, any case
/// (imports keep the source file's case, e.g. `audio.MP3`).
fn audio_rank(name: &str) -> Option<usize> {
    let (stem, ext) = name.split_once('.')?;
    if !stem.eq_ignore_ascii_case("audio") {
        return None;
    }
    AUDIO_EXTENSIONS.iter().position(|e| e.eq_ignore_ascii_case(ext))
}

fn is_checkpoint_name(name: &str) -> bool {
    name.starts_with("audio_chunk_") || name == "concat_list.txt"
}

pub fn audio_files(folder: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(folder) else { return Vec::new() };
    let mut found: Vec<(usize, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| audio_rank(&e.file_name().to_string_lossy()).map(|r| (r, e.path())))
        .collect();
    found.sort_by_key(|(r, _)| *r);
    found.into_iter().map(|(_, p)| p).collect()
}

pub fn find_audio_file(folder: &Path) -> Option<PathBuf> {
    audio_files(folder).into_iter().next()
}

/// Conclave's own files in `.checkpoints/` (`audio_chunk_NNN.mp4`, `concat_list.txt`, see
/// `incremental_saver.rs`). Empty when `.checkpoints` is missing or is a link.
pub fn checkpoint_files(checkpoints: &Path) -> Vec<PathBuf> {
    if !is_plain_dir(checkpoints) {
        return Vec::new();
    }
    fs::read_dir(checkpoints)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter(|e| is_checkpoint_name(&e.file_name().to_string_lossy()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// Re-checks one file right before recycling it: a real file whose parent resolves to exactly
/// the (canonical) meeting folder or its `.checkpoints`.
pub fn remove_checked(path: &Path, folder: &Path, remove: Remover) -> Result<(), String> {
    let parent_ok = path
        .parent()
        .and_then(|p| dunce::canonicalize(p).ok())
        .is_some_and(|p| p == folder || p == folder.join(".checkpoints"));
    if !parent_ok || !is_plain_file(path) {
        return Err("Skipped a file: it is no longer a Conclave file in the meeting folder".to_string());
    }
    remove(&[path.to_path_buf()])
}

/// Recycles a real, empty directory. `Ok(false)` if it is missing, a link or not empty.
pub fn remove_if_empty(dir: &Path, remove: Remover) -> Result<bool, String> {
    let empty = is_plain_dir(dir) && fs::read_dir(dir).is_ok_and(|mut rd| rd.next().is_none());
    if !empty {
        return Ok(false);
    }
    remove(&[dir.to_path_buf()]).map(|_| true)
}

/// The files to recycle for a whole-folder delete, checkpoint files first and audio last,
/// or `None` if the folder holds anything Conclave did not write (links never count as ours).
fn conclave_files(folder: &Path) -> Option<Vec<PathBuf>> {
    let mut checkpoints = Vec::new();
    let mut json = Vec::new();
    for e in fs::read_dir(folder).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        match e.file_type().ok()? {
            t if t.is_file() && audio_rank(&name).is_some() => {}
            t if t.is_file() && MEETING_FILES.contains(&name.as_str()) => json.push(e.path()),
            t if t.is_dir() && name == ".checkpoints" => {
                let files = checkpoint_files(&e.path());
                if fs::read_dir(e.path()).ok()?.count() != files.len() {
                    return None;
                }
                checkpoints = files;
            }
            _ => return None,
        }
    }
    Some([checkpoints, json, audio_files(folder)].concat())
}

/// Moves a meeting folder to the Recycle Bin item by item, but only a folder inside the
/// recordings base that holds nothing but Conclave's files. Checked completely first.
pub fn delete_meeting_folder(folder_path: &str, recordings_base: &Path, remove: Remover) -> Result<(), String> {
    if !Path::new(folder_path).exists() {
        return Ok(());
    }
    let folder = guarded_meeting_folder(folder_path, recordings_base)
        .ok_or_else(|| "Recording folder kept: it is outside the Conclave recordings folder".to_string())?;
    let files = conclave_files(&folder)
        .ok_or_else(|| "Recording folder kept: it contains other files".to_string())?;
    for f in &files {
        remove_checked(f, &folder, remove)?;
    }
    ensure_still_guarded(&folder, recordings_base, "Recording folder")?;
    remove_if_empty(&folder.join(".checkpoints"), remove)?;
    ensure_still_guarded(&folder, recordings_base, "Recording folder")?;
    if !remove_if_empty(&folder, remove)? {
        return Err("Recording folder kept: new files appeared in it".to_string());
    }
    Ok(())
}

/// Re-run before each later step: the folder may have been replaced by a link meanwhile.
/// `what` names the kept thing in the error ("Recording folder" or "Checkpoints").
pub(crate) fn ensure_still_guarded(folder: &Path, recordings_base: &Path, what: &str) -> Result<(), String> {
    if guarded_meeting_folder(&folder.to_string_lossy(), recordings_base).as_deref() == Some(folder) {
        Ok(())
    } else {
        Err(format!("{what} kept: it is no longer inside the recordings folder"))
    }
}

/// Permanent removal, for Conclave's own temporary files only (`delete_checkpoint_files`,
/// `discard_import_folder`). `remove_dir` refuses a non-empty folder, so nothing is recursive.
fn remove_permanently(paths: &[PathBuf]) -> Result<(), String> {
    for p in paths {
        let removed = if is_plain_dir(p) { fs::remove_dir(p) } else { fs::remove_file(p) };
        removed.map_err(|e| format!("Could not delete a file ({:?}): {e}", e.kind()))?;
    }
    Ok(())
}

/// Permanently deletes Conclave's own chunk files in a guarded folder, then `.checkpoints` if
/// empty. Other files in `.checkpoints` are left alone, so the folder stays. `folder` must be
/// the canonical path from `guarded_meeting_folder`.
pub fn delete_checkpoint_files(folder: &Path) -> Result<(), String> {
    let checkpoints = folder.join(".checkpoints");
    for f in checkpoint_files(&checkpoints) {
        remove_checked(&f, folder, &remove_permanently)?;
    }
    // Compare canonical paths: `is_plain_dir` follows a folder swapped for a link after the guard.
    if dunce::canonicalize(&checkpoints).is_ok_and(|c| c == checkpoints) {
        remove_if_empty(&checkpoints, &remove_permanently)?;
    }
    Ok(())
}

/// A real, non-empty top-level `audio.<ext>`: the proof that the chunks were merged.
fn has_merged_audio(folder: &Path) -> bool {
    audio_files(folder).iter().any(|p| is_plain_file(p) && fs::symlink_metadata(p).is_ok_and(|m| m.len() > 0))
}

/// Checkpoint cleanup for the `cleanup_checkpoints` command: same guard as a meeting delete, but
/// permanent, since these are Conclave's own merged chunks. The chunks may go only once a merged
/// audio file is in the folder; otherwise they are the only copy and nothing is touched.
/// A missing folder is `Ok(())`.
pub fn cleanup_checkpoints_guarded(folder_path: &str, recordings_base: &Path) -> Result<(), String> {
    if !Path::new(folder_path).exists() {
        return Ok(());
    }
    let folder = guarded_meeting_folder(folder_path, recordings_base)
        .ok_or_else(|| "Checkpoints kept: it is outside the Conclave recordings folder".to_string())?;
    if !has_merged_audio(&folder) {
        return Err("Checkpoints kept: no merged audio file in the meeting folder".to_string());
    }
    delete_checkpoint_files(&folder)
}

/// Import cleanup after a cancel or failure. Permanently removes the files `import.rs` wrote into
/// the meeting folder it just created (the audio copy, the transcript and metadata files and their
/// `.tmp` siblings), then the folder if it is empty. Anything else keeps the folder. Non-fatal:
/// problems are logged, as before.
pub fn discard_import_folder(folder: &Path, recordings_base: &Path) {
    let Some(folder) = guarded_meeting_folder(&folder.to_string_lossy(), recordings_base) else { return };
    let mut files = audio_files(&folder);
    files.extend(MEETING_FILES.iter().map(|n| folder.join(n)));
    let removed = files
        .iter()
        .filter(|p| is_plain_file(p))
        .try_for_each(|p| remove_checked(p, &folder, &remove_permanently));
    match removed.and_then(|()| remove_if_empty(&folder, &remove_permanently)) {
        Ok(true) => {}
        Ok(false) => warn!("Import cleanup left the folder: it still holds other files"),
        Err(e) => warn!("Import cleanup left the folder: {e}"),
    }
}

/// Whether another meeting's folder is this one. Compared by canonical path, so a trailing
/// slash, a `..` or a link to the same folder still counts. Rows that cannot be resolved are
/// skipped; the deleted meeting itself is excluded by id.
async fn used_by_another_meeting(pool: &sqlx::SqlitePool, meeting_id: &str, folder_path: &str) -> Result<bool, sqlx::Error> {
    let Ok(folder) = dunce::canonicalize(folder_path) else { return Ok(false) };
    let rows: Vec<(String,)> = sqlx::query_as("SELECT folder_path FROM meetings WHERE id <> ? AND folder_path IS NOT NULL")
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().any(|(p,)| dunce::canonicalize(p).is_ok_and(|c| c == folder)))
}

/// DB delete first, then the folder; the meeting stays deleted if the folder cannot be.
pub async fn delete_meeting_and_folder(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    recordings_base: &Path,
    remove: Remover<'_>,
) -> Result<Option<Option<String>>, sqlx::Error> {
    use crate::database::repositories::meeting::MeetingsRepository;
    let folder_path = MeetingsRepository::get_meeting_metadata(pool, meeting_id).await?.and_then(|m| m.folder_path);
    if !MeetingsRepository::delete_meeting(pool, meeting_id).await? {
        return Ok(None);
    }
    let warning = match folder_path {
        None => None,
        // The row is already gone, so a failed check must keep the folder and say so.
        Some(p) => match used_by_another_meeting(pool, meeting_id, &p).await {
            Ok(false) => delete_meeting_folder(&p, recordings_base, remove).err(),
            Ok(true) => Some(format!("Recording folder kept: another meeting uses it ({p})")),
            Err(e) => Some(format!("Recording folder kept: could not check whether another meeting uses it ({p}): {e}")),
        },
    };
    // The reason names the folder, so the log records only the fact; the caller gets the reason.
    if warning.is_some() {
        warn!("Meeting {meeting_id} deleted, recording folder kept");
    }
    Ok(Some(warning))
}

#[derive(Debug, Serialize)]
pub struct MeetingAudio {
    pub path: Option<String>,
    pub keep_audio: bool,
    /// Set when the user deleted the audio in the old-audio review; picks the player message.
    pub audio_deleted_at: Option<String>,
}

pub(crate) async fn meeting_audio_inner(pool: &sqlx::SqlitePool, meeting_id: &str, base: &Path) -> Result<MeetingAudio, String> {
    let meeting = crate::database::repositories::meeting::MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Meeting not found: {meeting_id}"))?;
    let file = meeting
        .folder_path
        .as_deref()
        .and_then(|p| guarded_meeting_folder(p, base))
        .and_then(|folder| find_audio_file(&folder));
    Ok(MeetingAudio {
        path: file.map(|f| f.to_string_lossy().to_string()),
        keep_audio: meeting.keep_audio,
        audio_deleted_at: meeting.audio_deleted_at,
    })
}

/// Finds the meeting's audio and lets the asset protocol read that one file (spec §8.2).
#[tauri::command]
pub async fn get_meeting_audio<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<MeetingAudio, String> {
    let state = app.try_state::<crate::state::AppState>().ok_or("Database not ready")?;
    let audio = meeting_audio_inner(state.db_manager.pool(), &meeting_id, &get_default_recordings_folder()).await?;
    if let Some(p) = &audio.path {
        app.asset_protocol_scope().allow_file(p).map_err(|e| e.to_string())?;
    }
    Ok(audio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    fn base() -> tempfile::TempDir { tempfile::tempdir().unwrap() }

    #[test]
    fn a_recycle_error_names_its_variant_without_the_path() {
        let path = "C:\\Users\\someone\\Recordings\\Standup with Anna Ivanova";
        let err = trash::Error::CanonicalizePath { original: PathBuf::from(path) };
        assert_eq!(trash_error_name(&err), "CanonicalizePath");
        assert_eq!(trash_error_name(&trash::Error::TargetedRoot), "TargetedRoot");
    }

    /// Stand-in for the Recycle Bin. Removes files, and directories only when empty
    /// (`remove_dir` fails otherwise), so a non-empty folder ever reaching it fails the test.
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
        fn names(&self) -> Vec<String> {
            self.0.lock().unwrap().iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect()
        }
    }

    #[test]
    fn guard_accepts_only_folders_strictly_inside_the_base() {
        let b = base();
        let inside = b.path().join("Standup_2026-10-15_10-00");
        fs::create_dir(&inside).unwrap();
        assert!(guarded_meeting_folder(inside.to_str().unwrap(), b.path()).is_some());
        let dotdot_inside = inside.join("..").join("Standup_2026-10-15_10-00");
        assert!(guarded_meeting_folder(dotdot_inside.to_str().unwrap(), b.path()).is_some());

        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join("metadata.json"), "{}").unwrap(); // not a pass any more
        assert!(guarded_meeting_folder(elsewhere.path().to_str().unwrap(), b.path()).is_none());
    }

    #[test]
    fn guard_rejects_base_itself_files_missing_and_dotdot_escape() {
        let b = base();
        assert!(guarded_meeting_folder(b.path().to_str().unwrap(), b.path()).is_none());
        let file = b.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        assert!(guarded_meeting_folder(file.to_str().unwrap(), b.path()).is_none());
        assert!(guarded_meeting_folder(b.path().join("nope").to_str().unwrap(), b.path()).is_none());
        let sub = b.path().join("m");
        fs::create_dir(&sub).unwrap();
        assert!(guarded_meeting_folder(sub.join("..").join("..").to_str().unwrap(), b.path()).is_none());
        assert!(guarded_meeting_folder("", b.path()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn guard_rejects_a_link_inside_the_base_that_points_outside() {
        let b = base();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), b.path().join("sneaky")).unwrap();
        assert!(guarded_meeting_folder(b.path().join("sneaky").to_str().unwrap(), b.path()).is_none());
    }

    #[test]
    fn guard_accepts_a_direct_child_folder() {
        let b = base();
        let m = b.path().join("Standup_2026-10-15_10-00");
        fs::create_dir(&m).unwrap();
        assert!(guarded_meeting_folder(m.to_str().unwrap(), b.path()).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn guard_rejects_a_link_inside_the_base_that_points_to_a_folder_inside_it() {
        let b = base();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        std::os::unix::fs::symlink(&m, b.path().join("sneaky")).unwrap();
        assert!(guarded_meeting_folder(b.path().join("sneaky").to_str().unwrap(), b.path()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn guard_refuses_a_trailing_slash_spelling_of_a_link_folder() {
        let b = base();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        let spelled = format!("{}/", b.path().join("sneaky").display());
        std::os::unix::fs::symlink(&m, b.path().join("sneaky")).unwrap();
        assert!(guarded_meeting_folder(&spelled, b.path()).is_none());
    }

    #[test]
    fn guard_rejects_a_nested_folder() {
        let b = base();
        let nested = b.path().join("sub").join("M");
        fs::create_dir_all(&nested).unwrap();
        assert!(guarded_meeting_folder(nested.to_str().unwrap(), b.path()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn guard_accepts_a_base_reached_through_a_link() {
        let real = base();
        fs::create_dir(real.path().join("m")).unwrap();
        let holder = base();
        let link_base = holder.path().join("Recordings");
        std::os::unix::fs::symlink(real.path(), &link_base).unwrap();
        assert!(guarded_meeting_folder(link_base.join("m").to_str().unwrap(), &link_base).is_some());
    }

    #[test]
    fn audio_files_prefer_mp4_and_find_imported_formats_in_any_case() {
        let b = base();
        assert!(find_audio_file(b.path()).is_none());
        fs::write(b.path().join("audio.MP3"), "x").unwrap();
        fs::write(b.path().join("other.mp4"), "x").unwrap();
        assert_eq!(find_audio_file(b.path()).unwrap().file_name().unwrap(), "audio.MP3");
        fs::write(b.path().join("audio.mp4"), "x").unwrap();
        assert_eq!(find_audio_file(b.path()).unwrap().file_name().unwrap(), "audio.mp4");
        assert_eq!(audio_files(b.path()).len(), 2);
    }

    #[test]
    fn delete_folder_recycles_conclave_files_one_by_one_audio_last() {
        let b = base();
        let m = b.path().join("m");
        fs::create_dir_all(m.join(".checkpoints")).unwrap();
        for f in ["audio.MKV", "transcripts.json", "metadata.json", ".checkpoints/audio_chunk_000.mp4", ".checkpoints/concat_list.txt"] {
            fs::write(m.join(f), "x").unwrap();
        }
        let bin = FakeBin::new();
        delete_meeting_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).unwrap();
        let names = bin.names();
        assert_eq!(names.len(), 7); // 5 files + .checkpoints + the folder
        assert_eq!(names[names.len() - 3], "audio.MKV"); // audio is the last file
        assert_eq!(names.last().unwrap(), "m");
        assert!(!m.exists());
        assert!(delete_meeting_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_ok()); // already gone
    }

    #[test]
    fn meeting_delete_accepts_levels_file() {
        let b = base();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        for f in ["audio.mp4", "transcripts.json", "metadata.json", "levels.bin", ".levels.bin.tmp"] {
            fs::write(m.join(f), "x").unwrap();
        }
        let bin = FakeBin::new();
        delete_meeting_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).unwrap();
        assert!(!m.exists());
        assert!(bin.names().contains(&"levels.bin".to_string()));
    }

    #[test]
    fn delete_folder_touches_nothing_outside_the_base_or_with_other_files() {
        let b = base();
        let bin = FakeBin::new();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("metadata.json"), "{}").unwrap();
        fs::write(outside.path().join("audio.mp4"), "theirs").unwrap();
        assert!(delete_meeting_folder(outside.path().to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_err());

        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        fs::write(m.join("audio.mp4"), "x").unwrap();
        fs::write(m.join("thesis.docx"), "precious").unwrap();
        assert!(delete_meeting_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).unwrap_err().contains("kept"));

        assert!(bin.names().is_empty());
        for f in [outside.path().join("metadata.json"), outside.path().join("audio.mp4"), m.join("audio.mp4"), m.join("thesis.docx")] {
            assert!(f.exists(), "{}", f.display());
        }
    }

    #[test]
    fn a_file_appearing_during_the_delete_keeps_the_folder() {
        let b = base();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        fs::write(m.join("metadata.json"), "{}").unwrap();
        fs::write(m.join("audio.mp4"), "x").unwrap();
        let bin = FakeBin::new();
        let intruder = m.join("new-file.txt");
        let remove = |p: &[PathBuf]| {
            if !intruder.exists() { fs::write(&intruder, "appeared").unwrap(); }
            bin.remove(p)
        };
        assert!(delete_meeting_folder(m.to_str().unwrap(), b.path(), &remove).unwrap_err().contains("kept"));
        assert!(intruder.exists());
        assert!(!bin.names().contains(&"new-file.txt".to_string()));
        assert!(!bin.names().contains(&"m".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn links_inside_a_meeting_folder_are_never_followed() {
        use std::os::unix::fs::symlink;
        let b = base();
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join("audio_chunk_000.mp4"), "victim").unwrap();
        fs::write(target.path().join("secret.mp4"), "secret").unwrap();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        symlink(target.path(), m.join(".checkpoints")).unwrap();
        symlink(target.path().join("secret.mp4"), m.join("audio.mp4")).unwrap();
        assert!(checkpoint_files(&m.join(".checkpoints")).is_empty());
        assert!(find_audio_file(&m).is_none());
        let bin = FakeBin::new();
        assert!(delete_meeting_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_err());
        assert!(remove_checked(&m.join(".checkpoints").join("audio_chunk_000.mp4"), &dunce::canonicalize(&m).unwrap(), &|p| bin.remove(p)).is_err());
        assert!(bin.names().is_empty());
        assert!(target.path().join("audio_chunk_000.mp4").exists());
        assert!(target.path().join("secret.mp4").exists());
    }

    async fn meeting(pool: &sqlx::SqlitePool, id: &str, folder: &Path) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z', ?)")
            .bind(id).bind(folder.to_string_lossy().to_string())
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn meeting_stays_deleted_when_its_folder_cannot_be() {
        let pool = crate::database::repositories::test_pool().await;
        let b = base();
        let outside = tempfile::tempdir().unwrap(); // fails the guard
        meeting(&pool, "m1", outside.path()).await;
        let bin = FakeBin::new();
        let result = delete_meeting_and_folder(&pool, "m1", b.path(), &|p| bin.remove(p)).await.unwrap();
        assert!(matches!(result, Some(Some(_))));
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meetings").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0);
        assert!(outside.path().exists());
        assert!(bin.names().is_empty());
        assert!(delete_meeting_and_folder(&pool, "m1", b.path(), &|p| bin.remove(p)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_folder_shared_by_two_meetings_is_kept() {
        let pool = crate::database::repositories::test_pool().await;
        let b = base();
        let m = b.path().join("Standup_2026-10-15_10-00");
        fs::create_dir(&m).unwrap();
        fs::write(m.join("audio.mp4"), "x").unwrap();
        meeting(&pool, "a", &m).await;
        meeting(&pool, "b", &m).await;
        let bin = FakeBin::new();
        let w = delete_meeting_and_folder(&pool, "a", b.path(), &|p| bin.remove(p)).await.unwrap().unwrap().unwrap();
        assert!(w.contains("another meeting"));
        assert!(m.join("audio.mp4").exists());
        assert!(bin.names().is_empty());
    }

    /// Inserts a meeting with the exact folder string the webview would send.
    async fn meeting_at(pool: &sqlx::SqlitePool, id: &str, folder: &str) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z', ?)")
            .bind(id).bind(folder)
            .execute(pool).await.unwrap();
    }

    /// Meeting `a` owns `m`; meeting `b` stores the same folder as `spelling`. Deleting `a` keeps it.
    async fn shared_under_spelling_is_kept(base: &Path, m: &Path, spelling: &str) {
        let pool = crate::database::repositories::test_pool().await;
        fs::write(m.join("audio.mp4"), "x").unwrap();
        meeting_at(&pool, "a", m.to_str().unwrap()).await;
        meeting_at(&pool, "b", spelling).await;
        let bin = FakeBin::new();
        let w = delete_meeting_and_folder(&pool, "a", base, &|p| bin.remove(p)).await.unwrap().unwrap().unwrap();
        assert!(w.contains("another meeting"), "{spelling}: {w}");
        assert!(m.join("audio.mp4").exists());
        assert!(bin.names().is_empty());
    }

    #[tokio::test]
    async fn a_folder_shared_by_a_trailing_slash_spelling_is_kept() {
        let b = base();
        let m = b.path().join("Standup_2026-10-15_10-00");
        fs::create_dir(&m).unwrap();
        let spelling = format!("{}{}", m.display(), std::path::MAIN_SEPARATOR);
        shared_under_spelling_is_kept(b.path(), &m, &spelling).await;
    }

    #[tokio::test]
    async fn a_folder_shared_by_a_dotdot_spelling_is_kept() {
        let b = base();
        let m = b.path().join("Standup_2026-10-15_10-00");
        fs::create_dir(&m).unwrap();
        let spelling = m.join("..").join("Standup_2026-10-15_10-00");
        shared_under_spelling_is_kept(b.path(), &m, spelling.to_str().unwrap()).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_folder_shared_through_a_link_inside_the_base_is_kept() {
        let b = base();
        let m = b.path().join("Standup_2026-10-15_10-00");
        fs::create_dir(&m).unwrap();
        let link = b.path().join("sneaky");
        std::os::unix::fs::symlink(&m, &link).unwrap();
        shared_under_spelling_is_kept(b.path(), &m, link.to_str().unwrap()).await;
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_mid_delete_leaves_checkpoints_outside_alone() {
        use std::os::unix::fs::symlink;
        use std::sync::atomic::{AtomicBool, Ordering};
        let b = base();
        let outside = tempfile::tempdir().unwrap();
        let m = b.path().join("m");
        fs::create_dir_all(m.join(".checkpoints")).unwrap();
        fs::write(m.join(".checkpoints").join("audio_chunk_000.mp4"), "x").unwrap();
        let moved = outside.path().join("moved");
        let bin = FakeBin::new();
        let swapped = AtomicBool::new(false);
        let remove = |p: &[PathBuf]| -> Result<(), String> {
            bin.remove(p)?;
            if !swapped.swap(true, Ordering::SeqCst) {
                // The folder leaves the base after the first file; a link takes its place.
                fs::rename(&m, &moved).unwrap();
                symlink(&moved, &m).unwrap();
            }
            Ok(())
        };
        assert!(delete_meeting_folder(m.to_str().unwrap(), b.path(), &remove).unwrap_err().contains("no longer inside"));
        assert!(moved.join(".checkpoints").is_dir(), "a folder outside the base must keep its .checkpoints");
        assert_eq!(bin.names(), vec!["audio_chunk_000.mp4".to_string()]);
    }

    #[test]
    fn checkpoint_cleanup_leaves_a_folder_outside_the_base_intact() {
        let b = base();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(outside.path().join(".checkpoints")).unwrap();
        fs::write(outside.path().join(".checkpoints").join("audio_chunk_000.mp4"), "x").unwrap();
        assert!(cleanup_checkpoints_guarded(outside.path().to_str().unwrap(), b.path()).is_err());
        assert!(outside.path().join(".checkpoints").join("audio_chunk_000.mp4").exists());
        assert!(cleanup_checkpoints_guarded(b.path().join("nope").to_str().unwrap(), b.path()).is_ok());
    }

    #[test]
    fn checkpoint_cleanup_deletes_only_conclave_chunks_inside_the_base() {
        let b = base();
        let m = b.path().join("m");
        let cp = m.join(".checkpoints");
        fs::create_dir_all(&cp).unwrap();
        for f in ["audio_chunk_000.mp4", "audio_chunk_001.mp4", "concat_list.txt"] {
            fs::write(cp.join(f), "x").unwrap();
        }
        fs::write(cp.join("keep.txt"), "mine").unwrap();
        fs::write(m.join("audio.mp4"), "x").unwrap();
        cleanup_checkpoints_guarded(m.to_str().unwrap(), b.path()).unwrap();
        for f in ["audio_chunk_000.mp4", "audio_chunk_001.mp4", "concat_list.txt"] {
            assert!(!cp.join(f).exists(), "{f}");
        }
        assert!(cp.join("keep.txt").exists()); // a foreign file keeps .checkpoints
        assert!(m.join("audio.mp4").exists());
    }

    #[test]
    fn checkpoint_cleanup_refuses_a_folder_without_merged_audio() {
        let b = base();
        let m = b.path().join("m");
        let cp = m.join(".checkpoints");
        fs::create_dir_all(&cp).unwrap();
        fs::write(cp.join("audio_chunk_000.mp4"), "x").unwrap();
        fs::write(cp.join("concat_list.txt"), "x").unwrap();
        // Chunks with no audio file are the only copy of the recording: nothing is deleted.
        assert!(cleanup_checkpoints_guarded(m.to_str().unwrap(), b.path()).is_err());
        assert!(cp.join("audio_chunk_000.mp4").exists());
        assert!(cp.join("concat_list.txt").exists());
        // An empty audio file is not a merge either.
        fs::write(m.join("audio.mp4"), "").unwrap();
        assert!(cleanup_checkpoints_guarded(m.to_str().unwrap(), b.path()).is_err());
        assert!(cp.join("audio_chunk_000.mp4").exists());
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_cleanup_does_not_accept_a_linked_audio_file_as_merged() {
        let b = base();
        let m = b.path().join("m");
        let cp = m.join(".checkpoints");
        fs::create_dir_all(&cp).unwrap();
        fs::write(cp.join("audio_chunk_000.mp4"), "x").unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("theirs.mp4"), "merged").unwrap();
        std::os::unix::fs::symlink(outside.path().join("theirs.mp4"), m.join("audio.mp4")).unwrap();
        assert!(cleanup_checkpoints_guarded(m.to_str().unwrap(), b.path()).is_err());
        assert!(cp.join("audio_chunk_000.mp4").exists());
    }

    #[test]
    fn checkpoint_cleanup_deletes_checkpoints_once_it_is_empty() {
        let b = base();
        let m = b.path().join("m");
        let cp = m.join(".checkpoints");
        fs::create_dir_all(&cp).unwrap();
        fs::write(cp.join("audio_chunk_000.mp4"), "x").unwrap();
        fs::write(m.join("audio.mp4"), "merged").unwrap();
        cleanup_checkpoints_guarded(m.to_str().unwrap(), b.path()).unwrap();
        assert!(!cp.exists());
        assert!(m.is_dir());
    }

    #[test]
    fn delete_checkpoint_files_removes_chunks_and_concat_list_then_the_empty_checkpoints() {
        let b = base();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        let m = dunce::canonicalize(&m).unwrap();
        let cp = m.join(".checkpoints");
        fs::create_dir_all(&cp).unwrap();
        for f in ["audio_chunk_000.mp4", "audio_chunk_001.mp4", "concat_list.txt"] {
            fs::write(cp.join(f), "x").unwrap();
        }
        fs::write(m.join("audio.mp4"), "x").unwrap();
        delete_checkpoint_files(&m).unwrap();
        assert!(!cp.exists());
        assert!(m.join("audio.mp4").exists());
    }

    #[test]
    fn delete_checkpoint_files_keeps_an_unknown_file_and_its_folder() {
        let b = base();
        let m = dunce::canonicalize(b.path()).unwrap();
        let cp = m.join(".checkpoints");
        fs::create_dir_all(&cp).unwrap();
        fs::write(cp.join("audio_chunk_000.mp4"), "x").unwrap();
        fs::write(cp.join("keep.txt"), "mine").unwrap();
        delete_checkpoint_files(&m).unwrap();
        assert!(!cp.join("audio_chunk_000.mp4").exists());
        assert!(cp.join("keep.txt").exists());
        assert!(cp.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn delete_checkpoint_files_never_follows_a_symlinked_chunk_or_checkpoints() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.mp4"), "secret").unwrap();
        // A link named like a chunk, inside a real .checkpoints.
        let b = base();
        let m = dunce::canonicalize(b.path()).unwrap();
        let cp = m.join(".checkpoints");
        fs::create_dir_all(&cp).unwrap();
        fs::write(cp.join("audio_chunk_000.mp4"), "x").unwrap();
        symlink(outside.path().join("secret.mp4"), cp.join("audio_chunk_001.mp4")).unwrap();
        delete_checkpoint_files(&m).unwrap();
        assert!(!cp.join("audio_chunk_000.mp4").exists());
        assert!(fs::symlink_metadata(cp.join("audio_chunk_001.mp4")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(outside.path().join("secret.mp4")).unwrap(), "secret");

        // A .checkpoints that is a link to a real folder holding chunks.
        let b2 = base();
        let m2 = dunce::canonicalize(b2.path()).unwrap();
        let target = outside.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("audio_chunk_000.mp4"), "theirs").unwrap();
        symlink(&target, m2.join(".checkpoints")).unwrap();
        delete_checkpoint_files(&m2).unwrap();
        assert_eq!(fs::read_to_string(target.join("audio_chunk_000.mp4")).unwrap(), "theirs");
        assert!(fs::symlink_metadata(m2.join(".checkpoints")).unwrap().file_type().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn delete_checkpoint_files_never_removes_a_checkpoints_reached_through_a_swapped_link() {
        use std::os::unix::fs::symlink;
        // The folder was a real folder inside the base at the guard, then became a link to outside.
        let b = base();
        let outside = tempfile::tempdir().unwrap();
        let moved = outside.path().join("moved");
        fs::create_dir_all(moved.join(".checkpoints")).unwrap();
        let m = b.path().join("m");
        symlink(&moved, &m).unwrap();
        // An empty .checkpoints outside must not be removed through the link.
        delete_checkpoint_files(&m).unwrap();
        assert!(moved.join(".checkpoints").is_dir());
        // Chunks outside must not be removed either.
        fs::write(moved.join(".checkpoints").join("audio_chunk_000.mp4"), "x").unwrap();
        assert!(delete_checkpoint_files(&m).is_err());
        assert!(moved.join(".checkpoints").join("audio_chunk_000.mp4").exists());
    }

    #[test]
    fn import_cleanup_removes_the_files_it_wrote_and_the_folder() {
        let b = base();
        let m = b.path().join("Import_2026-10-16_10-00");
        fs::create_dir(&m).unwrap();
        for f in ["audio.mp3", "transcripts.json", "metadata.json", ".transcripts.json.tmp", ".metadata.json.tmp"] {
            fs::write(m.join(f), "x").unwrap();
        }
        discard_import_folder(&m, b.path());
        assert!(!m.exists());
    }

    #[test]
    fn import_cleanup_keeps_a_folder_with_an_extra_file() {
        let b = base();
        let m = b.path().join("Import_2026-10-16_10-00");
        fs::create_dir(&m).unwrap();
        fs::write(m.join("audio.mp3"), "x").unwrap();
        fs::write(m.join("metadata.json"), "{}").unwrap();
        fs::write(m.join("thesis.docx"), "precious").unwrap();
        discard_import_folder(&m, b.path());
        assert!(m.join("thesis.docx").exists());
        assert!(!m.join("audio.mp3").exists());
        assert!(!m.join("metadata.json").exists());
        assert!(m.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn import_cleanup_never_follows_a_link_and_refuses_a_folder_outside_the_base() {
        use std::os::unix::fs::symlink;
        let b = base();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("victim.json"), "mine").unwrap();
        let m = b.path().join("Import_2026-10-16_10-00");
        fs::create_dir(&m).unwrap();
        symlink(outside.path().join("victim.json"), m.join("metadata.json")).unwrap();
        discard_import_folder(&m, b.path());
        assert_eq!(fs::read_to_string(outside.path().join("victim.json")).unwrap(), "mine");
        assert!(m.join("metadata.json").exists()); // the link stays, so the folder stays
        fs::write(outside.path().join("audio.mp3"), "theirs").unwrap();
        discard_import_folder(outside.path(), b.path());
        assert!(outside.path().join("audio.mp3").exists());
    }

    #[tokio::test]
    async fn meeting_audio_never_exposes_a_folder_outside_the_base() {
        let pool = crate::database::repositories::test_pool().await;
        let b = base();
        let outside = base();
        fs::write(outside.path().join("audio.mp4"), "x").unwrap();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path, keep_audio) VALUES ('m1', 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z', ?, 0)")
            .bind(outside.path().to_string_lossy().to_string())
            .execute(&pool).await.unwrap();
        assert!(meeting_audio_inner(&pool, "m1", b.path()).await.unwrap().path.is_none());
    }

    #[tokio::test]
    async fn meeting_audio_reports_file_pin_and_deletion() {
        let pool = crate::database::repositories::test_pool().await;
        let b = base();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        fs::write(m.join("audio.mp4"), "x").unwrap();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path, keep_audio, audio_deleted_at) VALUES ('m1', 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z', ?, 1, '2026-11-20T10:00:00+00:00')")
            .bind(m.to_string_lossy().to_string())
            .execute(&pool).await.unwrap();
        let a = meeting_audio_inner(&pool, "m1", b.path()).await.unwrap();
        assert!(a.path.unwrap().ends_with("audio.mp4"));
        assert!(a.keep_audio);
        assert_eq!(a.audio_deleted_at.as_deref(), Some("2026-11-20T10:00:00+00:00"));
        fs::remove_file(m.join("audio.mp4")).unwrap();
        assert!(meeting_audio_inner(&pool, "m1", b.path()).await.unwrap().path.is_none());
        assert!(meeting_audio_inner(&pool, "nope", b.path()).await.is_err());
    }
}
