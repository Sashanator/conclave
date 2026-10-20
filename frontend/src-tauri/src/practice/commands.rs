//! Speaking Practice commands: setups and materials, the session controls (start, cancel, tap, end),
//! voice downloads, voice preview and startup recovery.

use std::future::Future;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{Local, Utc};
use log::{info, warn};
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::audio::common::acquire_engine_lifecycle_lock;
use crate::audio::devices::parse_audio_device;
use crate::audio::import::is_import_in_progress;
use crate::audio::meeting_folder::{recycle, Remover};
use crate::audio::recording_commands::is_recording_now;
use crate::audio::recording_preferences::{get_default_recordings_folder, load_recording_preferences};
use crate::audio::retranscription::is_retranscription_in_progress;
use crate::audio::transcription::engine::get_or_init_whisper;
use crate::dictate;
use crate::state::AppState;

use super::audio::{Player, TurnMic, TurnRecorder};
use super::claude::{self, ClaudeError};
use super::db::{self, Material, MaterialKind, MaterialMeta, SessionRow, SetupInput, SetupRow, TurnRow};
use super::files::{allow_audio, create_session_folder, delete_session_folder, folder_name, PRACTICE_FILES};
use super::models;
use super::prompts::plan_prompts;
use super::report::Report;
use super::report_job::{self, summary_line, REPORT_GATE};
use super::runner::{self, Loaded};
use super::session::{Event, Notice, Phase};
use super::tts::{load_voice, Supertonic, Voice};
use super::{hold_model, ModelHold, TTS_RATE, TTS_SPEED, TTS_STEPS};

const NO_KEY: &str = "Add your Claude API key in Settings to use Practice";
pub(super) const NOT_WHISPER: &str = "Practice needs Whisper as the transcription engine (Settings → Transcription)";
const BUSY: &str = "Practice is unavailable while a meeting is recording, importing or re-transcribing";
const RUNNING: &str = "A practice session is already running";
const NOT_RETRYABLE: &str = "This report cannot be retried now";
const DELETE_BUSY: &str = "This session or its report is still running — delete it once it finishes";
const UNREADABLE_REPORT: &str = "The saved report could not be read — Retry";
const CANCELLED: &str = "Cancelled";
const PREVIEW_TEXT: &str = "Hello, I'm looking forward to our conversation.";

#[derive(Serialize)]
pub struct PracticeStatus {
    pub has_claude_key: bool,
    pub engine_is_whisper: bool,
    pub voices_ready: bool,
    pub blocked: Option<String>,
    pub running_session: Option<String>,
}

#[derive(Serialize)]
pub struct SetupSummary {
    pub id: String,
    pub name: String,
    pub character: String,
    pub situation: String,
    pub voice: String,
    pub seeded: bool,
    pub has_plan: bool,
    pub materials: Vec<MaterialMeta>,
    pub session_count: i64,
}

#[derive(Clone, Serialize)]
pub struct PracticeStatePayload {
    pub session_id: String,
    pub phase: Phase,
    pub character: String,
    pub elapsed_secs: u64,
    pub caption: Option<String>,
    pub notice: Option<Notice>,
}

#[derive(Clone, Serialize)]
pub struct PracticeTurnPayload {
    pub session_id: String,
    pub idx: i64,
    pub speaker: &'static str,
    pub text: String,
}

#[derive(Clone, Serialize)]
pub struct PracticePrepPayload {
    pub stage: &'static str,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct VoicesProgressPayload {
    pub percent: u64,
}

/// The one session slot. A start claims it before any loading, so two starts cannot both load.
pub(super) enum Slot {
    Idle,
    Starting(CancellationToken),
    Running { id: String, tx: UnboundedSender<Event> },
}

static SLOT: Mutex<Slot> = Mutex::new(Slot::Idle);

fn slot() -> MutexGuard<'static, Slot> {
    SLOT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Frees the slot when a session ends. A session that is not the one running is left alone.
pub(super) fn release_slot(session_id: &str) {
    let mut s = slot();
    if matches!(&*s, Slot::Running { id, .. } if id == session_id) {
        *s = Slot::Idle;
    }
}

/// Undoes a claimed start on any early return. A started session has replaced `Starting`, so this is a no-op then.
struct StartGuard;

impl Drop for StartGuard {
    fn drop(&mut self) {
        let mut s = slot();
        if matches!(*s, Slot::Starting(_)) {
            *s = Slot::Idle;
        }
    }
}

fn claim(token: CancellationToken) -> Result<StartGuard, String> {
    let mut s = slot();
    if !matches!(*s, Slot::Idle) {
        return Err(RUNNING.to_string());
    }
    *s = Slot::Starting(token);
    Ok(StartGuard)
}

fn running_id() -> Option<String> {
    match &*slot() {
        Slot::Running { id, .. } => Some(id.clone()),
        _ => None,
    }
}

fn send_event(ev: Event) -> Result<(), String> {
    if let Slot::Running { tx, .. } = &*slot() {
        let _ = tx.send(ev);
    }
    Ok(())
}

fn pool_of(state: &AppState) -> SqlitePool {
    state.db_manager.pool().clone()
}

