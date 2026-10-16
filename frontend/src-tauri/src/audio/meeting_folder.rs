//! The one place that decides whether a meeting folder may be read or deleted (spec §8.3).
//! Conclave never deletes outside the recordings folder and never permanently: see `recycle`.
//! Remaining window: another process running as the user could swap a path between the
//! per-file re-check in `recycle_checked` and the Recycle Bin call for that one file.
use log::warn;
use std::fs;
use std::path::{Path, PathBuf};

/// The import formats (`import.rs` copies a file in as `audio.<ext>`), so every format is found.
pub use crate::audio::constants::AUDIO_EXTENSIONS;

pub const MEETING_FILES: [&str; 4] = ["transcripts.json", "metadata.json", ".transcripts.json.tmp", ".metadata.json.tmp"];

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
    std::thread::spawn(move || trash::delete_all(&owned).map_err(|e| format!("Could not move to the Recycle Bin: {e}")))
        .join()
        .map_err(|_| "The Recycle Bin call failed unexpectedly".to_string())?
}

/// Canonical path of a meeting folder strictly inside the recordings base, or `None`.
/// Canonicalizing first resolves `..` and links, so nothing outside the base can qualify.
pub fn guarded_meeting_folder(folder_path: &str, recordings_base: &Path) -> Option<PathBuf> {
    if folder_path.trim().is_empty() {
        return None;
    }
    let (Ok(folder), Ok(base)) = (dunce::canonicalize(folder_path), dunce::canonicalize(recordings_base)) else {
        warn!("Meeting folder guard: cannot resolve {folder_path}");
        return None;
    };
    if folder.is_dir() && folder != base && folder.starts_with(&base) {
        Some(folder)
    } else {
        warn!("Meeting folder guard: refusing {} (not inside {})", folder.display(), base.display());
        None
    }
}

/// A real file, not a symlink/junction (links could point anywhere on disk).
fn is_plain_file(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file())
}

fn is_plain_dir(p: &Path) -> bool {
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
pub fn recycle_checked(path: &Path, folder: &Path, remove: Remover) -> Result<(), String> {
    let parent_ok = path
        .parent()
        .and_then(|p| dunce::canonicalize(p).ok())
        .is_some_and(|p| p == folder || p == folder.join(".checkpoints"));
    if !parent_ok || !is_plain_file(path) {
        return Err(format!("Skipped {}: it is no longer a Conclave file in the meeting folder", path.display()));
    }
    remove(&[path.to_path_buf()])
}

/// Recycles a real, empty directory. `Ok(false)` if it is missing, a link or not empty.
pub fn recycle_if_empty(dir: &Path, remove: Remover) -> Result<bool, String> {
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
        .ok_or_else(|| format!("Recording folder kept: it is outside the Conclave recordings folder ({folder_path})"))?;
    let files = conclave_files(&folder)
        .ok_or_else(|| format!("Recording folder kept: it contains other files ({})", folder.display()))?;
    for f in &files {
        recycle_checked(f, &folder, remove)?;
    }
    recycle_if_empty(&folder.join(".checkpoints"), remove)?;
    if guarded_meeting_folder(&folder.to_string_lossy(), recordings_base).as_ref() != Some(&folder)
        || !recycle_if_empty(&folder, remove)?
    {
        return Err(format!("Recording folder kept: new files appeared in it ({})", folder.display()));
    }
    Ok(())
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
        Some(p) => {
            let (others,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meetings WHERE folder_path = ?")
                .bind(&p)
                .fetch_one(pool)
                .await?;
            if others > 0 {
                Some(format!("Recording folder kept: another meeting uses it ({p})"))
            } else {
                delete_meeting_folder(&p, recordings_base, remove).err()
            }
        }
    };
    if let Some(w) = &warning {
        warn!("Meeting {meeting_id} deleted, folder kept: {w}");
    }
    Ok(Some(warning))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    fn base() -> tempfile::TempDir { tempfile::tempdir().unwrap() }

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
        assert!(recycle_checked(&m.join(".checkpoints").join("audio_chunk_000.mp4"), &dunce::canonicalize(&m).unwrap(), &|p| bin.remove(p)).is_err());
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
}
