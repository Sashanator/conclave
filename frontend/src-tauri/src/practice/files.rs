//! Session folders for Practice audio. A session folder sits directly inside the recordings
//! folder and holds only Practice's own `you.wav`. Delete goes through `guarded_meeting_folder`
//! and the Recycle Bin remover, and refuses any folder with anything else in it.
#![allow(dead_code)] // Nothing calls these yet; the Practice commands wire them up in a later task.
use crate::audio::audio_processing::sanitize_filename;
use crate::audio::meeting_folder::{ensure_still_guarded, guarded_meeting_folder, Remover};
use crate::audio::recording_preferences::get_default_recordings_folder;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

pub const PRACTICE_FILES: [&str; 1] = ["you.wav"];

/// "Practice 2026-10-20 18-05 Thesis defence", sanitized and at most 80 characters.
pub fn folder_name(started: chrono::DateTime<chrono::Local>, setup_name: &str) -> String {
    let raw = format!("Practice {} {}", started.format("%Y-%m-%d %H-%M"), setup_name);
    let clipped: String = sanitize_filename(&raw).chars().take(80).collect();
    // A cut can leave a trailing space or dot, which Windows strips from folder names.
    clipped.trim_end_matches([' ', '.']).to_string()
}

/// Creates `base/name`, or `base/name (2)`, `(3)`… if taken. Never reuses an existing folder.
pub fn create_session_folder(base: &Path, name: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(base).map_err(|e| format!("Could not create the recordings folder: {e}"))?;
    let name = sanitize_filename(name);
    // `create_dir` (not `_all`) fails on an existing folder, so a session never reuses one.
    let mut folder = base.join(&name);
    let mut n = 2;
    loop {
        match fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n <= 99 => {
                folder = base.join(format!("{name} ({n})"));
                n += 1;
            }
            Err(e) => return Err(format!("Could not create the session folder: {e}")),
        }
    }
}

/// Recycles a session folder that is named `Practice …` and holds only `you.wav`. Missing folder is `Ok`.
pub fn delete_session_folder(folder_path: &str, base: &Path, remove: Remover) -> Result<(), String> {
    if !Path::new(folder_path).exists() {
        return Ok(());
    }
    let folder = guarded_meeting_folder(folder_path, base)
        .ok_or_else(|| format!("Session folder kept: it is outside the recordings folder; nothing was deleted ({folder_path})"))?;
    if !folder.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("Practice ")) {
        return Err(format!("Session folder kept: its name is not a Practice folder; nothing was deleted ({})", folder.display()));
    }
    let only_practice_files = fs::read_dir(&folder).is_ok_and(|mut rd| {
        rd.all(|e| e.is_ok_and(|e| PRACTICE_FILES.contains(&e.file_name().to_string_lossy().as_ref()) && e.file_type().is_ok_and(|t| t.is_file())))
    });
    if !only_practice_files {
        return Err(format!("Session folder kept: it contains other files; nothing was deleted ({})", folder.display()));
    }
    // Re-check right before the Recycle Bin call: the folder may have been swapped for a link.
    ensure_still_guarded(&folder, base, "Session folder")?;
    remove(std::slice::from_ref(&folder))
}