fn busy_blocker() -> Option<&'static str> {
    (is_recording_now() || is_import_in_progress() || is_retranscription_in_progress()).then_some(BUSY)
}

/// The copy for a start that must be refused, or `None` when a start may proceed.
pub fn start_blocker(status: &PracticeStatus) -> Option<&'static str> {
    if status.running_session.is_some() {
        Some(RUNNING)
    } else if status.blocked.is_some() {
        Some(BUSY)
    } else if !status.has_claude_key {
        Some(NO_KEY)
    } else if !status.engine_is_whisper {
        Some(NOT_WHISPER)
    } else {
        None
    }
}

/// A plan is rebuilt when the setup has materials and the plan was made from different ones or another situation.
pub fn needs_plan(setup: &SetupRow, materials: &[Material]) -> bool {
    if materials.is_empty() {
        return false;
    }
    let fingerprint = db::plan_fingerprint(&setup.situation, materials);
    setup.plan_fingerprint.as_deref() != Some(fingerprint.as_str())
}

/// Rejects material bytes that do not match their kind: a PDF must start with its magic, text must be UTF-8.
pub fn check_material(kind: MaterialKind, bytes: &[u8]) -> Result<(), String> {
    match kind {
        MaterialKind::Pdf if !bytes.starts_with(b"%PDF-") => Err("That file is not a PDF".to_string()),
        MaterialKind::Text => std::str::from_utf8(bytes)
            .map(|_| ())
            .map_err(|_| "Text material must be UTF-8".to_string()),
        MaterialKind::Pdf => Ok(()),
    }
}

/// Decodes a `encodeURIComponent` value, as sent in the `x-name` header.
pub fn decode_uri_component(s: &str) -> Result<String, String> {
    const BAD: &str = "Bad name encoding";
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or(BAD)?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| BAD.to_string())
}

fn header(request: &Request<'_>, name: &str) -> Result<String, String> {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| format!("Missing {name}"))
}

async fn status_of<R: Runtime>(app: &AppHandle<R>, pool: &SqlitePool) -> PracticeStatus {
    PracticeStatus {
        has_claude_key: claude::api_key(pool).await.is_ok(),
        engine_is_whisper: dictate::engine_is_whisper(app).await,
        voices_ready: models::voices_present(),
        blocked: busy_blocker().map(str::to_string),
        running_session: running_id(),
    }
}

#[tauri::command]
pub async fn practice_status<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> Result<PracticeStatus, String> {
    Ok(status_of(&app, &pool_of(&state)).await)
}

async fn summarize(pool: &SqlitePool, row: SetupRow) -> Result<SetupSummary, String> {
    let materials = db::list_materials(pool, &row.id).await?;
    let session_count = db::count_sessions(pool, &row.id).await?;
    Ok(SetupSummary {
        has_plan: row.plan.is_some(),
        id: row.id,
        name: row.name,
        character: row.character,
        situation: row.situation,
        voice: row.voice,
        seeded: row.seeded,
        materials,
        session_count,
    })
}

#[tauri::command]
pub async fn practice_list_setups(state: State<'_, AppState>) -> Result<Vec<SetupSummary>, String> {
    let pool = pool_of(&state);
    let mut out = Vec::new();
    for row in db::list_setups(&pool).await? {
        out.push(summarize(&pool, row).await?);
    }
    Ok(out)
}

#[tauri::command]
pub async fn practice_save_setup(state: State<'_, AppState>, input: SetupInput) -> Result<SetupSummary, String> {
    let pool = pool_of(&state);
    let row = db::save_setup(&pool, input).await?;
    summarize(&pool, row).await
}

#[tauri::command]
pub async fn practice_delete_setup(state: State<'_, AppState>, id: String) -> Result<(), String> {
    db::delete_setup(&pool_of(&state), &id).await
}

async fn add_checked(
    pool: &SqlitePool,
    setup_id: &str,
    kind: MaterialKind,
    name: &str,
    content: Vec<u8>,
) -> Result<MaterialMeta, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Material needs a name".to_string());
    }
    if db::get_setup(pool, setup_id).await?.is_none() {
        return Err("Setup not found".to_string());
    }
    db::add_material(pool, setup_id, kind, name, content).await
}

/// Raw body bytes, with `x-setup-id`, `x-name` (URI-encoded) and `x-kind` (`pdf` or `text`) headers.
#[tauri::command]
pub async fn practice_add_material(state: State<'_, AppState>, request: Request<'_>) -> Result<MaterialMeta, String> {
    let setup_id = header(&request, "x-setup-id")?;
    let name = decode_uri_component(&header(&request, "x-name")?)?;
    let kind = match header(&request, "x-kind")?.as_str() {
        "pdf" => MaterialKind::Pdf,
        "text" => MaterialKind::Text,
        other => return Err(format!("Unknown material kind: {other}")),
    };
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("Material must be sent as raw bytes".to_string());
    };
    check_material(kind, bytes)?;
    let bytes = bytes.clone();
    add_checked(&pool_of(&state), &setup_id, kind, &name, bytes).await
}

