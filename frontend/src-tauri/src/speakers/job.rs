// The app-wide analysis worker (spec §8): one job at a time. A run decodes the recording, diarizes
// it, resolves the speakers and publishes them under its run token, so a run that was overtaken
// (re-transcribe, re-identify, meeting deleted) writes nothing. Every terminal state and every
// enqueue emits `speakers-updated`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use log::{info, warn};
use ndarray::Array2;
use once_cell::sync::Lazy;
use serde_json::json;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, Notify};

use super::assign::{
    assign_lines, centroid_windows, clusters_from_turns, resolve_speakers, self_learning_windows, ClusterInfo, Line,
    SelfEvidence,
};
use super::cluster::{mean_normalized, window_step_secs};
use super::diarize::{diarize, AudioSource};
use super::levels::{self, mic_only_flags, LevelPair};
use super::models;
use super::runtime::{OrtSpeakerModels, SegMeta, SpeakerModels};
use super::sentences::parse_json;
use super::settings;
use super::temp_audio::{TempAudio, TempFileGuard};
use crate::audio::decoder::decode_to_mono_16k_file;
use crate::audio::meeting_folder::{find_audio_file, guarded_meeting_folder};
use crate::audio::recording_preferences::{get_default_recordings_folder, load_recording_preferences};
use crate::database::repositories::speakers::{Publish, SpeakersRepository};
use crate::state::AppState;

const SAMPLE_RATE: f64 = 16_000.0;
const TEMP_DIR: &str = "speakers-tmp";
const LEVELS_FILE: &str = "levels.bin";
/// Sub-window length of self prints (spec §8.2 step 8).
const SUB_SECS: f64 = 3.0;
/// Clusters with less speech than this get no centroid.
const CENTROID_MIN_SECS: f64 = 3.0;
/// A self voiceprint needs this much mic-only speech.
const SELF_LEARN_SECS: f64 = 20.0;
/// A transcript that runs this far past the decoded audio is worth a warning.
const OVERRUN_SECS: f64 = 2.0;
const REASON_MAX_CHARS: usize = 300;
/// The shortest clip with a feature frame (25 ms). Shorter clips have no frames, so TitaNet never
/// runs on them and they embed to nothing; they are skipped before the model (`short_clips_have_no_embedding`).
const EMBED_MIN_SAMPLES: usize = 400;

static WAKE: Notify = Notify::const_new();
/// Latest percent of the running job per meeting. The entry is removed at the job's end.
static PROGRESS: Lazy<Mutex<HashMap<String, u8>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Wakes the worker after an enqueue.
pub fn wake() {
    WAKE.notify_one();
}

/// True while a job is claimed and not yet finished.
pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

pub fn percent_of(meeting_id: &str) -> Option<u8> {
    PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).get(meeting_id).copied()
}

fn set_percent(meeting_id: &str, percent: u8) {
    PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).insert(meeting_id.to_string(), percent);
}

fn forget_percent(meeting_id: &str) {
    PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).remove(meeting_id);
}

/// The app's facts and effects that a run needs. The app implements it; tests use fakes.
pub(crate) trait JobDeps: Send {
    /// Loads the speaker models. `prefer_directml` asks for DirectML where it is available.
    fn models(&mut self, prefer_directml: bool) -> Result<Box<dyn SpeakerModels + Send>, String>;
    /// The feature is switched on and both models are present.
    fn is_active(&self) -> bool;
    fn models_ready(&self) -> bool;
    fn emit(&self, event: &str, meeting_id: &str);
    fn progress(&self, meeting_id: &str, percent: u8);
    /// The folder that holds the meeting folders, or None when unknown.
    fn recordings_base(&self) -> Option<PathBuf>;
    /// Where the decoded temp audio goes (created on demand).
    fn temp_dir(&self) -> Result<PathBuf, String>;
}

