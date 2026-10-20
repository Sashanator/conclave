// Supertonic 3 voice and model files, pinned to one revision. Downloaded on first need with the speaker downloader.

use crate::speakers::models::{download_client, download_one, ModelFile};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Flat names in one folder, so `download_one` can write each file as `dir.join(name)`.
pub const FILES: [ModelFile; 16] = [
    ModelFile {
        name: "duration_predictor.onnx",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/onnx/duration_predictor.onnx",
        bytes: 3700147,
        sha256: "c3eb91414d5ff8a7a239b7fe9e34e7e2bf8a8140d8375ffb14718b1c639325db",
    },
    ModelFile {
        name: "text_encoder.onnx",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/onnx/text_encoder.onnx",
        bytes: 36416150,
        sha256: "c7befd5ea8c3119769e8a6c1486c4edc6a3bc8365c67621c881bbb774b9902ff",
    },
    ModelFile {
        name: "vector_estimator.onnx",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/onnx/vector_estimator.onnx",
        bytes: 256534781,
        sha256: "883ac868ea0275ef0e991524dc64f16b3c0376efd7c320af6b53f5b780d7c61c",
    },
    ModelFile {
        name: "vocoder.onnx",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/onnx/vocoder.onnx",
        bytes: 101424195,
        sha256: "085de76dd8e8d5836d6ca66826601f615939218f90e519f70ee8a36ed2a4c4ba",
    },
    ModelFile {
        name: "tts.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/onnx/tts.json",
        bytes: 8253,
        sha256: "42078d3aef1cd43ab43021f3c54f47d2d75ceb4e75f627f118890128b06a0d09",
    },
    ModelFile {
        name: "unicode_indexer.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/onnx/unicode_indexer.json",
        bytes: 277676,
        sha256: "9bf7346e43883a81f8645c81224f786d43c5b57f3641f6e7671a7d6c493cb24f",
    },
    ModelFile {
        name: "F1.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/F1.json",
        bytes: 292046,
        sha256: "bbdec6ee00231c2c742ad05483df5334cab3b52fda3ba38e6a07059c4563dbc2",
    },
    ModelFile {
        name: "F2.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/F2.json",
        bytes: 292423,
        sha256: "7c722c6a72707b1a77f035d67f0d1351ba187738e06f7683e8c72b1df3477fc6",
    },
    ModelFile {
        name: "F3.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/F3.json",
        bytes: 290794,
        sha256: "12f6ef2573baa2defa1128069cb59f203e3ab67c92af77b42df8a0e3a2f7c6ab",
    },
    ModelFile {
        name: "F4.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/F4.json",
        bytes: 291808,
        sha256: "c2fa764c1225a76dfc3e2c73e8aa4f70d9ee48793860eb34c295fff01c2e032b",
    },
    ModelFile {
        name: "F5.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/F5.json",
        bytes: 291479,
        sha256: "45966e73316415626cf41a7d1c6f3b4c70dbc1ba2bee5c1978ef0ce33244fc8d",
    },
    ModelFile {
        name: "M1.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/M1.json",
        bytes: 291748,
        sha256: "e35604687f5d23694b8e91593a93eec0e4eca6c0b02bb8ed69139ab2ea6b0a5b",
    },
    ModelFile {
        name: "M2.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/M2.json",
        bytes: 292055,
        sha256: "b76cbf62bac707c710cf0ae5aba5e31eea1a6339a9734bfae33ab98499534a50",
    },
    ModelFile {
        name: "M3.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/M3.json",
        bytes: 290198,
        sha256: "ea1ac35ccb91b0d7ecad533a2fbd0eec10c91513d8951e3b25fbba99954e159b",
    },
    ModelFile {
        name: "M4.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/M4.json",
        bytes: 291522,
        sha256: "ca8eefad4fcd989c9379032ff3e50738adc547eeb5e221b82593a6d7b3bac303",
    },
    ModelFile {
        name: "M5.json",
        url: "https://huggingface.co/Supertone/supertonic-3/resolve/724fb5abbf5502583fb520898d45929e62f02c0b/voice_styles/M5.json",
        bytes: 291469,
        sha256: "dd22b92740314321f8ae11c5e87f8dd60d060f15dd3a632b5adf77f471f77af2",
    },
];

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Set once at startup. The first call wins.
pub fn set_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

pub fn dir() -> Option<PathBuf> {
    DIR.get().cloned()
}

/// Cheap check for each Practice start: a metadata read per file. The hash is checked by `download_one`.
pub fn voices_present() -> bool {
    dir().is_some_and(|dir| present_in(&dir))
}

fn present_in(dir: &Path) -> bool {
    FILES
        .iter()
        .all(|m| std::fs::metadata(dir.join(m.name)).is_ok_and(|meta| meta.len() == m.bytes))
}

/// Downloads every missing or unverified file in `FILES`, then returns the folder.
/// `progress(downloaded, total)` counts bytes across all 16 files.
pub async fn ensure_voices(mut progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let folder = dir().ok_or("the practice models folder is not set")?;
    let client = download_client();
    let total: u64 = FILES.iter().map(|m| m.bytes).sum();
    let mut base = 0;
    for model in &FILES {
        download_one(&client, model, &folder, |done, _| {
            progress(base + done, total)
        })
        .await?;
        base += model.bytes;
    }
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supertone/supertonic-3 at the commit the URLs above pin. A new revision needs new hashes.
    const REVISION: &str = "724fb5abbf5502583fb520898d45929e62f02c0b";

    #[test]
    fn urls_are_pinned_to_the_revision() {
        for m in &FILES {
            let folder = if m.name.starts_with('F') || m.name.starts_with('M') {
                "voice_styles"
            } else {
                "onnx"
            };
            assert_eq!(
                m.url,
                format!(
                    "https://huggingface.co/Supertone/supertonic-3/resolve/{REVISION}/{folder}/{}",
                    m.name
                )
            );
            assert!(!m.name.contains('/'), "{} must be a flat name", m.name);
        }
        assert_eq!(FILES.iter().filter(|m| m.url.contains("/onnx/")).count(), 6);
        assert_eq!(
            FILES
                .iter()
                .filter(|m| m.url.contains("/voice_styles/"))
                .count(),
            10
        );
    }

    #[test]
    fn voices_present_checks_sizes() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!present_in(dir.path()), "empty folder");
        for m in &FILES {
            // Sparse files: the size is what matters here, and no bytes are written.
            std::fs::File::create(dir.path().join(m.name))
                .unwrap()
                .set_len(m.bytes)
                .unwrap();
        }
        assert!(present_in(dir.path()), "every file at its pinned size");
        std::fs::File::create(dir.path().join(FILES[0].name))
            .unwrap()
            .set_len(FILES[0].bytes - 1)
            .unwrap();
        assert!(!present_in(dir.path()), "one file one byte short");
    }

    #[tokio::test]
    #[ignore] // Live: downloads about 400 MB from huggingface.co. Run with PRACTICE_MODELS_DIR set.
    async fn live_download() {
        let folder = std::env::var("PRACTICE_MODELS_DIR").expect("set PRACTICE_MODELS_DIR");
        set_dir(PathBuf::from(folder));
        ensure_voices(|_, _| {}).await.expect("download the voices");
        assert!(voices_present());
    }
}