#[tauri::command]
pub async fn practice_add_text(
    state: State<'_, AppState>,
    setup_id: String,
    name: String,
    text: String,
) -> Result<MaterialMeta, String> {
    check_material(MaterialKind::Text, text.as_bytes())?;
    add_checked(&pool_of(&state), &setup_id, MaterialKind::Text, &name, text.into_bytes()).await
}

#[tauri::command]
pub async fn practice_delete_material(state: State<'_, AppState>, id: String) -> Result<(), String> {
    db::delete_material(&pool_of(&state), &id).await
}

/// Clears the plan, so the next start builds questions again.
#[tauri::command]
pub async fn practice_new_questions(state: State<'_, AppState>, setup_id: String) -> Result<(), String> {
    db::set_plan(&pool_of(&state), &setup_id, None, None).await
}

#[tauri::command]
pub async fn practice_download_voices<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let mut last = None;
    models::ensure_voices(|done, total| {
        let percent = if total == 0 { 100 } else { done * 100 / total };
        if last != Some(percent) {
            last = Some(percent);
            let _ = app.emit("practice-voices-progress", VoicesProgressPayload { percent });
        }
    })
    .await
    .map(|_| ())
}

/// Plays one sentence in the chosen voice. Refused while a session runs, since the speaker is shared.
#[tauri::command]
pub async fn practice_preview_voice(voice: String) -> Result<(), String> {
    speak(voice, PREVIEW_TEXT.to_string()).await
}

/// Plays the reference pronunciation of one word in the session's voice.
#[tauri::command]
pub async fn practice_speak_word(
    state: State<'_, AppState>,
    session_id: String,
    word: String,
) -> Result<(), String> {
    let word = validate_word(&word)?;
    let (row, _) = db::get_session(&pool_of(&state), &session_id).await?.ok_or("Session not found")?;
    speak(row.voice, word).await
}

fn validate_word(word: &str) -> Result<String, String> {
    let word = word.trim();
    if (1..=40).contains(&word.chars().count()) {
        Ok(word.to_string())
    } else {
        Err("A word must be 1–40 characters".to_string())
    }
}

/// Refused while a session runs. Synthesizes `text` in `voice` and plays it on the default speaker.
async fn speak(voice: String, text: String) -> Result<(), String> {
    if !matches!(*slot(), Slot::Idle) {
        return Err(RUNNING.to_string());
    }
    if !db::VOICES.iter().any(|(code, _)| *code == voice) {
        return Err(format!("Unknown voice: {voice}"));
    }
    let dir = match models::dir() {
        Some(dir) if models::voices_present() => dir,
        _ => return Err("Download the voices first".to_string()),
    };
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut tts = Supertonic::load(&dir, true)?;
        let v = load_voice(&dir.join(format!("{voice}.json")))?;
        let wav = tts.synthesize(&text, &v, TTS_STEPS, TTS_SPEED)?;
        drop(tts);
        let wait = std::time::Duration::from_secs_f64(wav.len() as f64 / f64::from(TTS_RATE) + 5.0);
        let (player, finished) = Player::open()?;
        player.play(1, wav);
        // The output stays open until the audio has played, then closes on this thread.
        std::thread::spawn(move || {
            let _ = finished.recv_timeout(wait);
            drop(player);
        });
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Runs a slow loading step in the background and returns as soon as the start is cancelled. The step
/// still finishes on its own (a model load keeps its lifecycle lock until then), and the caller's undo runs after
/// that lock is released.
async fn cancellable<T: Send + 'static>(
    token: &CancellationToken,
    step: impl Future<Output = Result<T, String>> + Send + 'static,
) -> Result<T, String> {
    let handle = tauri::async_runtime::spawn(step);
    tokio::select! {
        biased;
        _ = token.cancelled() => Err(CANCELLED.to_string()),
        joined = handle => joined.map_err(|e| e.to_string())?,
    }
}

fn check(token: &CancellationToken) -> Result<(), String> {
    if token.is_cancelled() {
        Err(CANCELLED.to_string())
    } else {
        Ok(())
    }
}

fn emit_prep<R: Runtime>(app: &AppHandle<R>, stage: &'static str, error: Option<&str>) {
    let _ = app.emit(
        "practice-prep",
        PracticePrepPayload {
            stage,
            error: error.map(str::to_string),
        },
    );
}

async fn preferred_mic<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let prefs = load_recording_preferences(app).await.ok()?;
    prefs
        .preferred_mic_device
        .as_deref()
        .and_then(|raw| parse_audio_device(raw).ok())
        .map(|d| d.name)
}

/// Writes the plan with Claude and stores it. Failure is reported to the UI with the user-facing copy.
async fn build_plan<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    setup: &SetupRow,
    materials: &[Material],
    token: &CancellationToken,
) -> Result<String, String> {
    emit_prep(app, "plan", None);
    match plan_text(pool, setup, materials, token).await {
        Ok(text) => {
            let fingerprint = db::plan_fingerprint(&setup.situation, materials);
            db::set_plan(pool, &setup.id, Some(&text), Some(&fingerprint)).await?;
            Ok(text)
        }
        Err(ClaudeError::Cancelled) => Err(CANCELLED.to_string()),
        Err(e) => {
            let msg = e.user_message();
            emit_prep(app, "failed", Some(msg));
            Err(msg.to_string())
        }
    }
}