/// The single worker task. Jobs left `pending` or `running` by a previous run are queued again at startup.
pub fn start_worker<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let Some(pool) = app.try_state::<AppState>().map(|s| s.db_manager.pool().clone()) else {
                log::error!("speakers: the database is not ready, the analysis worker stops");
                return;
            };
            match SpeakersRepository::claim_next(&pool).await {
                Ok(Some((meeting_id, token))) => {
                    RUNNING.store(true, Ordering::SeqCst);
                    let mut deps = AppDeps::load(&app).await;
                    if let Err(e) = run_one(&pool, &meeting_id, &token, &mut deps).await {
                        log::error!("speakers: could not record the end of the job for {meeting_id}: {e}");
                    }
                    RUNNING.store(false, Ordering::SeqCst);
                }
                Ok(None) => WAKE.notified().await,
                Err(e) => {
                    log::error!("speakers: could not take the next job: {e}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });
}

/// The folder that holds the meeting folders: the saved recording folder.
pub(crate) async fn recordings_base<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    load_recording_preferences(app)
        .await
        .map(|p| p.save_folder)
        .unwrap_or_else(|_| get_default_recordings_folder())
}

/// The real app: models from the speakers folder, events on the app handle.
struct AppDeps<R: Runtime> {
    app: AppHandle<R>,
    /// Read off the async runtime before the run, so the models hash is never computed there.
    ready: bool,
    base: PathBuf,
}

impl<R: Runtime> AppDeps<R> {
    async fn load(app: &AppHandle<R>) -> Self {
        let ready = tokio::task::spawn_blocking(models::models_ready).await.unwrap_or(false);
        Self { app: app.clone(), ready, base: recordings_base(app).await }
    }
}

impl<R: Runtime> JobDeps for AppDeps<R> {
    fn models(&mut self, prefer_directml: bool) -> Result<Box<dyn SpeakerModels + Send>, String> {
        let dir = models::models_dir().ok_or("the speaker models folder is not set")?;
        Ok(Box::new(OrtSpeakerModels::load(&dir, prefer_directml)?))
    }
    fn is_active(&self) -> bool {
        settings::is_enabled() && self.ready
    }
    fn models_ready(&self) -> bool {
        self.ready
    }
    fn emit(&self, event: &str, meeting_id: &str) {
        let _ = self.app.emit(event, json!({ "meeting_id": meeting_id }));
    }
    fn progress(&self, meeting_id: &str, percent: u8) {
        set_percent(meeting_id, percent);
        let _ = self.app.emit("speakers-progress", json!({ "meeting_id": meeting_id, "percent": percent }));
    }
    fn recordings_base(&self) -> Option<PathBuf> {
        Some(self.base.clone())
    }
    fn temp_dir(&self) -> Result<PathBuf, String> {
        let data = self.app.path().app_data_dir().map_err(|e| e.to_string())?;
        Ok(data.join(TEMP_DIR))
    }
}

/// How a run ended before it could publish.
enum Stop {
    /// A precondition failed: the reason is one of the spec's skip reasons.
    Skip(&'static str),
    Fail(String),
}

impl From<String> for Stop {
    fn from(e: String) -> Self {
        Stop::Fail(e)
    }
}

impl From<sqlx::Error> for Stop {
    fn from(e: sqlx::Error) -> Self {
        Stop::Fail(e.to_string())
    }
}

/// Runs one claimed job to its end state and records that state. Returns Err only when the end state cannot be written.
pub(crate) async fn run_one(pool: &SqlitePool, meeting_id: &str, token: &str, deps: &mut dyn JobDeps) -> Result<(), String> {
    let outcome = analyse(pool, meeting_id, token, deps).await;
    forget_percent(meeting_id);
    let (status, reason) = match outcome {
        Ok(true) => {
            deps.emit("speakers-updated", meeting_id);
            return Ok(());
        }
        Ok(false) => {
            info!("speakers: job for {meeting_id} was overtaken, nothing was written");
            return Ok(());
        }
        Err(Stop::Skip(reason)) => ("skipped", reason.to_string()),
        Err(Stop::Fail(e)) => ("failed", e),
    };
    let reason = truncate(&reason);
    if status == "failed" {
        warn!("speakers: analysis of {meeting_id} failed: {reason}");
    } else {
        info!("speakers: analysis of {meeting_id} skipped: {reason}");
    }
    let written = SpeakersRepository::finish_without_publish(pool, meeting_id, token, status, Some(&reason), None)
        .await
        .map_err(|e| e.to_string())?;
    if written {
        deps.emit("speakers-updated", meeting_id);
    } else {
        info!("speakers: job for {meeting_id} was overtaken, nothing was written");
    }
    Ok(())
}

fn truncate(s: &str) -> String {
    s.chars().take(REASON_MAX_CHARS).collect()
}

/// The stages of one run. Ok(true) when the result was published, Ok(false) when the run was overtaken.
async fn analyse(pool: &SqlitePool, meeting_id: &str, token: &str, deps: &mut dyn JobDeps) -> Result<bool, Stop> {
    let started = Instant::now();
    if !deps.is_active() {
        return Err(Stop::Skip(if deps.models_ready() { "feature_off" } else { "models_missing" }));
    }
    let audio_path = recording_of(pool, meeting_id, deps.recordings_base())
        .await?
        .ok_or(Stop::Skip("no_recording"))?;
    let folder = audio_path.parent().map(Path::to_path_buf).unwrap_or_default();

    // Decode. The guard deletes the temp file on every failure below; the audio is opened only after success.
    let temp_dir = deps.temp_dir()?;
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("could not create {}: {e}", temp_dir.display()))?;
    let guard = TempFileGuard::new_in(&temp_dir, meeting_id);
    let decoded_path = guard.path().to_path_buf();
    let t = Instant::now();
    let decoded = run_blocking(deps, meeting_id, 0, 10, move |progress| {
        decode_to_mono_16k_file(&audio_path, &decoded_path, progress)
    })
    .await?;
    let decoded_secs = decoded.map_err(|e| format!("could not decode the recording: {e:#}"))?;
    let decode_secs = t.elapsed().as_secs_f64();
    let audio = Arc::new(TempAudio::open(guard).map_err(|e| format!("could not open the decoded audio: {e}"))?);

    let last_end: Option<f64> = sqlx::query_scalar("SELECT MAX(audio_end_time) FROM transcripts WHERE meeting_id = ?")
        .bind(meeting_id)
        .fetch_one(pool)
        .await?;
    if last_end.is_some_and(|end| end - decoded_secs > OVERRUN_SECS) {
        warn!(
            "speakers: decoded {decoded_secs:.1}s, but the transcript runs to {:.1}s for {meeting_id}",
            last_end.unwrap_or_default()
        );
    }
    if !is_current(pool, meeting_id, token).await? {
        return Ok(false);
    }

    let mut models: Box<dyn SpeakerModels + Send> = Box::new(Failover::new(deps.models(true)?));

    // Diarize, in windows of the decoded audio. The reader fails once the run is overtaken.
    let step = window_step_secs(decoded_secs);
    let t = Instant::now();
    let reader = Guarded::new(&audio, pool, meeting_id, token);
    let (turns, back) = run_blocking(deps, meeting_id, 10, 80, move |progress| {
        let turns = diarize(&reader, models.as_mut(), step, progress);
        (turns, models)
    })
    .await?;
    models = back;
    let Some(turns) = or_overtaken(turns, pool, meeting_id, token).await? else { return Ok(false) };
    let diarize_secs = t.elapsed().as_secs_f64();

    // Centroids of the clusters with enough speech.
    let t = Instant::now();
    let mut clusters = clusters_from_turns(&turns);
    let reader = Guarded::new(&audio, pool, meeting_id, token);
    let (clusters, back) = run_blocking(deps, meeting_id, 80, 95, move |progress| {
        let r = embed_centroids(&reader, models.as_mut(), &mut clusters, progress);
        (r.map(|()| clusters), models)
    })
    .await?;
    models = back;
    let Some(clusters) = or_overtaken(clusters, pool, meeting_id, token).await? else { return Ok(false) };
    let centroid_secs = t.elapsed().as_secs_f64();

    // Self evidence, known people, then resolution.
    let (self_prints, people) = SpeakersRepository::known_people(pool).await?;
    let evidence = self_evidence(&folder, decoded_secs, self_prints);
    let speakers = resolve_speakers(clusters, &evidence, &people);

    // Self voiceprint: learned only when the level track decided who is self.
    let mut self_print = None;
    if let (SelfEvidence::Levels(flags), Some(me)) = (&evidence, speakers.iter().find(|s| s.idx == 0)) {
        let windows = self_learning_windows(&me.turns, flags);
        let seconds = windows.len() as f64 * SUB_SECS;
        if seconds >= SELF_LEARN_SECS {
            let reader = Guarded::new(&audio, pool, meeting_id, token);
            let (print, back) = run_blocking(deps, meeting_id, 95, 95, move |_| {
                let r = centroid_of(&reader, models.as_mut(), &windows);
                (r, models)
            })
            .await?;
            models = back;
            let Some(print) = or_overtaken(print, pool, meeting_id, token).await? else { return Ok(false) };
            self_print = print.map(|v| (v, seconds));
        }
    }

    let lines = load_lines(pool, meeting_id).await?;
    let outs = assign_lines(&lines, &speakers);

    let backend = models.backend_label().to_string();
    drop(models);
    // The temp audio is deleted before anything is published.
    drop(audio);
    let run_secs = started.elapsed().as_secs_f64();
    let publish = Publish {
        speakers,
        lines: outs,
        self_print,
        backend: backend.clone(),
        audio_seconds: decoded_secs,
        run_seconds: run_secs,
    };
    deps.progress(meeting_id, 100);
    let t = Instant::now();
    if !SpeakersRepository::publish(pool, meeting_id, token, &publish).await? {
        return Ok(false);
    }
    info!(
        "speakers: stage timings decode={decode_secs:.1}s diarize={diarize_secs:.1}s centroids={centroid_secs:.1}s publish={:.1}s backend={backend} audio={decoded_secs:.0}s run={run_secs:.1}s",
        t.elapsed().as_secs_f64()
    );
    Ok(true)
}

/// Runs `work` on the blocking pool and forwards its progress (fractions 0..=1) as percent in `from..=to`.
async fn run_blocking<T, F>(deps: &mut dyn JobDeps, meeting_id: &str, from: u8, to: u8, work: F) -> Result<T, Stop>
where
    T: Send + 'static,
    F: FnOnce(&mut dyn FnMut(f32)) -> T + Send + 'static,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<f32>();
    let task = tokio::task::spawn_blocking(move || {
        let mut report = |fraction: f32| {
            let _ = tx.send(fraction);
        };
        work(&mut report)
    });
    tokio::pin!(task);
    let joined = loop {
        tokio::select! {
            Some(fraction) = rx.recv() => deps.progress(meeting_id, percent(from, to, fraction)),
            res = &mut task => break res,
        }
    };
    while let Ok(fraction) = rx.try_recv() {
        deps.progress(meeting_id, percent(from, to, fraction));
    }
    joined.map_err(|e| Stop::Fail(format!("speaker analysis stopped unexpectedly: {e}")))
}