/// Lets the asset protocol read this session's `you.wav`; returns its path, or `None` if absent.
pub fn allow_audio<R: Runtime>(app: &AppHandle<R>, folder_path: &str) -> Result<Option<String>, String> {
    let folder = guarded_meeting_folder(folder_path, &get_default_recordings_folder())
        .ok_or("Session folder is outside the recordings folder")?;
    let file = folder.join(PRACTICE_FILES[0]);
    // A link here would let the asset protocol read a file outside the recordings folder.
    if !fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_file()) {
        return Ok(None);
    }
    app.asset_protocol_scope().allow_file(&file).map_err(|e| e.to_string())?;
    Ok(Some(file.to_string_lossy().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::sync::Mutex;

    fn base() -> tempfile::TempDir { tempfile::tempdir().unwrap() }

    /// Stand-in for the Recycle Bin. Removes the whole tree, as the Bin moves it, and records it.
    struct FakeBin(Mutex<Vec<PathBuf>>);
    impl FakeBin {
        fn new() -> Self { FakeBin(Mutex::new(Vec::new())) }
        fn remove(&self, paths: &[PathBuf]) -> Result<(), String> {
            for p in paths {
                if p.is_dir() { fs::remove_dir_all(p).map_err(|e| e.to_string())? } else { fs::remove_file(p).map_err(|e| e.to_string())? }
                self.0.lock().unwrap().push(p.clone());
            }
            Ok(())
        }
        fn calls(&self) -> Vec<PathBuf> { self.0.lock().unwrap().clone() }
    }

    fn session(base: &Path, name: &str) -> PathBuf {
        let m = base.join(name);
        fs::create_dir(&m).unwrap();
        fs::write(m.join("you.wav"), "x").unwrap();
        m
    }

    #[test]
    fn folder_is_created_directly_inside_base_with_suffix_when_taken() {
        let b = base();
        let first = create_session_folder(b.path(), "Practice 2026-10-20 18-05 Thesis").unwrap();
        assert_eq!(first.parent().unwrap(), b.path());
        assert_eq!(first.file_name().unwrap(), "Practice 2026-10-20 18-05 Thesis");
        let second = create_session_folder(b.path(), "Practice 2026-10-20 18-05 Thesis").unwrap();
        assert_eq!(second.file_name().unwrap(), "Practice 2026-10-20 18-05 Thesis (2)");
        let third = create_session_folder(b.path(), "Practice 2026-10-20 18-05 Thesis").unwrap();
        assert_eq!(third.file_name().unwrap(), "Practice 2026-10-20 18-05 Thesis (3)");
        assert!(first.is_dir() && second.is_dir() && third.is_dir());
    }

    #[test]
    fn folder_name_is_sanitized_and_at_most_80_chars() {
        let t = chrono::Local.with_ymd_and_hms(2026, 10, 20, 18, 5, 0).unwrap();
        assert_eq!(folder_name(t, "Thesis: defence/2"), "Practice 2026-10-20 18-05 Thesis_ defence_2");
        let long = folder_name(t, &"ж".repeat(200));
        assert!(long.chars().count() <= 80);
        assert!(!long.ends_with(' ') && !long.ends_with('.'));
    }

    #[test]
    fn delete_recycles_only_a_folder_holding_you_wav() {
        let b = base();
        let m = session(b.path(), "Practice 1");
        let bin = FakeBin::new();
        delete_session_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).unwrap();
        assert!(!m.exists());
        assert_eq!(bin.calls(), vec![dunce::canonicalize(b.path()).unwrap().join("Practice 1")]);
    }

    #[test]
    fn a_foreign_file_blocks_delete_and_nothing_is_removed() {
        let b = base();
        let m = session(b.path(), "Practice 1");
        fs::write(m.join("notes.txt"), "mine").unwrap();
        let bin = FakeBin::new();
        let err = delete_session_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).unwrap_err();
        assert!(err.contains("contains other files; nothing was deleted"), "{err}");
        assert!(bin.calls().is_empty());
        assert!(m.join("you.wav").exists() && m.join("notes.txt").exists());
    }

    #[test]
    fn a_folder_not_named_practice_is_refused_even_when_empty() {
        let b = base();
        let m = b.path().join("Notes");
        fs::create_dir(&m).unwrap();
        let bin = FakeBin::new();
        let err = delete_session_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).unwrap_err();
        assert!(err.contains("nothing was deleted"), "{err}");
        assert!(bin.calls().is_empty());
        assert!(m.is_dir());
    }

    #[test]
    fn a_subfolder_blocks_delete() {
        let b = base();
        let m = session(b.path(), "Practice 1");
        fs::create_dir(m.join("take2")).unwrap();
        let bin = FakeBin::new();
        assert!(delete_session_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_err());
        assert!(bin.calls().is_empty());
        assert!(m.join("you.wav").exists() && m.join("take2").is_dir());
    }

    #[test]
    fn a_folder_outside_base_is_refused() {
        let b = base();
        let elsewhere = base();
        let outside = session(elsewhere.path(), "Practice 1");
        let bin = FakeBin::new();
        let err = delete_session_folder(outside.to_str().unwrap(), b.path(), &|p| bin.remove(p)).unwrap_err();
        assert!(err.contains("outside the recordings folder; nothing was deleted"), "{err}");
        assert!(bin.calls().is_empty());
        assert!(outside.join("you.wav").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_session_folder_is_refused() {
        let b = base();
        let target = session(b.path(), "Practice 1");
        let link = b.path().join("Practice link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let elsewhere = base();
        let outside = session(elsewhere.path(), "Practice 2");
        let outside_link = b.path().join("Practice outside");
        std::os::unix::fs::symlink(&outside, &outside_link).unwrap();
        let bin = FakeBin::new();
        assert!(delete_session_folder(link.to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_err());
        assert!(delete_session_folder(outside_link.to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_err());
        assert!(bin.calls().is_empty());
        assert!(target.join("you.wav").exists() && outside.join("you.wav").exists());
    }

    #[test]
    fn missing_folder_is_ok() {
        let b = base();
        let bin = FakeBin::new();
        assert!(delete_session_folder(b.path().join("gone").to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_ok());
        assert!(bin.calls().is_empty());
    }
}