async fn plan_text(
    pool: &SqlitePool,
    setup: &SetupRow,
    materials: &[Material],
    token: &CancellationToken,
) -> Result<String, ClaudeError> {
    let key = claude::api_key(pool).await?;
    let (system, user) = plan_prompts(&setup.character, &setup.situation);
    let body = claude::plan_body(&system, &user, materials);
    claude::stream(&claude::http_client(), &key, &body, token, |_| {}).await
}

/// Loads the engine, voices, microphone, speaker and session folder. On error, what was created is undone
/// here, except the model hold, which the caller releases.
async fn load_session<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    setup: &SetupRow,
    plan: Option<String>,
    token: &CancellationToken,
    hold: ModelHold,
) -> Result<Loaded, String> {
    let key = claude::api_key(pool).await.map_err(|e| e.user_message().to_string())?;
    let app_for_load = app.clone();
    let engine = cancellable(token, async move {
        let _lifecycle = acquire_engine_lifecycle_lock().await;
        get_or_init_whisper(&app_for_load).await
    })
    .await?;
    let dir = cancellable(token, models::ensure_voices(|_, _| {})).await?;
    check(token)?;
    let tts_dir = dir.clone();
    let voice_file = format!("{}.json", setup.voice);
    let (tts, voice) = tokio::task::spawn_blocking(move || -> Result<(Supertonic, Voice), String> {
        Ok((Supertonic::load(&dir, true)?, load_voice(&dir.join(voice_file))?))
    })
    .await
    .map_err(|e| e.to_string())??;
    check(token)?;
    let mic_pref = preferred_mic(app).await;
    let pref = mic_pref.clone();
    let (mic, mic_rx, mic_rate) = tokio::task::spawn_blocking(move || TurnMic::open(pref))
        .await
        .map_err(|e| e.to_string())??;
    check(token)?;
    let (player, finished) = tokio::task::spawn_blocking(Player::open)
        .await
        .map_err(|e| e.to_string())??;
    check(token)?;

    let folder = create_session_folder(&get_default_recordings_folder(), &folder_name(Local::now(), &setup.name))?;
    let recorder = match TurnRecorder::create(&folder.join(PRACTICE_FILES[0])) {
        Ok(recorder) => recorder,
        Err(e) => {
            discard_folder(&folder, &get_default_recordings_folder(), &recycle);
            return Err(e);
        }
    };
    let session = match db::create_session(pool, setup, &folder.to_string_lossy(), plan.as_deref()).await {
        Ok(session) => session,
        Err(e) => {
            drop(recorder);
            discard_folder(&folder, &get_default_recordings_folder(), &recycle);
            return Err(e);
        }
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    Ok(Loaded {
        session_id: session.id,
        setup: setup.clone(),
        plan,
        key,
        pool: pool.clone(),
        hold,
        engine,
        tts,
        voice,
        tts_dir,
        mic,
        mic_rx,
        mic_rate,
        mic_pref,
        player,
        finished,
        recorder,
        seed: Utc::now().timestamp() as u32,
        tx,
        rx,
    })
}

/// Recycles a session folder this start created. Only Practice's own folder holding just `you.wav` moves to the
/// Recycle Bin; anything else is refused and left where it is (R33).
fn discard_folder(folder: &Path, base: &Path, remove: Remover) {
    if let Err(e) = delete_session_folder(&folder.to_string_lossy(), base, remove) {
        warn!("practice: could not discard the session folder: {e}");
    }
}

#[tauri::command]
pub async fn practice_start<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    setup_id: String,
    skip_plan: bool,
) -> Result<String, String> {
    let pool = pool_of(&state);
    let token = CancellationToken::new();
    let _claim = claim(token.clone())?;
    if let Some(msg) = start_blocker(&status_of(&app, &pool).await) {
        return Err(msg.to_string());
    }
    let setup = db::get_setup(&pool, &setup_id).await?.ok_or("Setup not found")?;
    let materials = db::load_materials(&pool, &setup_id).await?;
    let plan = if skip_plan || materials.is_empty() {
        None
    } else if needs_plan(&setup, &materials) {
        Some(build_plan(&app, &pool, &setup, &materials, &token).await?)
    } else {
        setup.plan.clone()
    };
    check(&token)?;

    emit_prep(&app, "loading", None);
    let hold = hold_model();
    let loaded = match load_session(&app, &pool, &setup, plan, &token, hold).await {
        Ok(loaded) => loaded,
        Err(e) => {
            dictate::unload_if_idle().await;
            return Err(e);
        }
    };

    let session_id = loaded.session_id.clone();
    let tx = loaded.tx.clone();
    {
        // The check and the slot write share the lock, so a cancel either ends this session here or finds it Running.
        let mut s = slot();
        let first = if token.is_cancelled() { Event::EndRequested } else { Event::Ready };
        let _ = tx.send(first);
        *s = Slot::Running { id: session_id.clone(), tx };
    }
    tauri::async_runtime::spawn(runner::supervise(app.clone(), loaded));
    info!("practice: session started");
    Ok(session_id)
}

