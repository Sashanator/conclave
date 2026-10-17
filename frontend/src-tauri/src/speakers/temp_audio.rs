// The 16 kHz mono decode of a meeting, kept in a temp file so the diarizer can read it in windows.
// Files live in `<app_data>/speakers-tmp/` as `<meeting>-<uuid>.f32` (raw little-endian f32).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::diarize::AudioSource;

/// Owns a temp file path and deletes the file on drop, so a failed decode leaves nothing behind.
pub struct TempFileGuard(PathBuf);

impl TempFileGuard {
    pub fn new_in(dir: &Path, meeting_id: &str) -> Self {
        // The id comes from the database, but keep only characters that are safe in a file name.
        let safe: String = meeting_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        Self(dir.join(format!("{safe}-{}.f32", uuid::Uuid::new_v4())))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A decoded meeting read from its temp file. The file is deleted when this is dropped.
pub struct TempAudio {
    file: File,
    /// Number of f32 samples in the file.
    len: usize,
    /// Never read: held so that dropping `TempAudio` deletes the file.
    #[allow(dead_code)]
    guard: TempFileGuard,
}

impl TempAudio {
    pub fn open(guard: TempFileGuard) -> std::io::Result<Self> {
        let file = File::open(guard.path())?;
        let len = (file.metadata()?.len() / 4) as usize;
        Ok(Self { file, len, guard })
    }
}

impl AudioSource for TempAudio {
    fn len(&self) -> usize {
        self.len
    }

    fn read(&self, start: usize, out: &mut [f32]) -> Result<(), String> {
        let end = start.checked_add(out.len()).filter(|&end| end <= self.len);
        if end.is_none() {
            return Err("read past the end of the audio".into());
        }
        // `&File` implements Read and Seek, so no lock is needed for reads through `&self`.
        let mut bytes = vec![0u8; out.len() * 4];
        (&self.file)
            .seek(SeekFrom::Start(start as u64 * 4))
            .and_then(|_| (&self.file).read_exact(&mut bytes))
            .map_err(|e| format!("failed to read the temp audio: {e}"))?;
        for (dst, c) in out.iter_mut().zip(bytes.chunks_exact(4)) {
            *dst = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        }
        Ok(())
    }
}

/// Startup cleanup: removes leftover `*.f32` files directly in `dir`. Subfolders and links are
/// never followed or removed.
pub fn clear_temp_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_f32 = path.extension().and_then(|e| e.to_str()) == Some("f32");
        // file_type() does not follow links, so a symlink is not a file here.
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if is_f32 && is_file {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_f32(path: &Path, values: &[f32]) {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn temp_audio_reads_ranges_and_deletes_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let guard = TempFileGuard::new_in(dir.path(), "meeting-1");
        let path = guard.path().to_path_buf();
        let values: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        write_f32(&path, &values);

        let audio = TempAudio::open(guard).unwrap();
        assert_eq!(audio.len(), 1000);

        let mut out = [0.0f32; 10];
        audio.read(100, &mut out).unwrap();
        assert_eq!(out[0], 100.0);
        assert_eq!(out[9], 109.0);

        audio.read(990, &mut out).unwrap();
        assert_eq!(out[9], 999.0);
        assert!(audio.read(995, &mut out).is_err(), "a read past the end must fail");
        assert!(audio.read(usize::MAX, &mut out).is_err());

        drop(audio);
        assert!(!path.exists(), "dropping TempAudio deletes the file");
    }

    #[test]
    fn temp_guard_deletes_partial_file_on_decode_error() {
        // A truncated m4a cannot be probed, so the decode fails before any output exists.
        // The partial file is simulated here, because the guard's job is to remove it.
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/he_aac_48k_5s.m4a");
        let bytes = std::fs::read(&fixture).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("truncated.m4a");
        std::fs::write(&src, &bytes[..bytes.len() / 2]).unwrap();

        let guard = TempFileGuard::new_in(dir.path(), "meeting-2");
        let path = guard.path().to_path_buf();
        std::fs::write(&path, [0u8; 16]).unwrap();

        let result = crate::audio::decoder::decode_to_mono_16k_file(&src, &path, &mut |_| {});
        assert!(result.is_err());
        drop(guard);
        assert!(!path.exists(), "the guard removes the partial file");
    }

    #[test]
    fn clear_temp_dir_removes_only_f32_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.f32"), [0u8; 4]).unwrap();
        std::fs::write(root.join("b.wav"), [0u8; 4]).unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("c.f32"), [0u8; 4]).unwrap();

        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("target.bin");
        std::fs::write(&target, [0u8; 4]).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, root.join("link.f32")).unwrap();

        clear_temp_dir(root);

        assert!(!root.join("a.f32").exists());
        assert!(root.join("b.wav").exists());
        assert!(root.join("sub").join("c.f32").exists(), "subfolders are not entered");
        assert!(target.exists(), "links are not followed");
        #[cfg(unix)]
        assert!(
            std::fs::symlink_metadata(root.join("link.f32")).is_ok(),
            "links are not removed either"
        );
    }

    #[test]
    fn clear_temp_dir_ignores_a_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        clear_temp_dir(&dir.path().join("missing"));
    }
}