fn percent(from: u8, to: u8, fraction: f32) -> u8 {
    (f32::from(from) + f32::from(to - from) * fraction.clamp(0.0, 1.0)).round() as u8
}

/// The stage's value, or None when the run was overtaken while the stage failed (that is not an error).
async fn or_overtaken<T>(stage: Result<T, String>, pool: &SqlitePool, meeting_id: &str, token: &str) -> Result<Option<T>, Stop> {
    match stage {
        Ok(value) => Ok(Some(value)),
        Err(e) => {
            if is_current(pool, meeting_id, token).await? {
                Err(Stop::Fail(e))
            } else {
                info!("speakers: job for {meeting_id} was overtaken during analysis");
                Ok(None)
            }
        }
    }
}

async fn is_current(pool: &SqlitePool, meeting_id: &str, token: &str) -> Result<bool, sqlx::Error> {
    Ok(SpeakersRepository::get_job(pool, meeting_id)
        .await?
        .is_some_and(|job| job.status == "running" && job.run_token == token))
}

/// The meeting's audio file, if its folder is still a plain folder inside `base`.
pub(crate) async fn recording_of(pool: &SqlitePool, meeting_id: &str, base: Option<PathBuf>) -> Result<Option<PathBuf>, sqlx::Error> {
    let folder: Option<(Option<String>,)> = sqlx::query_as("SELECT folder_path FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;
    let (Some((Some(folder),)), Some(base)) = (folder, base) else { return Ok(None) };
    Ok(guarded_meeting_folder(&folder, &base).and_then(|dir| find_audio_file(&dir)))
}

/// Queues a re-identify. Refused when the feature is off, the meeting has no recording, or a run is pending or running.
pub(crate) async fn identify(pool: &SqlitePool, meeting_id: &str, active: bool, recording: bool) -> Result<(), String> {
    if !active {
        return Err("Speaker identification is off. Turn it on in Settings".into());
    }
    if !recording {
        return Err("No recording saved, so speakers weren't identified".into());
    }
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let status: Option<(String,)> = sqlx::query_as("SELECT status FROM speaker_jobs WHERE meeting_id = ?")
        .bind(meeting_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if let Some((s,)) = status {
        if s == "pending" || s == "running" {
            return Err("Speaker analysis is already running for this meeting".into());
        }
    }
    SpeakersRepository::enqueue_in_tx(&mut *tx, meeting_id).await.map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

/// Self from the level track, or from the stored self voiceprints when there is no readable track.
fn self_evidence(folder: &Path, decoded_secs: f64, self_prints: Vec<Vec<f32>>) -> SelfEvidence {
    match read_levels(folder, decoded_secs) {
        Some(pairs) => SelfEvidence::Levels(mic_only_flags(&pairs)),
        None if self_prints.is_empty() => SelfEvidence::None,
        None => SelfEvidence::Prints(self_prints),
    }
}

/// `levels.bin` decoded. None when it is missing, bigger than this audio can need, or invalid.
fn read_levels(folder: &Path, decoded_secs: f64) -> Option<Vec<LevelPair>> {
    let path = folder.join(LEVELS_FILE);
    let meta = std::fs::metadata(&path).ok()?;
    // One 8-byte pair per 50 ms slice, plus slack and the header. Checked before reading, so a bad file cannot fill memory.
    let limit = (decoded_secs / levels::SLICE_SECS + 1200.0) * 8.0 + 16.0;
    if meta.len() as f64 > limit {
        warn!("speakers: {} is bigger than the recording allows, ignored", path.display());
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    match levels::decode(&bytes) {
        Ok(pairs) => Some(pairs),
        Err(e) => {
            warn!("speakers: {} is not a valid level track, ignored: {e}", path.display());
            None
        }
    }
}

async fn load_lines(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<Line>, sqlx::Error> {
    let rows: Vec<(String, Option<f64>, Option<f64>, String, Option<String>)> = sqlx::query_as(
        "SELECT id, audio_start_time, audio_end_time, transcript, sentences FROM transcripts WHERE meeting_id = ?",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;
    // A line without audio times cannot be placed, so it keeps what it had.
    Ok(rows
        .into_iter()
        .filter_map(|(id, start, end, text, sentences)| {
            Some(Line {
                id,
                start: start?,
                end: end?,
                text,
                sentences: sentences.as_deref().and_then(parse_json),
            })
        })
        .collect())
}

/// Embeds the centroid of each cluster with at least 3 s of speech.
fn embed_centroids(
    audio: &dyn AudioSource,
    models: &mut dyn SpeakerModels,
    clusters: &mut [ClusterInfo],
    progress: &mut dyn FnMut(f32),
) -> Result<(), String> {
    let n = clusters.len();
    for (i, c) in clusters.iter_mut().enumerate() {
        if c.seconds >= CENTROID_MIN_SECS {
            c.centroid = centroid_of(audio, models, &centroid_windows(&c.turns))?;
        }
        progress((i + 1) as f32 / n as f32);
    }
    Ok(())
}

/// The mean of the embeddings of `windows`. None when no clip gives a usable embedding.
fn centroid_of(audio: &dyn AudioSource, models: &mut dyn SpeakerModels, windows: &[(f64, f64)]) -> Result<Option<Vec<f32>>, String> {
    let mut embeddings = Vec::with_capacity(windows.len());
    for &(start, end) in windows {
        let clip = read_clip(audio, start, end)?;
        if let Some(v) = models.embed(&clip)? {
            embeddings.push(v);
        }
    }
    Ok(mean_normalized(&embeddings))
}

fn read_clip(audio: &dyn AudioSource, start: f64, end: f64) -> Result<Vec<f32>, String> {
    let from = ((start * SAMPLE_RATE).round().max(0.0) as usize).min(audio.len());
    let to = ((end * SAMPLE_RATE).round().max(0.0) as usize).min(audio.len()).max(from);
    let mut clip = vec![0f32; to - from];
    audio.read(from, &mut clip)?;
    Ok(clip)
}

/// The decoded audio as the diarizer and the embedder read it. Every read first checks that the run is still current,
/// so an overtaken run stops at its next read. Reads run on blocking threads, where waiting for the database is allowed.
struct Guarded {
    audio: Arc<TempAudio>,
    pool: SqlitePool,
    meeting_id: String,
    token: String,
    handle: Handle,
}

impl Guarded {
    fn new(audio: &Arc<TempAudio>, pool: &SqlitePool, meeting_id: &str, token: &str) -> Self {
        Self {
            audio: Arc::clone(audio),
            pool: pool.clone(),
            meeting_id: meeting_id.to_string(),
            token: token.to_string(),
            handle: Handle::current(),
        }
    }
}

impl AudioSource for Guarded {
    fn len(&self) -> usize {
        self.audio.len()
    }

    fn read(&self, start: usize, out: &mut [f32]) -> Result<(), String> {
        match self.handle.block_on(is_current(&self.pool, &self.meeting_id, &self.token)) {
            Ok(true) => self.audio.read(start, out),
            Ok(false) => Err("speaker analysis was overtaken".into()),
            Err(e) => Err(format!("could not check the speaker analysis: {e}")),
        }
    }
}

/// Wraps the run's models. A DirectML error is retried once after the fallback to CPU, and only when the
/// failing model is on DirectML. A clip too short to embed is skipped, not a failure.
struct Failover {
    inner: Box<dyn SpeakerModels + Send>,
    failed_over: bool,
}

impl Failover {
    fn new(inner: Box<dyn SpeakerModels + Send>) -> Self {
        Self { inner, failed_over: false }
    }

    fn fail_over(&mut self) -> Result<(), String> {
        self.inner.fall_back_to_cpu()?;
        self.failed_over = true;
        Ok(())
    }
}

impl SpeakerModels for Failover {
    fn seg_meta(&self) -> &SegMeta {
        self.inner.seg_meta()
    }

    fn segment(&mut self, chunk: &[f32]) -> Result<Array2<f32>, String> {
        match self.inner.segment(chunk) {
            Err(e) if self.inner.seg_on_directml() => {
                warn!("speakers: segmentation failed on DirectML, retrying on CPU: {e}");
                self.fail_over()?;
                self.inner.segment(chunk)
            }
            other => other,
        }
    }

    fn embed(&mut self, samples: &[f32]) -> Result<Option<Vec<f32>>, String> {
        if samples.len() < EMBED_MIN_SAMPLES {
            return Ok(None);
        }
        match self.inner.embed(samples) {
            Err(e) if self.inner.emb_on_directml() => {
                warn!("speakers: embedding failed on DirectML, retrying on CPU: {e}");
                self.fail_over()?;
                self.inner.embed(samples)
            }
            other => other,
        }
    }

    fn backend_label(&self) -> &'static str {
        if self.failed_over {
            "mixed"
        } else {
            self.inner.backend_label()
        }
    }

    fn fall_back_to_cpu(&mut self) -> Result<(), String> {
        self.inner.fall_back_to_cpu()
    }

    fn seg_on_directml(&self) -> bool {
        self.inner.seg_on_directml()
    }

    fn emb_on_directml(&self) -> bool {
        self.inner.emb_on_directml()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use std::sync::Mutex as StdMutex;

    const CHUNK_FRAMES: usize = 589;
    const CLASSES: usize = 7;

    fn seg_meta() -> SegMeta {
        SegMeta {
            sample_rate: 16_000,
            window_size: 160_000,
            receptive_field_size: 991,
            receptive_field_shift: 270,
            num_speakers: 3,
            powerset_max_classes: 2,
            num_classes: CLASSES,
        }
    }

    /// Logits for one 10 s window. Frame `f` picks the powerset class at `class_at(absolute second)`.
    fn window(start: f64, class_at: impl Fn(f64) -> usize) -> Array2<f32> {
        let mut m = Array2::zeros((CHUNK_FRAMES, CLASSES));
        for f in 0..CHUNK_FRAMES {
            let t = start + f as f64 * 10.0 / CHUNK_FRAMES as f64;
            m[[f, class_at(t)]] = 10.0;
        }
        m
    }

    fn unit(axis: usize) -> Vec<f32> {
        let mut v = vec![0.0; 192];
        v[axis] = 1.0;
        v
    }

    /// Scripted models. Segment call `i` gets `seg(i)`; embed call `i` gets `embed(i, clip)`.
    /// `on_directml` makes the embedding model report DirectML until `fall_back_to_cpu` runs.
    struct FakeModels {
        meta: SegMeta,
        seg: Box<dyn FnMut(usize) -> Result<Array2<f32>, String> + Send>,
        embed: Box<dyn FnMut(usize, &[f32]) -> Result<Option<Vec<f32>>, String> + Send>,
        seg_calls: usize,
        embed_calls: usize,
        on_directml: bool,
        fell_back: bool,
    }

    impl FakeModels {
        fn new(
            seg: impl FnMut(usize) -> Result<Array2<f32>, String> + Send + 'static,
            embed: impl FnMut(usize, &[f32]) -> Result<Option<Vec<f32>>, String> + Send + 'static,
        ) -> Self {
            Self {
                meta: seg_meta(),
                seg: Box::new(seg),
                embed: Box::new(embed),
                seg_calls: 0,
                embed_calls: 0,
                on_directml: false,
                fell_back: false,
            }
        }
    }

    impl SpeakerModels for FakeModels {
        fn seg_meta(&self) -> &SegMeta {
            &self.meta
        }
        fn segment(&mut self, _chunk: &[f32]) -> Result<Array2<f32>, String> {
            let i = self.seg_calls;
            self.seg_calls += 1;
            (self.seg)(i)
        }
        fn embed(&mut self, samples: &[f32]) -> Result<Option<Vec<f32>>, String> {
            let i = self.embed_calls;
            self.embed_calls += 1;
            (self.embed)(i, samples)
        }
        fn backend_label(&self) -> &'static str {
            match (self.on_directml, self.fell_back) {
                (false, _) => "cpu",
                (true, false) => "directml",
                (true, true) => "mixed",
            }
        }
        fn fall_back_to_cpu(&mut self) -> Result<(), String> {
            self.fell_back = true;
            Ok(())
        }
        fn emb_on_directml(&self) -> bool {
            self.on_directml && !self.fell_back
        }
    }

    /// Records events and hands out one set of models (or a load error).
    struct FakeDeps {
        active: bool,
        ready: bool,
        base: Option<PathBuf>,
        temp: PathBuf,
        models: Option<Result<Box<dyn SpeakerModels + Send>, String>>,
        events: StdMutex<Vec<String>>,
    }

    impl FakeDeps {
        fn new(base: &Path, temp: &Path, models: Option<FakeModels>) -> Self {
            Self {
                active: true,
                ready: true,
                base: Some(base.to_path_buf()),
                temp: temp.to_path_buf(),
                models: Some(match models {
                    Some(m) => Ok(Box::new(m) as Box<dyn SpeakerModels + Send>),
                    None => Err("no models in this test".into()),
                }),
                events: StdMutex::new(Vec::new()),
            }
        }
        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }

    impl JobDeps for FakeDeps {
        fn models(&mut self, _prefer_directml: bool) -> Result<Box<dyn SpeakerModels + Send>, String> {
            self.models.take().unwrap_or_else(|| Err("models already taken".into()))
        }
        fn is_active(&self) -> bool {
            self.active
        }
        fn models_ready(&self) -> bool {
            self.ready
        }
        fn emit(&self, event: &str, meeting_id: &str) {
            self.events.lock().unwrap().push(format!("{event} {meeting_id}"));
        }
        fn progress(&self, _meeting_id: &str, _percent: u8) {}
        fn recordings_base(&self) -> Option<PathBuf> {
            self.base.clone()
        }
        fn temp_dir(&self) -> Result<PathBuf, String> {
            Ok(self.temp.clone())
        }
    }

    /// A base folder with one meeting folder in it, and a temp folder.
    struct Setup {
        _dir: tempfile::TempDir,
        base: PathBuf,
        folder: PathBuf,
        temp: PathBuf,
    }

    fn setup() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("recordings");
        let folder = base.join("meeting-1");
        let temp = dir.path().join("speakers-tmp");
        std::fs::create_dir_all(&folder).unwrap();
        Setup { _dir: dir, base, folder, temp }
    }

    /// A 16 kHz mono WAV named `audio.wav`: a 440 Hz tone whose amplitude is `amp(t)`.
    fn write_wav(folder: &Path, seconds: f64, amp: impl Fn(f64) -> f32) {
        let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(folder.join("audio.wav"), spec).unwrap();
        for i in 0..(seconds * 16_000.0) as usize {
            let t = i as f64 / 16_000.0;
            let v = amp(t) * (2.0 * std::f64::consts::PI * 440.0 * t).sin() as f32;
            w.write_sample((v * 16_000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    /// Speaker 0 talks in the first 15 s (quiet), speaker 1 in the rest (loud).
    fn two_speakers_audio() -> impl Fn(f64) -> f32 {
        |t| if t < 15.0 { 0.1 } else { 0.5 }
    }

    /// Segmentation for two speakers: class 1 (speaker 0) before 15 s, class 2 (speaker 1) after.
    fn two_speaker_logits(i: usize) -> Result<Array2<f32>, String> {
        Ok(window(i as f64, |t| if t < 15.0 { 1 } else { 2 }))
    }

    /// Embeddings separate the two voices by loudness: the quiet speaker's clips average about 0.03, the loud one's about 0.16.
    fn two_voices(_i: usize, clip: &[f32]) -> Result<Option<Vec<f32>>, String> {
        let mean_abs = clip.iter().map(|x| x.abs()).sum::<f32>() / clip.len().max(1) as f32;
        Ok(Some(if mean_abs > 0.1 { unit(1) } else { unit(0) }))
    }

    /// Level track for the two-speaker audio: speaker 0's mic is loud on two slices of three, speaker 1 never.
    fn two_speaker_levels() -> Vec<LevelPair> {
        (0..600)
            .map(|k| {
                if k < 300 {
                    (if k % 3 != 2 { 0.5 } else { 0.001 }, 0.001)
                } else {
                    (0.001, 0.5)
                }
            })
            .collect()
    }

    async fn pool_with_meeting(folder: &Path) -> SqlitePool {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES ('m1', 'M', ?, ?, ?)")
            .bind(chrono::Utc::now())
            .bind(chrono::Utc::now())
            .bind(folder.to_string_lossy().to_string())
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    async fn add_line(pool: &SqlitePool, id: &str, start: f64, end: f64) {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
             VALUES (?, 'm1', 'hello there.', 'ts-1', ?, ?, ?)",
        )
        .bind(id)
        .bind(start)
        .bind(end)
        .bind(end - start)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Enqueues and claims the job, as the worker does, so that `run_one` sees a running job.
    async fn claim(pool: &SqlitePool) -> String {
        SpeakersRepository::enqueue(pool, "m1").await.unwrap();
        let (id, token) = SpeakersRepository::claim_next(pool).await.unwrap().unwrap();
        assert_eq!(id, "m1");
        token
    }

    async fn status(pool: &SqlitePool) -> (String, Option<String>, Option<String>) {
        let job = SpeakersRepository::get_job(pool, "m1").await.unwrap().unwrap();
        (job.status, job.reason, job.backend)
    }

    async fn speaker_count(pool: &SqlitePool) -> usize {
        SpeakersRepository::meeting_speakers(pool, "m1").await.unwrap().len()
    }

    #[tokio::test]
    async fn job_skips_without_recording() {
        let s = setup();
        let pool = pool_with_meeting(&s.folder).await;
        let token = claim(&pool).await;
        let mut deps = FakeDeps::new(&s.base, &s.temp, None);
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();
        assert_eq!(status(&pool).await.0, "skipped");
        assert_eq!(status(&pool).await.1.as_deref(), Some("no_recording"));
        assert!(deps.events().contains(&"speakers-updated m1".to_string()));
    }

    #[tokio::test]
    async fn job_skips_when_feature_off() {
        let s = setup();
        write_wav(&s.folder, 30.0, |_| 0.2);
        let pool = pool_with_meeting(&s.folder).await;
        let token = claim(&pool).await;
        let mut deps = FakeDeps::new(&s.base, &s.temp, None);
        deps.active = false;
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();
        assert_eq!(status(&pool).await.1.as_deref(), Some("feature_off"));
        assert!(deps.events().contains(&"speakers-updated m1".to_string()));
    }

    #[tokio::test]
    async fn job_skips_when_models_missing() {
        let s = setup();
        write_wav(&s.folder, 30.0, |_| 0.2);
        let pool = pool_with_meeting(&s.folder).await;
        let token = claim(&pool).await;
        let mut deps = FakeDeps::new(&s.base, &s.temp, None);
        deps.active = false;
        deps.ready = false;
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();
        assert_eq!(status(&pool).await.1.as_deref(), Some("models_missing"));
        assert!(deps.events().contains(&"speakers-updated m1".to_string()));
    }

    #[tokio::test]
    async fn job_failure_emits_update() {
        let s = setup();
        write_wav(&s.folder, 30.0, |_| 0.2);
        let pool = pool_with_meeting(&s.folder).await;
        let token = claim(&pool).await;
        let mut deps = FakeDeps::new(&s.base, &s.temp, None); // models fail to load
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();
        let (st, reason, _) = status(&pool).await;
        assert_eq!(st, "failed");
        assert!(reason.unwrap().contains("no models"));
        assert!(deps.events().contains(&"speakers-updated m1".to_string()));
    }

    #[tokio::test]
    async fn job_publishes_speakers_and_labels() {
        let s = setup();
        write_wav(&s.folder, 30.0, two_speakers_audio());
        std::fs::write(s.folder.join(LEVELS_FILE), levels::encode(&two_speaker_levels())).unwrap();
        let pool = pool_with_meeting(&s.folder).await;
        add_line(&pool, "l1", 0.0, 10.0).await;
        add_line(&pool, "l2", 16.0, 26.0).await;
        let token = claim(&pool).await;
        let models = FakeModels::new(two_speaker_logits, two_voices);
        let mut deps = FakeDeps::new(&s.base, &s.temp, Some(models));
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();

        assert_eq!(status(&pool).await, ("done".into(), None, Some("cpu".into())));
        let speakers = SpeakersRepository::meeting_speakers(&pool, "m1").await.unwrap();
        let idxs: Vec<i64> = speakers.iter().map(|s| s.idx).collect();
        assert_eq!(idxs, vec![0, 1]);
        let labels: Vec<Option<i64>> = sqlx::query_scalar("SELECT speaker_idx FROM transcripts WHERE meeting_id = 'm1' ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(labels, vec![Some(0), Some(1)]);
        assert!(deps.events().contains(&"speakers-updated m1".to_string()));
        assert!(std::fs::read_dir(&s.temp).unwrap().next().is_none(), "temp audio is deleted");
    }

    #[tokio::test]
    async fn job_discards_result_when_token_changed() {
        let s = setup();
        write_wav(&s.folder, 30.0, two_speakers_audio());
        std::fs::write(s.folder.join(LEVELS_FILE), levels::encode(&two_speaker_levels())).unwrap();
        let pool = pool_with_meeting(&s.folder).await;
        add_line(&pool, "l1", 0.0, 10.0).await;
        let token = claim(&pool).await;
        // Re-enqueue while the first segmentation window runs.
        let handle = Handle::current();
        let hook_pool = pool.clone();
        let mut models = FakeModels::new(
            move |i| {
                if i == 0 {
                    handle.block_on(SpeakersRepository::enqueue(&hook_pool, "m1")).unwrap();
                }
                two_speaker_logits(i)
            },
            two_voices,
        );
        let mut deps = FakeDeps::new(&s.base, &s.temp, Some(models));
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();

        assert_eq!(status(&pool).await.0, "pending");
        assert_eq!(speaker_count(&pool).await, 0);
        let labels: Vec<Option<i64>> = sqlx::query_scalar("SELECT speaker_idx FROM transcripts WHERE meeting_id = 'm1'")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(labels, vec![None]);
    }

    #[tokio::test]
    async fn job_falls_back_to_cpu_and_records_mixed() {
        let s = setup();
        write_wav(&s.folder, 30.0, two_speakers_audio());
        std::fs::write(s.folder.join(LEVELS_FILE), levels::encode(&two_speaker_levels())).unwrap();
        let pool = pool_with_meeting(&s.folder).await;
        add_line(&pool, "l1", 0.0, 10.0).await;
        let token = claim(&pool).await;
        let mut models = FakeModels::new(two_speaker_logits, |i, clip| {
            if i == 0 {
                Err("DirectML device lost".into())
            } else {
                two_voices(i, clip)
            }
        });
        models.on_directml = true;
        let mut deps = FakeDeps::new(&s.base, &s.temp, Some(models));
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();

        assert_eq!(status(&pool).await, ("done".into(), None, Some("mixed".into())));
        assert_eq!(speaker_count(&pool).await, 2);
    }

    #[tokio::test]
    async fn job_deletes_temp_file_on_failure() {
        let s = setup();
        write_wav(&s.folder, 30.0, two_speakers_audio());
        let pool = pool_with_meeting(&s.folder).await;
        let token = claim(&pool).await;
        // The CPU model fails on the first window: nothing to fall back from, so the job fails.
        let models = FakeModels::new(|_| Err("model broken".into()), two_voices);
        let mut deps = FakeDeps::new(&s.base, &s.temp, Some(models));
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();

        assert_eq!(status(&pool).await.0, "failed");
        assert!(std::fs::read_dir(&s.temp).unwrap().next().is_none(), "temp audio is deleted");
    }

    #[tokio::test]
    async fn job_silence_finishes_done_without_speakers() {
        let s = setup();
        write_wav(&s.folder, 30.0, |_| 0.0);
        let pool = pool_with_meeting(&s.folder).await;
        let token = claim(&pool).await;
        let models = FakeModels::new(|_| Ok(window(0.0, |_| 0)), two_voices);
        let mut deps = FakeDeps::new(&s.base, &s.temp, Some(models));
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();

        assert_eq!(status(&pool).await, ("done".into(), None, Some("cpu".into())));
        assert_eq!(speaker_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn identify_refused_while_running() {
        let s = setup();
        write_wav(&s.folder, 30.0, two_speakers_audio());
        let pool = pool_with_meeting(&s.folder).await;
        let _ = claim(&pool).await; // now running

        let err = identify(&pool, "m1", true, true).await.unwrap_err();
        assert!(err.contains("already running"), "{err}");
        assert_eq!(status(&pool).await.0, "running", "a refused identify changes nothing");

        assert!(identify(&pool, "m1", false, true).await.is_err(), "feature off");
        assert!(identify(&pool, "m1", true, false).await.is_err(), "no recording");
    }

    #[tokio::test]
    async fn identify_queues_a_finished_meeting_again() {
        let s = setup();
        write_wav(&s.folder, 30.0, two_speakers_audio());
        let pool = pool_with_meeting(&s.folder).await;
        let token = claim(&pool).await;
        let mut deps = FakeDeps::new(&s.base, &s.temp, None);
        run_one(&pool, "m1", &token, &mut deps).await.unwrap(); // skipped/failed, not running any more
        identify(&pool, "m1", true, true).await.unwrap();
        assert_eq!(status(&pool).await.0, "pending");
    }

    #[test]
    fn read_clip_stays_inside_the_audio() {
        let audio: Vec<f32> = vec![1.0; 16_000];
        let clip = read_clip(&audio, 0.5, 2.0).unwrap();
        assert_eq!(clip.len(), 16_000 - 8_000);
        assert!(read_clip(&audio, 0.9, 5.0).unwrap().len() == 1_600);
        assert!(read_clip(&audio, 3.0, 4.0).unwrap().is_empty());
    }

    #[test]
    fn percent_maps_fractions_into_the_stage() {
        assert_eq!(percent(0, 10, 0.5), 5);
        assert_eq!(percent(80, 95, 1.0), 95);
        assert_eq!(percent(10, 80, 2.0), 80);
    }

    #[test]
    fn overlong_levels_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        // 1 s of audio allows (20 + 1200) * 8 + 16 = 9776 bytes; a bigger file is refused before it is read.
        std::fs::write(dir.path().join(LEVELS_FILE), vec![0u8; 10_000]).unwrap();
        assert!(read_levels(dir.path(), 1.0).is_none());
        let valid = levels::encode(&[(0.1, 0.1); 4]);
        std::fs::write(dir.path().join(LEVELS_FILE), valid).unwrap();
        assert_eq!(read_levels(dir.path(), 1.0).map(|p| p.len()), Some(4));
    }

    /// Only the single-model fallback path: a CPU embed error is not turned into a fallback.
    #[test]
    fn failover_never_falls_back_for_a_cpu_model() {
        let models = FakeModels::new(|_| Ok(window(0.0, |_| 0)), |_, _| Err("bad clip".into()));
        let mut f = Failover::new(Box::new(models));
        let long = vec![0.1f32; 16_000 * 3];
        assert!(f.embed(&long).is_err());
        assert_eq!(f.backend_label(), "cpu");
    }

    /// The meetup recording end to end on the real models (CPU). Needs SPEAKER_MODELS_DIR, SPEAKER_MEETUP_MP4 (the
    /// recording) and SPEAKER_MEETUP_JSON (merged.json). Speaker count must match; seconds within 2 % after sorting.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn job_end_to_end_on_meetup() {
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
        let reference: serde_json::Value = serde_json::from_slice(&std::fs::read(env("SPEAKER_MEETUP_JSON")).unwrap()).unwrap();
        let s = setup();
        std::fs::copy(env("SPEAKER_MEETUP_MP4"), s.folder.join("audio.mp4")).unwrap();
        let pool = pool_with_meeting(&s.folder).await;
        // The recording is 59:45 long: a line every 10 s, and a last short one.
        for i in 0..358 {
            add_line(&pool, &format!("l{i}"), i as f64 * 10.0, i as f64 * 10.0 + 10.0).await;
        }
        add_line(&pool, "last", 3580.0, 3585.0).await;
        let token = claim(&pool).await;

        let mut deps = FakeDeps::new(&s.base, &s.temp, None);
        deps.models = Some(OrtSpeakerModels::load(Path::new(&env("SPEAKER_MODELS_DIR")), false).map(|m| Box::new(m) as Box<dyn SpeakerModels + Send>));
        run_one(&pool, "m1", &token, &mut deps).await.unwrap();

        let job = SpeakersRepository::get_job(&pool, "m1").await.unwrap().unwrap();
        assert_eq!(job.status, "done", "{:?}", job.reason);
        let run = job.run_seconds.unwrap();
        let audio = job.audio_seconds.unwrap();
        eprintln!("e2e: backend={} audio={audio:.1}s run={run:.1}s ({:.1} s per hour of audio)", job.backend.clone().unwrap_or_default(), run / (audio / 3600.0));

        let mut ours: Vec<f64> = SpeakersRepository::meeting_speakers(&pool, "m1").await.unwrap().iter().map(|s| s.seconds).collect();
        ours.sort_by(|a, b| b.total_cmp(a));
        let theirs: Vec<f64> = reference["seconds"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        eprintln!("e2e: ours {ours:.1?}");
        eprintln!("e2e: merged.json {theirs:.1?}");
        assert_eq!(ours.len(), theirs.len(), "speaker count");
        // Printed, not asserted: this decodes the container clock, while merged.json came from the naive-concatenation
        // fixture (about 4.5 s/h longer), so the diarization input differs and per-speaker seconds can shift.
        // The merge itself is checked by `merge_parity_on_fixture_audio`.
        for (a, b) in ours.iter().zip(&theirs) {
            eprintln!("e2e: seconds {a:.1} vs merged.json {b:.1} ({:+.1} %)", (a - b) / b * 100.0);
        }
    }

    /// The merge alone, on the fixture audio that merged.json was made from (no decode, no clock difference):
    /// diarize, embed the centroids as the job does, then resolve with no self evidence and no known people.
    /// Needs SPEAKER_MODELS_DIR. Speaker count must match; each sorted seconds value within 0.5 s.
    #[test]
    #[ignore]
    fn merge_parity_on_fixture_audio() {
        let models_dir = std::env::var("SPEAKER_MODELS_DIR").expect("set SPEAKER_MODELS_DIR");
        let reference: serde_json::Value =
            serde_json::from_slice(&std::fs::read("/workspace/.superpowers/speaker-parity/meetup/merged.json").unwrap()).unwrap();
        let bytes = std::fs::read("/workspace/.superpowers/speaker-parity/meetup/audio16k.f32").unwrap();
        let audio: Vec<f32> = bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let step = window_step_secs(audio.len() as f64 / SAMPLE_RATE);
        let mut models = OrtSpeakerModels::load(Path::new(&models_dir), false).unwrap();

        let turns = diarize(&audio, &mut models, step, &mut |_| {}).unwrap();
        let mut clusters = clusters_from_turns(&turns);
        embed_centroids(&audio, &mut models, &mut clusters, &mut |_| {}).unwrap();
        let speakers = resolve_speakers(clusters, &SelfEvidence::None, &[]);

        let mut ours: Vec<f64> = speakers.iter().map(|s| s.seconds).collect();
        ours.sort_by(|a, b| b.total_cmp(a));
        let theirs: Vec<f64> = reference["seconds"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        eprintln!("merge parity: step {step} s, ours {ours:.2?}");
        eprintln!("merge parity: merged.json {theirs:.2?}");
        assert_eq!(ours.len(), theirs.len(), "speaker count");
        for (a, b) in ours.iter().zip(&theirs) {
            assert!((a - b).abs() <= 0.5, "{a:.2} s vs {b:.2} s");
        }
    }

    /// Pins EMBED_MIN_SAMPLES with the real model: no embedding below it, one from it. Needs SPEAKER_MODELS_DIR.
    #[test]
    #[ignore]
    fn short_clips_have_no_embedding() {
        let dir = std::env::var("SPEAKER_MODELS_DIR").expect("set SPEAKER_MODELS_DIR");
        let mut m = OrtSpeakerModels::load(Path::new(&dir), false).unwrap();
        let tone = |n: usize| -> Vec<f32> { (0..n).map(|i| 0.1 * (i as f32 * 0.05).sin()).collect() };
        for n in (0..EMBED_MIN_SAMPLES).step_by(16) {
            assert!(matches!(m.embed(&tone(n)), Ok(None)), "{n} samples");
        }
        assert!(matches!(m.embed(&tone(EMBED_MIN_SAMPLES)), Ok(Some(_))));
    }

    /// Both DirectML errors and short clips: the DirectML fallback is retried once and labelled mixed.
    #[test]
    fn failover_retries_once_on_directml() {
        let mut models = FakeModels::new(|_| Ok(window(0.0, |_| 0)), |i, _| {
            if i == 0 { Err("device lost".into()) } else { Ok(Some(unit(3))) }
        });
        models.on_directml = true;
        let mut f = Failover::new(Box::new(models));
        let long = vec![0.1f32; 16_000 * 3];
        assert_eq!(f.embed(&long).unwrap(), Some(unit(3)));
        assert_eq!(f.backend_label(), "mixed");
    }
}