/// Cancels a start in any stage. A session already created is ended instead, and its cleanup runs.
#[tauri::command]
pub fn practice_cancel_start() -> Result<(), String> {
    match &*slot() {
        Slot::Starting(token) => token.cancel(),
        Slot::Running { tx, .. } => {
            let _ = tx.send(Event::EndRequested);
        }
        Slot::Idle => {}
    }
    Ok(())
}

#[tauri::command]
pub fn practice_tap() -> Result<(), String> {
    send_event(Event::Tap)
}

#[tauri::command]
pub fn practice_end() -> Result<(), String> {
    send_event(Event::EndRequested)
}

#[derive(Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub setup_name: String,
    pub started_at: String,
    pub duration_secs: f64,
    pub report_status: String,
    /// e.g. "4 grammar · 31 fillers"; set once the report is ready.
    pub summary: Option<String>,
    /// `None` while the session runs.
    pub end_reason: Option<String>,
}

#[derive(Serialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub character: String,
    pub turns: Vec<TurnRow>,
    pub report: Option<Report>,
    pub report_error: Option<String>,
    pub audio_path: Option<String>,
}

fn summary_of(row: &SessionRow) -> SessionSummary {
    let summary = row
        .report_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Report>(json).ok())
        .map(|report| summary_line(&report));
    SessionSummary {
        id: row.id.clone(),
        setup_name: row.setup_name.clone(),
        started_at: row.started_at.clone(),
        duration_secs: row.duration_secs,
        report_status: row.report_status.clone(),
        summary,
        end_reason: row.end_reason.clone(),
    }
}

#[tauri::command]
pub async fn practice_list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    let rows = db::list_sessions(&pool_of(&state)).await?;
    Ok(rows.iter().map(summary_of).collect())
}

/// Reads the stored report. An unreadable one is dropped and the row is marked failed, so the UI offers Retry.
/// Returns the report and whether it was unreadable. The parse error can quote report content, so it is not logged.
fn read_stored_report(row: &mut SessionRow) -> (Option<Report>, bool) {
    let Some(json) = row.report_json.take() else {
        return (None, false);
    };
    match serde_json::from_str::<Report>(&json) {
        Ok(report) => {
            row.report_json = Some(json);
            (Some(report), false)
        }
        Err(_) => {
            warn!("practice: a saved report could not be read");
            row.report_status = "failed".to_string();
            row.report_error = Some(UNREADABLE_REPORT.to_string());
            (None, true)
        }
    }
}

#[tauri::command]
pub async fn practice_get_session<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
) -> Result<SessionDetail, String> {
    let pool = pool_of(&state);
    let (mut row, turns) = db::get_session(&pool, &id).await?.ok_or("Session not found")?;
    let (report, unreadable) = read_stored_report(&mut row);
    if unreadable {
        // Saved, so the list and Retry agree with this view. A failed write leaves the old status.
        if let Err(e) = db::set_report(&pool, &id, "failed", None, Some(UNREADABLE_REPORT)).await {
            warn!("practice: could not mark the unreadable report failed: {e}");
        }
    }
    let audio_path = match row.folder_path.as_deref() {
        // A missing or unreadable recording leaves the report usable without audio.
        Some(folder) => allow_audio(&app, folder).unwrap_or_else(|e| {
            warn!("practice: no audio for the session: {e}");
            None
        }),
        None => None,
    };
    Ok(SessionDetail {
        summary: summary_of(&row),
        character: row.character.clone(),
        turns,
        report,
        report_error: row.report_error.clone(),
        audio_path,
    })
}

/// Runs the report again for a session whose report failed or was interrupted. Refused while it runs.
#[tauri::command]
pub async fn practice_retry_report<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let (row, _) = db::get_session(&pool_of(&state), &session_id)
        .await?
        .ok_or("Session not found")?;
    if running_id().as_deref() == Some(session_id.as_str()) || row.status == "running" {
        return Err(RUNNING.to_string());
    }
    if !matches!(row.report_status.as_str(), "failed" | "pending") {
        return Err(NOT_RETRYABLE.to_string());
    }
    tauri::async_runtime::spawn(report_job::run(app, session_id, hold_model()));
    Ok(())
}

/// Sends a past session to the Recycle Bin, then removes its rows. Refused while it runs or its report does.
#[tauri::command]
pub async fn practice_delete_session(state: State<'_, AppState>, id: String) -> Result<(), String> {
    delete_session_inner(&pool_of(&state), &id, &get_default_recordings_folder(), &recycle).await
}

