// Speaker model catalogue: pinned URLs, exact sizes and SHA-256, resumable verified download.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy)]
pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

pub const MODELS: [ModelFile; 2] = [
    ModelFile {
        name: "segmentation-3.0.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/9403a6902bb58e3d5ae8c7e77c3422de279db2e0/model.onnx",
        bytes: 5_992_913,
        sha256: "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079",
    },
    ModelFile {
        name: "titanet-large.onnx",
        url: "https://huggingface.co/csukuangfj/speaker-embedding-models/resolve/0743f301363dec56491a490f6d6cbc9d67f9a3bf/nemo_en_titanet_large.onnx",
        bytes: 101_405_493,
        sha256: "d51abcf31717ef28162f26acb9d44dd4127c3d44c9b8624f699f3425daca8e77",
    },
];

static MODELS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// `(len, mtime)` at the time of the last hash, and the result. Re-hash only when either changes.
type Stamp = (u64, Option<SystemTime>);
static VERIFIED: Lazy<Mutex<HashMap<PathBuf, (Stamp, bool)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn models_dir() -> Option<PathBuf> {
    MODELS_DIR.get().cloned()
}

/// Set once at startup. The first call wins.
pub fn set_models_dir(dir: PathBuf) {
    let _ = MODELS_DIR.set(dir);
}

/// Cheap enough for each recording start and save: a metadata read per file, plus a hash only on a cache miss.
pub fn models_ready() -> bool {
    models_dir().is_some_and(|dir| all_verified(&dir, &MODELS))
}

fn all_verified(dir: &Path, models: &[ModelFile]) -> bool {
    models.iter().all(|m| file_verified(&dir.join(m.name), m))
}

fn file_verified(path: &Path, model: &ModelFile) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() != model.bytes {
        return false;
    }
    let stamp = (meta.len(), meta.modified().ok());
    let cached = VERIFIED.lock().unwrap().get(path).copied();
    if let Some((cached_stamp, ok)) = cached {
        if cached_stamp == stamp {
            return ok;
        }
    }
    let ok = sha256_file(path).is_ok_and(|hash| hash == model.sha256);
    VERIFIED
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), (stamp, ok));
    ok
}

/// Downloads every model in `MODELS`. `progress(downloaded, total)` counts bytes across both files.
pub async fn download_models(
    client: &reqwest::Client,
    dir: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let total: u64 = MODELS.iter().map(|m| m.bytes).sum();
    let mut base = 0;
    for model in &MODELS {
        download_one(client, model, dir, |done, _| progress(base + done, total)).await?;
        base += model.bytes;
    }
    Ok(())
}

/// Downloads one model into `dir`, resuming from `<name>.part`. The file is renamed into place only after its hash matches.
pub(crate) async fn download_one(
    client: &reqwest::Client,
    model: &ModelFile,
    dir: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    let final_path = dir.join(model.name);
    let (copy, check_path) = (*model, final_path.clone());
    if tokio::task::spawn_blocking(move || file_verified(&check_path, &copy))
        .await
        .unwrap_or(false)
    {
        progress(model.bytes, model.bytes);
        return Ok(());
    }

    let part = dir.join(format!("{}.part", model.name));
    let mut have = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if have > model.bytes {
        let _ = tokio::fs::remove_file(&part).await;
        have = 0;
    }
    if have < model.bytes {
        progress(have, model.bytes);
        fetch_into(client, model, &part, have, &mut progress).await?;
    }

    let hash_path = part.clone();
    let actual = tokio::task::spawn_blocking(move || sha256_file(&hash_path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Failed to read {}: {e}", model.name))?;
    if actual != model.sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(format!("{} failed its SHA-256 check", model.name));
    }
    tokio::fs::rename(&part, &final_path)
        .await
        .map_err(|e| format!("Failed to save {}: {e}", model.name))?;
    progress(model.bytes, model.bytes);
    Ok(())
}

/// Streams the rest of the file into `part`. On a network error the partial file stays, so the next try resumes.
async fn fetch_into(
    client: &reqwest::Client,
    model: &ModelFile,
    part: &Path,
    have: u64,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut request = client.get(model.url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Failed to start {} download: {e}", model.name))?;
    // reqwest follows redirects. Refuse a final address that is not https, unless it is the pinned URL itself.
    if response.url().scheme() != "https" && response.url().as_str() != model.url {
        return Err(format!("{} redirected to a non-https address", model.name));
    }

    let mut written = match response.status() {
        reqwest::StatusCode::PARTIAL_CONTENT if have > 0 => {
            let expected = format!("bytes {have}-{}/{}", model.bytes - 1, model.bytes);
            let found = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok());
            if found != Some(expected.as_str()) {
                return Err(format!(
                    "{} resume returned Content-Range {:?}, expected {expected}",
                    model.name, found
                ));
            }
            have
        }
        reqwest::StatusCode::OK => 0,
        status => return Err(format!("{} download failed with HTTP {status}", model.name)),
    };

    let file = if written == 0 {
        tokio::fs::File::create(part).await
    } else {
        tokio::fs::OpenOptions::new().append(true).open(part).await
    }
    .map_err(|e| format!("Failed to open {} for writing: {e}", model.name))?;
    let mut writer = tokio::io::BufWriter::with_capacity(1 << 20, file);

    let mut stream = response.bytes_stream();
    loop {
        let chunk = match tokio::time::timeout(Duration::from_secs(30), stream.next()).await {
            Err(_) => return Err(format!("Download of {} stalled for 30 seconds", model.name)),
            Ok(None) => break,
            Ok(Some(chunk)) => {
                chunk.map_err(|e| format!("Download of {} interrupted: {e}", model.name))?
            }
        };
        written += chunk.len() as u64;
        if written > model.bytes {
            drop(writer);
            let _ = tokio::fs::remove_file(part).await;
            return Err(format!(
                "{} is larger than its pinned {} bytes",
                model.name, model.bytes
            ));
        }
        writer
            .write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write {}: {e}", model.name))?;
        progress(written, model.bytes);
    }
    writer
        .flush()
        .await
        .map_err(|e| format!("Failed to write {}: {e}", model.name))?;
    if written != model.bytes {
        return Err(format!(
            "{} download ended at {written} of {} bytes; retry to resume",
            model.name, model.bytes
        ));
    }
    Ok(())
}

/// Removes only the two known model names and their `.part` files.
pub fn delete_models(dir: &Path) -> Result<(), String> {
    for m in &MODELS {
        for path in [dir.join(m.name), dir.join(format!("{}.part", m.name))] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Failed to delete {}: {e}", path.display())),
            }
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn leak(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    fn sha_hex(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    /// A model whose URL points at a local stub serving `served`; `expected` fixes the size and hash.
    fn test_model(base: &str, name: &str, expected: &[u8]) -> ModelFile {
        ModelFile {
            name: leak(name.to_string()),
            url: leak(format!("{base}/{name}")),
            bytes: expected.len() as u64,
            sha256: leak(sha_hex(expected)),
        }
    }

    /// Serves `body` for any GET on 127.0.0.1, honouring `Range: bytes=N-`. Returns the base URL and the Range headers seen.
    async fn serve(body: Vec<u8>) -> (String, Arc<StdMutex<Vec<Option<String>>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let seen_task = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if sock.read(&mut byte).await.unwrap_or(0) == 0 {
                        break;
                    }
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).to_string();
                let range = head
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .map(|l| l["range:".len()..].trim().to_string());
                seen_task.lock().unwrap().push(range.clone());

                let start = range
                    .as_deref()
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.strip_suffix('-'))
                    .and_then(|n| n.parse::<usize>().ok())
                    .filter(|n| *n < body.len());
                let (status, from, extra) = match start {
                    Some(s) => (
                        "206 Partial Content",
                        s,
                        format!(
                            "Content-Range: bytes {s}-{}/{}\r\n",
                            body.len() - 1,
                            body.len()
                        ),
                    ),
                    None => ("200 OK", 0, String::new()),
                };
                let payload = &body[from..];
                let head = format!(
                    "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(payload).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://{addr}"), seen)
    }

    #[tokio::test]
    async fn download_verifies_hash_and_renames_part() {
        let body: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let (base, _) = serve(body.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let model = test_model(&base, "model.onnx", &body);

        download_one(&reqwest::Client::new(), &model, dir.path(), |_, _| {})
            .await
            .unwrap();

        assert_eq!(std::fs::read(dir.path().join("model.onnx")).unwrap(), body);
        assert!(!dir.path().join("model.onnx.part").exists());
    }

    #[tokio::test]
    async fn download_resumes_from_part_with_range() {
        let body: Vec<u8> = (0..1000u32).map(|i| (i % 253) as u8).collect();
        let (base, seen) = serve(body.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("model.onnx.part"), &body[..500]).unwrap();
        let model = test_model(&base, "model.onnx", &body);

        download_one(&reqwest::Client::new(), &model, dir.path(), |_, _| {})
            .await
            .unwrap();

        assert_eq!(*seen.lock().unwrap(), vec![Some("bytes=500-".to_string())]);
        assert_eq!(std::fs::read(dir.path().join("model.onnx")).unwrap(), body);
        assert!(!dir.path().join("model.onnx.part").exists());
    }

    #[tokio::test]
    async fn download_rejects_wrong_hash_and_deletes_part() {
        let expected: Vec<u8> = vec![7u8; 400];
        let served: Vec<u8> = vec![8u8; 400];
        let (base, _) = serve(served).await;
        let dir = tempfile::tempdir().unwrap();
        let model = test_model(&base, "model.onnx", &expected);

        let result = download_one(&reqwest::Client::new(), &model, dir.path(), |_, _| {}).await;

        assert!(result.is_err());
        assert!(!dir.path().join("model.onnx").exists());
        assert!(!dir.path().join("model.onnx.part").exists());
    }

    #[tokio::test]
    async fn download_rejects_oversize_stream() {
        let expected: Vec<u8> = vec![1u8; 100];
        let served: Vec<u8> = vec![1u8; 150];
        let (base, _) = serve(served).await;
        let dir = tempfile::tempdir().unwrap();
        let model = test_model(&base, "model.onnx", &expected);

        let result = download_one(&reqwest::Client::new(), &model, dir.path(), |_, _| {}).await;

        assert!(result.is_err());
        assert!(!dir.path().join("model.onnx.part").exists());
    }

    #[test]
    fn models_ready_false_on_size_or_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let good = b"speaker model bytes".to_vec();
        let model = ModelFile {
            name: "model.onnx",
            url: "https://example.invalid/model.onnx",
            bytes: good.len() as u64,
            sha256: leak(sha_hex(&good)),
        };
        let path = dir.path().join("model.onnx");
        let models = [model];

        assert!(!all_verified(dir.path(), &models), "missing file");
        std::fs::write(&path, b"short").unwrap();
        assert!(!all_verified(dir.path(), &models), "size mismatch");
        std::fs::write(&path, vec![b'x'; good.len()]).unwrap();
        assert!(!all_verified(dir.path(), &models), "hash mismatch");
        std::fs::write(&path, &good).unwrap();
        assert!(all_verified(dir.path(), &models), "exact match");
    }

    #[test]
    fn delete_removes_only_known_files() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "segmentation-3.0.onnx",
            "titanet-large.onnx.part",
            "unrelated.txt",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }

        delete_models(dir.path()).unwrap();

        assert!(!dir.path().join("segmentation-3.0.onnx").exists());
        assert!(!dir.path().join("titanet-large.onnx.part").exists());
        assert!(dir.path().join("unrelated.txt").exists());
    }
}