/// The folder goes first: if it cannot be recycled, the rows stay so the session can be found and retried.
async fn delete_session_inner(pool: &SqlitePool, id: &str, base: &Path, remove: Remover<'_>) -> Result<(), String> {
    // Held from the check to the last row delete, so no report claim can land in between.
    let _gate = REPORT_GATE.lock().await;
    if running_id().as_deref() == Some(id) {
        return Err(DELETE_BUSY.to_string());
    }
    // Already gone, e.g. a second click: nothing left to delete.
    let Some((row, _)) = db::get_session(pool, id).await? else {
        return Ok(());
    };
    // Row 'running' covers the start window before the slot says so. Report 'waiting' is claimed but
    // not started; report 'running' is in progress.
    if row.status == "running" || matches!(row.report_status.as_str(), "waiting" | "running") {
        return Err(DELETE_BUSY.to_string());
    }
    if let Some(folder) = row.folder_path.as_deref() {
        delete_session_folder(folder, base, remove)?;
    }
    db::delete_session_rows(pool, id).await
}

/// Run at startup: a session left running by a crash is closed as interrupted.
pub async fn recover_at_startup<R: Runtime>(app: AppHandle<R>) {
    let pool = pool_of(&app.state::<AppState>());
    match db::recover_interrupted(&pool).await {
        Ok(0) => {}
        Ok(n) => info!("practice: closed {n} interrupted session rows"),
        Err(e) => warn!("practice: could not recover interrupted sessions: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::db::{Material, MaterialMeta, SetupRow};
    use std::path::PathBuf;
    use crate::practice::report::Fluency;
    use std::sync::Mutex;

    fn session_row(report_json: Option<&str>) -> SessionRow {
        SessionRow {
            id: "s".into(),
            setup_id: None,
            setup_name: "Thesis".into(),
            character: "Examiner".into(),
            voice: "F1".into(),
            started_at: String::new(),
            duration_secs: 60.0,
            folder_path: None,
            plan: None,
            status: "ended".into(),
            end_reason: Some("user".into()),
            report_json: report_json.map(str::to_string),
            report_status: "ready".into(),
            report_error: None,
        }
    }

    #[test]
    fn unreadable_saved_report_becomes_failed_with_retry() {
        let mut row = session_row(Some("{not json"));
        let (report, unreadable) = read_stored_report(&mut row);
        assert!(report.is_none());
        assert!(unreadable);
        assert_eq!(row.report_status, "failed");
        assert_eq!(row.report_error.as_deref(), Some("The saved report could not be read — Retry"));
        assert!(row.report_json.is_none());
    }

    #[test]
    fn readable_saved_report_is_kept() {
        let stored = serde_json::to_string(&Report { version: 1, fluency: Fluency::default(), items: vec![], dropped_items: 0 }).unwrap();
        let mut row = session_row(Some(&stored));
        let (report, unreadable) = read_stored_report(&mut row);
        assert!(report.is_some());
        assert!(!unreadable);
        assert_eq!(row.report_status, "ready");
        assert!(row.report_error.is_none());
        assert_eq!(row.report_json.as_deref(), Some(stored.as_str()));

        let mut none = session_row(None);
        assert!(!read_stored_report(&mut none).1);
    }

    #[test]
    fn speak_word_validates_input() {
        assert!(validate_word("").is_err());
        assert!(validate_word("   ").is_err());
        assert!(validate_word(&"a".repeat(41)).is_err());
        assert_eq!(validate_word(&"a".repeat(40)).unwrap().len(), 40);
        assert_eq!(validate_word("  hypothesis ").unwrap(), "hypothesis");
    }

    fn ok_status() -> PracticeStatus {
        PracticeStatus {
            has_claude_key: true,
            engine_is_whisper: true,
            voices_ready: true,
            blocked: None,
            running_session: None,
        }
    }

    #[test]
    fn start_refusals_use_exact_copy() {
        assert_eq!(start_blocker(&ok_status()), None);
        assert_eq!(
            start_blocker(&PracticeStatus { has_claude_key: false, ..ok_status() }),
            Some("Add your Claude API key in Settings to use Practice")
        );
        assert_eq!(
            start_blocker(&PracticeStatus { engine_is_whisper: false, ..ok_status() }),
            Some("Practice needs Whisper as the transcription engine (Settings → Transcription)")
        );
        assert_eq!(
            start_blocker(&PracticeStatus { blocked: Some("x".into()), ..ok_status() }),
            Some("Practice is unavailable while a meeting is recording, importing or re-transcribing")
        );
        assert_eq!(
            start_blocker(&PracticeStatus { running_session: Some("s".into()), ..ok_status() }),
            Some("A practice session is already running")
        );
    }

    fn setup(plan_fingerprint: Option<String>, situation: &str) -> SetupRow {
        SetupRow {
            id: "setup".into(),
            name: "Thesis".into(),
            character: "Examiner".into(),
            situation: situation.into(),
            voice: "F1".into(),
            seeded: false,
            plan: Some("plan".into()),
            plan_fingerprint,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn material(id: &str, content: &[u8]) -> Material {
        Material {
            meta: MaterialMeta {
                id: id.into(),
                setup_id: "setup".into(),
                kind: MaterialKind::Text,
                name: id.into(),
                bytes: content.len() as i64,
            },
            content: content.to_vec(),
        }
    }

    #[test]
    fn plan_is_rebuilt_only_when_stale() {
        let mats = vec![material("a", b"notes")];
        let fresh = db::plan_fingerprint("situation", &mats);

        assert!(!needs_plan(&setup(None, "situation"), &[]), "no materials, no plan");
        assert!(needs_plan(&setup(None, "situation"), &mats), "never planned");
        assert!(!needs_plan(&setup(Some(fresh.clone()), "situation"), &mats), "same inputs");
        assert!(needs_plan(&setup(Some(fresh), "other situation"), &mats), "situation changed");
    }

    #[test]
    fn add_material_rejects_non_pdf_bytes_and_bad_utf8() {
        assert!(check_material(MaterialKind::Pdf, b"%PDF-1.7 body").is_ok());
        assert!(check_material(MaterialKind::Pdf, b"plain text").is_err());
        assert!(check_material(MaterialKind::Text, "Привет, world".as_bytes()).is_ok());
        assert!(check_material(MaterialKind::Text, &[0xff, 0xfe, 0x00]).is_err());
    }

    #[test]
    fn name_header_is_uri_decoded() {
        assert_eq!(decode_uri_component("Thesis%20defence%20%D0%96").unwrap(), "Thesis defence Ж");
        assert_eq!(decode_uri_component("plain+name").unwrap(), "plain+name");
        assert!(decode_uri_component("bad%zz").is_err());
        assert!(decode_uri_component("cut%2").is_err());
    }

    async fn session_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    /// An ended session whose folder holds only `you.wav`, directly inside `base`.
    async fn finished_session(pool: &SqlitePool, base: &Path, name: &str) -> (String, PathBuf) {
        let input = db::SetupInput {
            id: None,
            name: "Chat".into(),
            character: "Mike".into(),
            situation: "Car".into(),
            voice: "M3".into(),
        };
        let setup = db::save_setup(pool, input).await.unwrap();
        let folder = base.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("you.wav"), b"wav").unwrap();
        let row = db::create_session(pool, &setup, &folder.to_string_lossy(), None).await.unwrap();
        sqlx::query("UPDATE practice_sessions SET status = 'ended' WHERE id = ?")
            .bind(&row.id)
            .execute(pool)
            .await
            .unwrap();
        (row.id, folder)
    }

    #[tokio::test]
    async fn folder_error_keeps_the_rows() {
        let pool = session_pool().await;
        let base = tempfile::tempdir().unwrap();
        let (id, folder) = finished_session(&pool, base.path(), "Practice 1").await;
        let refuse = |_: &[PathBuf]| -> Result<(), String> { Err("Recycle Bin unavailable".to_string()) };
        assert!(delete_session_inner(&pool, &id, base.path(), &refuse).await.is_err());
        assert!(db::get_session(&pool, &id).await.unwrap().is_some(), "rows must survive a failed recycle");
        assert!(folder.join("you.wav").exists());
    }

    #[test]
    fn a_failed_start_recycles_only_its_own_folder() {
        let base = tempfile::tempdir().unwrap();
        let recycled = Mutex::new(Vec::<PathBuf>::new());
        let remove = |p: &[PathBuf]| {
            recycled.lock().unwrap().extend(p.iter().cloned());
            Ok(())
        };

        let own = base.path().join("Practice 1");
        std::fs::create_dir(&own).unwrap();
        std::fs::write(own.join(PRACTICE_FILES[0]), b"header").unwrap();
        discard_folder(&own, base.path(), &remove);
        assert_eq!(*recycled.lock().unwrap(), vec![dunce::canonicalize(&own).unwrap()]);
        recycled.lock().unwrap().clear();

        // A folder with anything else in it is refused, and nothing is removed
        let mixed = base.path().join("Practice 2");
        std::fs::create_dir(&mixed).unwrap();
        std::fs::write(mixed.join("notes.txt"), b"keep").unwrap();
        discard_folder(&mixed, base.path(), &remove);
        assert!(recycled.lock().unwrap().is_empty());
        assert!(mixed.join("notes.txt").exists());

        // A folder outside the recordings folder is refused too
        let outside = tempfile::tempdir().unwrap();
        discard_folder(outside.path(), base.path(), &remove);
        assert!(recycled.lock().unwrap().is_empty());
        assert!(outside.path().exists());
    }

    #[tokio::test]
    async fn success_recycles_the_folder_and_removes_the_rows() {
        let pool = session_pool().await;
        let base = tempfile::tempdir().unwrap();
        let (id, folder) = finished_session(&pool, base.path(), "Practice 1").await;
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let bin = |paths: &[PathBuf]| -> Result<(), String> {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::fs::remove_dir_all(&paths[0]).map_err(|e| e.to_string())
        };
        delete_session_inner(&pool, &id, base.path(), &bin).await.unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(!folder.exists());
        assert!(db::get_session(&pool, &id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_report_in_flight_is_refused_and_nothing_is_touched() {
        let pool = session_pool().await;
        let base = tempfile::tempdir().unwrap();
        let never = |_: &[PathBuf]| -> Result<(), String> { panic!("the Recycle Bin must not be called") };
        for (status, name) in [("waiting", "Practice 1"), ("running", "Practice 2")] {
            let (id, folder) = finished_session(&pool, base.path(), name).await;
            db::set_report(&pool, &id, status, None, None).await.unwrap();
            assert_eq!(delete_session_inner(&pool, &id, base.path(), &never).await, Err(DELETE_BUSY.to_string()), "{status}");
            assert!(db::get_session(&pool, &id).await.unwrap().is_some());
            assert!(folder.join("you.wav").exists());
        }
    }

    #[tokio::test]
    async fn a_session_already_gone_is_ok_so_a_double_click_is_quiet() {
        let pool = session_pool().await;
        let base = tempfile::tempdir().unwrap();
        let never = |_: &[PathBuf]| -> Result<(), String> { panic!("the Recycle Bin must not be called") };
        assert_eq!(delete_session_inner(&pool, "no-such-session", base.path(), &never).await, Ok(()));
    }

    #[tokio::test]
    async fn a_session_still_running_is_refused_and_nothing_is_touched() {
        let pool = session_pool().await;
        let base = tempfile::tempdir().unwrap();
        let (id, folder) = finished_session(&pool, base.path(), "Practice 1").await;
        sqlx::query("UPDATE practice_sessions SET status = 'running' WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        let never = |_: &[PathBuf]| -> Result<(), String> { panic!("the Recycle Bin must not be called") };
        assert_eq!(delete_session_inner(&pool, &id, base.path(), &never).await, Err(DELETE_BUSY.to_string()));
        assert!(db::get_session(&pool, &id).await.unwrap().is_some());
        assert!(folder.join("you.wav").exists());
    }

    /// The delete holds the gate while its recycle is in progress; a claim started meanwhile must wait for it.
    /// The recycle blocks in `block_in_place`, so the runtime can still run the claim and pool tasks.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_claim_waits_for_a_delete_in_progress() {
        let pool = session_pool().await;
        let base = tempfile::tempdir().unwrap();
        let (id, folder) = finished_session(&pool, base.path(), "Practice 1").await;
        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let (proceed_tx, proceed_rx) = std::sync::mpsc::channel::<()>();
        let proceed_rx = std::sync::Mutex::new(proceed_rx);
        let delete = {
            let (pool, id, base) = (pool.clone(), id.clone(), base.path().to_path_buf());
            tokio::spawn(async move {
                let bin = |paths: &[PathBuf]| -> Result<(), String> {
                    let _ = started_tx.send(());
                    tokio::task::block_in_place(|| proceed_rx.lock().unwrap().recv_timeout(std::time::Duration::from_secs(10)))
                        .map_err(|e| e.to_string())?;
                    std::fs::remove_dir_all(&paths[0]).map_err(|e| e.to_string())
                };
                delete_session_inner(&pool, &id, &base, &bin).await
            })
        };
        tokio::task::spawn_blocking(move || started_rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap())
            .await
            .unwrap();
        let claim = {
            let (pool, id) = (pool.clone(), id.clone());
            tokio::spawn(async move { report_job::claim_for_report(&pool, &id).await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let claim_waited = !claim.is_finished();
        // Release the delete before asserting, so a failed assertion cannot leave it blocked.
        proceed_tx.send(()).unwrap();
        assert_eq!(delete.await.unwrap(), Ok(()));
        assert!(claim_waited, "the claim must wait for the delete");
        assert_eq!(claim.await.unwrap(), Ok(false), "the row is gone, so no report can be claimed");
        assert!(!folder.exists());
        assert!(db::get_session(&pool, &id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_running_session_is_refused_and_nothing_is_touched() {
        let pool = session_pool().await;
        let base = tempfile::tempdir().unwrap();
        let (id, folder) = finished_session(&pool, base.path(), "Practice 1").await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
        *slot() = Slot::Running { id: id.clone(), tx };
        let never = |_: &[PathBuf]| -> Result<(), String> { panic!("the Recycle Bin must not be called") };
        let result = delete_session_inner(&pool, &id, base.path(), &never).await;
        *slot() = Slot::Idle;
        assert_eq!(result, Err(DELETE_BUSY.to_string()));
        assert!(db::get_session(&pool, &id).await.unwrap().is_some());
        assert!(folder.join("you.wav").exists());
    }

    #[tokio::test]
    async fn a_session_without_a_folder_or_with_a_missing_one_loses_only_its_rows() {
        let pool = session_pool().await;
        let base = tempfile::tempdir().unwrap();
        let (none, _) = finished_session(&pool, base.path(), "Practice 1").await;
        sqlx::query("UPDATE practice_sessions SET folder_path = NULL WHERE id = ?")
            .bind(&none)
            .execute(&pool)
            .await
            .unwrap();
        let (gone, folder) = finished_session(&pool, base.path(), "Practice 2").await;
        std::fs::remove_dir_all(&folder).unwrap();
        let never = |_: &[PathBuf]| -> Result<(), String> { panic!("the Recycle Bin must not be called") };
        delete_session_inner(&pool, &none, base.path(), &never).await.unwrap();
        delete_session_inner(&pool, &gone, base.path(), &never).await.unwrap();
        assert!(db::get_session(&pool, &none).await.unwrap().is_none());
        assert!(db::get_session(&pool, &gone).await.unwrap().is_none());
    }
}
