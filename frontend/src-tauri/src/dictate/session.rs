// One dictation at a time. SessionCore is a pure state machine: hotkey, pill and timer events go in, Effects
// come out. The executor task (below) is the only place with side effects: the microphone, the meeting mute,
// the pill, Whisper, cleanup, the clipboard and the hotkey hook. Dictated text stays in memory and is never logged.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use dictate_input::machine::{Action, Target};
use dictate_input::paste::{foreground_monitor_point, paste_text};
use dictate_input::PasteOutcome;
use log::{info, warn};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use super::capture::{CapturedAudio, MicCapture};
use super::cleanup::{self, ModeSpec};
use super::mute::MuteGuard;
use super::pill::{self, PillState, PillView};
use super::settings::{self, DictateSettings};
use super::{
    engine_is_whisper, ensure_model, whisper_engine, COPIED_SHOW_MS, EPOCH, HOLD_THRESHOLD_MS, HOOK,
    MAX_DICTATION_SECS, MIN_SPEECH_SECS, PASTED_FADE_MS, RESTORE_DELAY_MS,
};
use crate::api::api::api_get_transcript_config;
use crate::audio::audio_processing::resample_audio;
use crate::audio::vad;
use crate::audio::devices::{list_audio_devices, parse_audio_device};
use crate::audio::default_input_device;
use crate::audio::recording_preferences::load_recording_preferences;
use crate::database::repositories::setting::SettingsRepository;
use crate::get_allowed_languages_internal;
use crate::state::AppState;
use crate::summary::llm_target::{resolve_llm_target, LlmTarget};
use crate::whisper_engine::language_policy::LanguagePolicy;
use crate::whisper_engine::whisper_engine::ModelStatus;

/// A microphone that is absent or dead is retried this often.
const MIC_RETRY: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq, Eq)]
pub enum SpeechCheck {
    Ok,
    NothingHeard,
}

/// Shorter speech than `MIN_SPEECH_SECS` is treated as silence.
pub fn check_speech(speech_secs: f32) -> SpeechCheck {
    if speech_secs < MIN_SPEECH_SECS {
        SpeechCheck::NothingHeard
    } else {
        SpeechCheck::Ok
    }
}

/// The last dictation, kept in memory so the tray can copy it again. Never written to disk.
#[derive(Clone)]
pub struct LastDictation {
    pub as_spoken: String,
    pub final_text: String,
}

/// The pill state after the paste. Cleanup failure turns a paste into "as spoken". Copied text stays in the
/// last dictation, so the user can still get it with the tray.
/// Validates, then hands the checked copy to `store`. The store is never called for invalid settings.
pub fn validate_then_store(
    s: DictateSettings,
    store: impl FnOnce(DictateSettings) -> Result<DictateSettings, String>,
) -> Result<DictateSettings, String> {
    store(super::settings::validate_for_save(&s)?)
}

pub fn final_pill_state(cleanup_failed: bool, outcome: PasteOutcome) -> Option<PillState> {
    Some(match outcome {
        PasteOutcome::Pasted if cleanup_failed => PillState::PastedAsSpoken,
        PasteOutcome::Pasted => PillState::Pasted,
        PasteOutcome::Copied(_) | PasteOutcome::Failed => PillState::Copied,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum SessionEvent {
    Hotkey(Action),
    PillStop,
    PillCancel,
    CapTimer(u64),
    ShowPillTimer(u64),
    CaptureFailed,
    ProcessingDone,
    ReopenMic,
    Disable,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    BeginCapture(Target),
    /// The capture's hotkey target changed. Sent only while capturing.
    SetMode(Target),
    CancelCapture,
    EndCaptureAndProcess(Target),
    MuteOn,
    MuteOff,
    ShowPill,
    PillState(PillState),
    HidePill,
    ResetMachine,
    StartCapTimer(u64),
    StartShowPillTimer(u64),
    ReopenMic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Idle,
    Capturing {
        target: Target,
        hands_free: bool,
        id: u64,
        pill_shown: bool,
    },
    Processing,
}

pub struct SessionCore {
    phase: Phase,
    next_id: u64,
}

impl SessionCore {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            next_id: 0,
        }
    }

    pub fn capturing(&self) -> bool {
        matches!(self.phase, Phase::Capturing { .. })
    }

    /// Pure: the caller runs the returned effects in order.
    pub fn on_event(&mut self, ev: SessionEvent) -> Vec<Effect> {
        match ev {
            SessionEvent::Hotkey(action) => self.on_action(action),
            SessionEvent::PillStop => self.finish(true),
            SessionEvent::PillCancel => self.abort(true),
            SessionEvent::CapTimer(id) if self.is_capture(id) => self.finish(true),
            SessionEvent::CapTimer(_) => Vec::new(),
            SessionEvent::ShowPillTimer(id) => match &mut self.phase {
                Phase::Capturing { id: current, pill_shown, .. } if *current == id && !*pill_shown => {
                    *pill_shown = true;
                    vec![Effect::ShowPill, Effect::PillState(PillState::Listening)]
                }
                _ => Vec::new(),
            },
            SessionEvent::CaptureFailed if self.capturing() => {
                self.phase = Phase::Idle;
                vec![
                    Effect::MuteOff,
                    Effect::ShowPill,
                    Effect::PillState(PillState::NoMicrophone),
                    Effect::ResetMachine,
                ]
            }
            SessionEvent::CaptureFailed => Vec::new(),
            SessionEvent::ProcessingDone => {
                if self.phase == Phase::Processing {
                    self.phase = Phase::Idle;
                }
                Vec::new()
            }
            SessionEvent::ReopenMic => vec![Effect::ReopenMic],
            SessionEvent::Disable => match self.phase {
                Phase::Capturing { .. } => {
                    self.phase = Phase::Idle;
                    vec![Effect::CancelCapture, Effect::MuteOff, Effect::HidePill]
                }
                Phase::Processing => vec![Effect::HidePill],
                Phase::Idle => Vec::new(),
            },
        }
    }

    fn on_action(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::None => Vec::new(),
            Action::Start(target) => match self.phase {
                Phase::Idle => {
                    let id = self.next_id;
                    self.next_id += 1;
                    self.phase = Phase::Capturing {
                        target: target.clone(),
                        hands_free: false,
                        id,
                        pill_shown: false,
                    };
                    vec![
                        Effect::BeginCapture(target),
                        Effect::MuteOn,
                        Effect::StartShowPillTimer(id),
                        Effect::StartCapTimer(id),
                    ]
                }
                // The hook thinks it is starting, but a session is already running. Reset the hook's machine.
                _ => vec![Effect::ResetMachine],
            },
            Action::HandsFree(target) => match &mut self.phase {
                Phase::Capturing {
                    target: current,
                    hands_free,
                    pill_shown,
                    ..
                } => {
                    let mut effects = vec![Effect::SetMode(target.clone())];
                    *current = target;
                    *hands_free = true;
                    if !*pill_shown {
                        *pill_shown = true;
                        effects.push(Effect::ShowPill);
                    }
                    effects.push(Effect::PillState(PillState::HandsFree));
                    effects
                }
                _ => Vec::new(),
            },
            Action::Retarget(target) => match &mut self.phase {
                Phase::Capturing { target: current, .. } => {
                    *current = target.clone();
                    vec![Effect::SetMode(target)]
                }
                _ => Vec::new(),
            },
            Action::Stop => self.finish(false),
            Action::Cancel | Action::Abandon => self.abort(false),
        }
    }

    fn is_capture(&self, id: u64) -> bool {
        matches!(self.phase, Phase::Capturing { id: current, .. } if current == id)
    }

    /// Ends the capture and hands it to processing. A hold that ends just past the threshold never showed
    /// the pill, so it is shown here, and the processing states are visible.
    fn finish(&mut self, reset: bool) -> Vec<Effect> {
        let (target, pill_shown) = match &self.phase {
            Phase::Capturing { target, pill_shown, .. } => (target.clone(), *pill_shown),
            _ => return Vec::new(),
        };
        self.phase = Phase::Processing;
        let mut effects = vec![Effect::MuteOff];
        if !pill_shown {
            effects.push(Effect::ShowPill);
            effects.push(Effect::PillState(PillState::Transcribing));
        }
        effects.push(Effect::EndCaptureAndProcess(target));
        if reset {
            effects.push(Effect::ResetMachine);
        }
        effects
    }

    /// Drops the capture without pasting anything.
    fn abort(&mut self, reset: bool) -> Vec<Effect> {
        if !self.capturing() {
            return Vec::new();
        }
        self.phase = Phase::Idle;
        let mut effects = vec![Effect::CancelCapture, Effect::MuteOff, Effect::HidePill];
        if reset {
            effects.push(Effect::ResetMachine);
        }
        effects
    }
}


// ----- Executor: the only side effects. -----

static SENDER: OnceLock<UnboundedSender<SessionEvent>> = OnceLock::new();
/// The open microphone. Only the executor and the lifecycle code (`take_mic`) touch it.
static MIC: Mutex<Option<MicCapture>> = Mutex::new(None);
/// Set while a reopen runs, so two reopens never overlap.
static OPENING: AtomicBool = AtomicBool::new(false);
/// True from a capture's start to its end. Set and read under the MIC lock, so a reopen never replaces the mic
/// under a dictation.
static CAPTURING: AtomicBool = AtomicBool::new(false);
/// The microphone preference the last reopen acted on (the bare device name, or None for no preference).
/// None outside: no reopen has run yet.
static REQUESTED: Mutex<Option<Option<String>>> = Mutex::new(None);
/// Dictation's language policy, kept between dictations. Taken out while a dictation uses it.
static DICTATE_POLICY: Mutex<Option<LanguagePolicy>> = Mutex::new(None);
static LAST: Mutex<Option<LastDictation>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `f` on the open microphone, if it is alive.
fn with_live_mic<T>(f: impl FnOnce(&MicCapture) -> T) -> Option<T> {
    lock(&MIC).as_ref().filter(|m| m.is_alive()).map(f)
}

/// The mic for the lifecycle code to drop off the async runtime.
pub(super) fn take_mic() -> Option<MicCapture> {
    lock(&MIC).take()
}

/// Starts a capture on the live mic. False when there is none.
fn begin_capture() -> bool {
    let slot = lock(&MIC);
    match slot.as_ref().filter(|m| m.is_alive()) {
        Some(mic) => {
            mic.begin();
            CAPTURING.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// Dictate is still on, and it was not turned off (and on again) since the dictation began.
fn still_on(epoch: u64) -> bool {
    epoch == EPOCH.load(Ordering::SeqCst) && settings::current().enabled
}

pub fn mic_ok() -> bool {
    with_live_mic(|_| ()).is_some()
}

/// Sends an event to the session task. A no-op until Dictate has been on once.
pub fn send(ev: SessionEvent) {
    if let Some(tx) = SENDER.get() {
        let _ = tx.send(ev);
    }
}

/// The sender of the session task, starting the task on first use.
pub(super) fn start<R: Runtime>(app: &AppHandle<R>) -> UnboundedSender<SessionEvent> {
    SENDER
        .get_or_init(|| {
            let (tx, rx) = unbounded_channel();
            tauri::async_runtime::spawn(run(app.clone(), tx.clone(), rx));
            tx
        })
        .clone()
}

async fn run<R: Runtime>(
    app: AppHandle<R>,
    tx: UnboundedSender<SessionEvent>,
    mut rx: UnboundedReceiver<SessionEvent>,
) {
    let mut core = SessionCore::new();
    let mut exec = Exec::default();
    let mut retry = tokio::time::interval(MIC_RETRY);
    let mut pill_tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            ev = rx.recv() => {
                let Some(ev) = ev else { break };
                for effect in core.on_event(ev) {
                    exec.run(&app, &tx, effect);
                }
            }
            _ = retry.tick() => {
                if !core.capturing() && settings::current().enabled {
                    tauri::async_runtime::spawn(reopen(app.clone()));
                }
            }
            _ = pill_tick.tick() => exec.tick(&app, core.capturing()),
        }
    }
}

/// Per-session side state that the effects need: the meeting mute, the pill's state and mode.
#[derive(Default)]
struct Exec {
    muted: Option<MuteGuard>,
    mode: String,
    pill_state: Option<PillState>,
    started: Option<Instant>,
    /// Dictate's epoch when the current capture began.
    epoch: u64,
}

impl Exec {
    fn run<R: Runtime>(&mut self, app: &AppHandle<R>, tx: &UnboundedSender<SessionEvent>, effect: Effect) {
        match effect {
            Effect::BeginCapture(target) => {
                self.mode = resolve_mode_id(&target, &settings::current().default_mode);
                self.started = Some(Instant::now());
                self.epoch = EPOCH.load(Ordering::SeqCst);
                self.pill_state = None;
                if !begin_capture() {
                    warn!("dictate: no microphone to capture from");
                    let _ = tx.send(SessionEvent::CaptureFailed);
                    tauri::async_runtime::spawn(reopen(app.clone()));
                }
            }
            Effect::SetMode(target) => {
                self.mode = resolve_mode_id(&target, &settings::current().default_mode);
                // A visible pill is re-sent so it shows the new mode.
                if let Some(state) = self.pill_state {
                    set_pill(app, state, &self.mode);
                }
            }
            Effect::CancelCapture => {
                with_live_mic(|m| m.cancel());
                CAPTURING.store(false, Ordering::SeqCst);
            }
            Effect::EndCaptureAndProcess(target) => {
                let captured = with_live_mic(|m| m.end());
                CAPTURING.store(false, Ordering::SeqCst);
                tauri::async_runtime::spawn(process(app.clone(), tx.clone(), target, captured, self.epoch));
            }
            Effect::MuteOn => self.muted = Some(MuteGuard::new()),
            Effect::MuteOff => self.muted = None,
            Effect::ShowPill => pill::show(app, foreground_monitor_point()),
            Effect::PillState(state) => {
                self.pill_state = Some(state);
                set_pill(app, state, &self.mode);
            }
            Effect::HidePill => pill::close(app),
            Effect::ResetMachine => {
                if let Some(hook) = lock(&HOOK).as_ref() {
                    hook.reset();
                }
            }
            Effect::StartCapTimer(id) => spawn_timer(tx, Duration::from_secs(MAX_DICTATION_SECS), SessionEvent::CapTimer(id)),
            Effect::StartShowPillTimer(id) => {
                spawn_timer(tx, Duration::from_millis(HOLD_THRESHOLD_MS), SessionEvent::ShowPillTimer(id))
            }
            Effect::ReopenMic => {
                tauri::async_runtime::spawn(reopen(app.clone()));
            }
        }
    }

    /// Keeps the live pill's level and elapsed time moving while a dictation is being recorded.
    fn tick<R: Runtime>(&self, app: &AppHandle<R>, capturing: bool) {
        let Some(state) = self.pill_state else { return };
        if !capturing || !matches!(state, PillState::Listening | PillState::HandsFree) {
            return;
        }
        let view = PillView {
            state,
            elapsed_ms: self.started.map_or(0, |t| t.elapsed().as_millis() as u64),
            level: with_live_mic(|m| m.level()).unwrap_or(0.0),
            mode: self.mode.clone(),
        };
        pill::update(app, &view);
    }
}

fn spawn_timer(tx: &UnboundedSender<SessionEvent>, after: Duration, ev: SessionEvent) {
    let tx = tx.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(after).await;
        let _ = tx.send(ev);
    });
}

/// The mode a dictation runs in: the hotkey's mode, or the default mode for the main hotkey.
fn resolve_mode_id(target: &Target, default_mode: &str) -> String {
    match target {
        Target::Main => default_mode.to_string(),
        Target::Mode(id) => id.clone(),
    }
}

/// Shows a pill state. The final states hide the pill after their own delay.
fn set_pill<R: Runtime>(app: &AppHandle<R>, state: PillState, mode: &str) {
    pill::update(
        app,
        &PillView {
            state,
            elapsed_ms: 0,
            level: 0.0,
            mode: mode.to_string(),
        },
    );
    match state {
        PillState::Pasted | PillState::PastedAsSpoken => pill::hide_after(app, PASTED_FADE_MS),
        PillState::Copied | PillState::NothingHeard | PillState::NoMicrophone => {
            pill::hide_after(app, COPIED_SHOW_MS)
        }
        _ => {}
    }
}

/// Opens the microphone the preferences ask for, when needed: no live mic, the preference changed since the
/// last reopen, or the preferred device is listed again after a fallback. A reopen during a capture is
/// deferred; the retry tick picks it up once the capture has ended.
async fn reopen<R: Runtime>(app: AppHandle<R>) {
    if OPENING.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Ok(prefs) = load_recording_preferences(&app).await {
        let raw = prefs.preferred_mic_device;
        let wanted = raw
            .as_deref()
            .and_then(|p| parse_audio_device(p).ok())
            .map(|d| d.name);
        let epoch = EPOCH.load(Ordering::SeqCst);
        let _ = tokio::task::spawn_blocking(move || reopen_blocking(raw, wanted, epoch)).await;
    }
    OPENING.store(false, Ordering::SeqCst);
}

fn reopen_blocking(raw: Option<String>, wanted: Option<String>, epoch: u64) {
    // Dictate off means no open microphone, so nothing is opened (the indicator must not flash).
    if CAPTURING.load(Ordering::SeqCst) || !still_on(epoch) {
        return;
    }
    let changed = lock(&REQUESTED).as_ref() != Some(&wanted);
    // The device the preference resolves to; with no preference, the default input.
    let target = wanted
        .clone()
        .or_else(|| default_input_device().ok().map(|d| d.name));
    let need = match with_live_mic(|m| m.device_name()) {
        None => true,
        Some(name) => target
            .as_ref()
            .is_some_and(|t| &name != t && (changed || device_listed(t))),
    };
    if !need {
        *lock(&REQUESTED) = Some(wanted);
        return;
    }
    match MicCapture::open(raw) {
        Ok(mic) => {
            let mut slot = lock(&MIC);
            // Checked under the lock: a capture or a turn-off that started meanwhile wins, and the new mic is dropped.
            if CAPTURING.load(Ordering::SeqCst) || !still_on(epoch) {
                drop(slot);
                drop(mic);
                return;
            }
            let old = slot.replace(mic);
            drop(slot);
            drop(old);
            *lock(&REQUESTED) = Some(wanted);
        }
        Err(e) => warn!("dictate: microphone not opened: {e}"),
    }
}

fn device_listed(name: &str) -> bool {
    tauri::async_runtime::block_on(list_audio_devices())
        .map(|devices| devices.iter().any(|d| d.name == name))
        .unwrap_or(false)
}

/// Sends `ProcessingDone` when dropped, so the session returns to Idle on every exit path, including a panic.
struct DoneGuard(UnboundedSender<SessionEvent>);

impl Drop for DoneGuard {
    fn drop(&mut self) {
        let _ = self.0.send(SessionEvent::ProcessingDone);
    }
}

/// Speech seconds of 16 kHz audio, by the VAD. A VAD failure counts as silence.
fn speech_secs(a16: &[f32]) -> f32 {
    match vad::get_speech_chunks(a16, 400) {
        Ok(segments) => segments.iter().map(|s| s.samples.len() as f32 / 16_000.0).sum(),
        Err(_) => {
            warn!("dictate: speech detection failed");
            0.0
        }
    }
}

/// Takes the language policy out for one dictation, rebuilt when the allowed languages changed.
fn take_policy() -> LanguagePolicy {
    let allowed = get_allowed_languages_internal();
    match lock(&DICTATE_POLICY).take() {
        Some(policy) if policy.allowed() == allowed.as_slice() => policy,
        _ => LanguagePolicy::new(allowed),
    }
}

/// The LLM for cleanup: the cleanup provider and model if both are set, else the summary provider and model.
async fn cleanup_target<R: Runtime>(app: &AppHandle<R>) -> Result<LlmTarget, String> {
    let state = app.try_state::<AppState>().ok_or("no database")?;
    let pool = state.db_manager.pool();
    let s = settings::current();
    let (provider, model) = match (s.cleanup_provider, s.cleanup_model) {
        (Some(p), Some(m)) => (p, m),
        _ => {
            let config = SettingsRepository::get_model_config(pool)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("no summary provider set")?;
            (config.provider, config.model)
        }
    };
    resolve_llm_target(pool, &provider, &model).await
}

/// No text to paste. With Dictate off meanwhile, the pill closes without a message.
fn nothing_heard<R: Runtime>(app: &AppHandle<R>, mode_id: &str, epoch: u64) {
    if still_on(epoch) {
        set_pill(app, PillState::NothingHeard, mode_id);
    } else {
        pill::close(app);
    }
}

/// One dictation after the mic is closed: VAD, Whisper, cleanup, paste. Errors end in a pill state, never a panic.
async fn process<R: Runtime>(
    app: AppHandle<R>,
    tx: UnboundedSender<SessionEvent>,
    target: Target,
    captured: Option<CapturedAudio>,
    epoch: u64,
) {
    let _done = DoneGuard(tx);
    let s = settings::current();
    let mode_id = resolve_mode_id(&target, &s.default_mode);
    let started = Instant::now();
    let Some(audio) = captured else {
        warn!("dictate: no audio captured");
        nothing_heard(&app, &mode_id, epoch);
        return;
    };
    let rate = audio.sample_rate;
    let prepared = tokio::task::spawn_blocking(move || {
        let a16 = resample_audio(&audio.samples, rate, 16_000);
        let secs = speech_secs(&a16);
        (a16, secs)
    })
    .await;
    let Ok((a16, secs)) = prepared else {
        nothing_heard(&app, &mode_id, epoch);
        return;
    };
    if matches!(check_speech(secs), SpeechCheck::NothingHeard) {
        info!("dictate: nothing heard ({:.1} s of speech)", secs);
        nothing_heard(&app, &mode_id, epoch);
        return;
    }
    if !engine_is_whisper(&app).await {
        warn!("dictate: transcript provider is not localWhisper, dictation skipped");
        nothing_heard(&app, &mode_id, epoch);
        return;
    }
    let loaded = match whisper_engine() {
        Some(engine) => engine.is_model_loaded().await,
        None => false,
    };
    if !loaded {
        set_pill(&app, PillState::LoadingModel, &mode_id);
    }
    if ensure_model(&app).await.is_err() {
        warn!("dictate: Whisper model not ready");
        nothing_heard(&app, &mode_id, epoch);
        return;
    }
    set_pill(&app, PillState::Transcribing, &mode_id);
    let Some(engine) = whisper_engine() else {
        nothing_heard(&app, &mode_id, epoch);
        return;
    };
    let mut policy = take_policy();
    let outcome = engine
        .transcribe_audio_with_confidence(
            a16,
            Some("auto".to_string()),
            Some(&mut policy),
            None,
        )
        .await;
    *lock(&DICTATE_POLICY) = Some(policy);
    let text = match outcome {
        Ok((text, _, _, _)) => text.trim().to_string(),
        Err(_) => {
            warn!("dictate: transcription failed");
            nothing_heard(&app, &mode_id, epoch);
            return;
        }
    };
    if text.is_empty() {
        nothing_heard(&app, &mode_id, epoch);
        return;
    }
    info!("dictate: transcribed {} chars in {} ms", text.chars().count(), started.elapsed().as_millis());
    if !still_on(epoch) {
        // Dictate went off while Whisper ran: the text stays for the tray, but nothing is cleaned up or pasted.
        info!("dictate: Dictate was turned off during transcription, nothing pasted");
        store_last(&app, text.clone(), text);
        return;
    }

    let spec = cleanup::resolve_mode(&s, &mode_id);
    let (final_text, cleanup_failed) = match &spec {
        ModeSpec::AsSpoken => (text.clone(), false),
        _ => {
            let cleaned = match cleanup_target(&app).await {
                Ok(llm) => cleanup::clean(&llm, &spec, &text, &s.dictionary).await,
                Err(e) => Err(e),
            };
            match cleaned {
                Ok(out) => (out, false),
                Err(_) => {
                    warn!("dictate: cleanup failed, pasting as spoken");
                    (text.clone(), true)
                }
            }
        }
    };
    // Kept before the paste, so a failed paste can still be copied from the tray.
    store_last(&app, text, final_text.clone());
    if !still_on(epoch) {
        info!("dictate: Dictate was turned off, paste skipped");
        return;
    }
    let chars = final_text.chars().count();
    let outcome = tokio::task::spawn_blocking(move || {
        paste_text(&final_text, std::process::id(), Duration::from_millis(RESTORE_DELAY_MS), &|| still_on(epoch))
    })
    .await
    .unwrap_or(PasteOutcome::Failed);
    info!(
        "dictate: pasted {} chars, mode {}, {} ms, outcome {:?}",
        chars,
        mode_id,
        started.elapsed().as_millis(),
        outcome
    );
    if !still_on(epoch) {
        // Dictate went off during the paste: show nothing.
        pill::close(&app);
        return;
    }
    if let Some(state) = final_pill_state(cleanup_failed, outcome) {
        set_pill(&app, state, &mode_id);
    }
}

fn store_last<R: Runtime>(app: &AppHandle<R>, as_spoken: String, final_text: String) {
    *lock(&LAST) = Some(LastDictation { as_spoken, final_text });
    crate::tray::update_tray_menu(app);
}

/// The last dictation's text, for the tray. Memory only.
pub fn last_dictation() -> Option<LastDictation> {
    lock(&LAST).clone()
}

#[tauri::command]
pub async fn dictate_save_settings<R: Runtime>(
    app: AppHandle<R>,
    settings: DictateSettings,
) -> Result<DictateSettings, String> {
    super::save_and_apply(&app, settings).await
}

/// A built-in mode or one of the custom modes.
fn is_known_mode(s: &DictateSettings, id: &str) -> bool {
    id == settings::AS_SPOKEN_ID || id == settings::CLEANED_UP_ID || s.modes.iter().any(|m| m.id == id)
}

/// Makes `id` the default mode and saves it, as the settings page does. An id that is not a mode is refused
/// without a save.
pub async fn set_default_mode<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    if !is_known_mode(&settings::current(), id) {
        return Err("unknown mode".into());
    }
    let saved = super::update_and_apply(app, |old| with_default_mode(old, id)).await?;
    // The open Dictate page keeps its own copy. Only the tray changes the mode behind its back, so only this tells it.
    if let Err(e) = app.emit("dictate-settings-changed", saved) {
        warn!("dictate: settings change not sent to the page: {e}");
    }
    Ok(())
}

fn with_default_mode(old: &DictateSettings, id: &str) -> DictateSettings {
    DictateSettings {
        default_mode: id.to_string(),
        ..old.clone()
    }
}

#[tauri::command]
pub fn dictate_pill_action(action: String) -> Result<(), String> {
    match action.as_str() {
        "stop" => send(SessionEvent::PillStop),
        "cancel" => send(SessionEvent::PillCancel),
        _ => return Err("unknown pill action".into()),
    }
    Ok(())
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct DictateStatus {
    pub enabled: bool,
    pub model_downloaded: bool,
    pub mic_ok: bool,
    pub engine_is_whisper: bool,
    pub reason: Option<String>,
}

#[tauri::command]
pub async fn dictate_status<R: Runtime>(app: AppHandle<R>) -> DictateStatus {
    let enabled = settings::current().enabled;
    let engine_is_whisper = engine_is_whisper(&app).await;
    let model_downloaded = model_downloaded(&app).await;
    let mic_ok = mic_ok();
    let reason = if !enabled {
        None
    } else if !engine_is_whisper {
        Some("Dictate needs the local Whisper transcription.".to_string())
    } else if !model_downloaded {
        Some("The Whisper model is not downloaded.".to_string())
    } else if lock(&HOOK).is_none() {
        Some("The hotkey listener is not running.".to_string())
    } else if !mic_ok {
        Some("No microphone.".to_string())
    } else {
        None
    };
    DictateStatus {
        enabled,
        model_downloaded,
        mic_ok,
        engine_is_whisper,
        reason,
    }
}

/// Whether the configured Whisper model is on disk.
async fn model_downloaded<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let Ok(Some(config)) = api_get_transcript_config(app.clone(), state, None).await else {
        return false;
    };
    let Some(engine) = whisper_engine() else {
        return false;
    };
    match engine.discover_models().await {
        Ok(models) => models
            .iter()
            .any(|m| m.name == config.model && matches!(m.status, ModelStatus::Available)),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictate::settings::{self as settings_mod, CustomMode};

    #[test]
    fn stop_without_a_shown_pill_shows_it_first() {
        // A hold that ends just past HOLD_THRESHOLD_MS: the pill timer has not fired yet.
        let mut core = SessionCore::new();
        start(&mut core);
        let fx = core.on_event(SessionEvent::Hotkey(Action::Stop));
        assert_eq!(
            fx,
            vec![
                Effect::MuteOff,
                Effect::ShowPill,
                Effect::PillState(PillState::Transcribing),
                Effect::EndCaptureAndProcess(Target::Main)
            ]
        );
        // The timer that was already in flight does nothing now.
        assert!(core.on_event(SessionEvent::ShowPillTimer(0)).is_empty());
    }

    fn start(core: &mut SessionCore) -> Vec<Effect> {
        core.on_event(SessionEvent::Hotkey(Action::Start(Target::Main)))
    }

    #[test]
    fn default_mode_change_keeps_the_other_settings() {
        let s = DictateSettings {
            enabled: true,
            dictionary: vec!["Kubernetes".into()],
            ..DictateSettings::default()
        };
        let next = with_default_mode(&s, "as-spoken");
        assert_eq!(next.default_mode, "as-spoken");
        assert!(next.enabled);
        assert_eq!(next.dictionary, vec!["Kubernetes".to_string()]);
    }

    #[test]
    fn known_modes_are_builtin_or_custom() {
        let mut s = DictateSettings::default();
        s.modes.push(CustomMode {
            id: "mine".into(),
            name: "Mine".into(),
            instructions: String::new(),
            examples: vec![],
            hotkey: None,
        });
        assert!(is_known_mode(&s, "as-spoken"));
        assert!(is_known_mode(&s, "cleaned-up"));
        assert!(is_known_mode(&s, "mine"));
        assert!(!is_known_mode(&s, "deleted-mode"));
        assert!(!is_known_mode(&s, ""));
    }

    #[test]
    fn speech_gate() {
        assert_eq!(check_speech(0.49), SpeechCheck::NothingHeard);
        assert_eq!(check_speech(0.5), SpeechCheck::Ok);
    }

    #[test]
    fn final_states() {
        assert_eq!(final_pill_state(false, PasteOutcome::Pasted), Some(PillState::Pasted));
        assert_eq!(final_pill_state(true, PasteOutcome::Pasted), Some(PillState::PastedAsSpoken));
        assert_eq!(
            final_pill_state(false, PasteOutcome::Copied(dictate_input::CopiedReason::Elevated)),
            Some(PillState::Copied)
        );
        assert_eq!(final_pill_state(false, PasteOutcome::Failed), Some(PillState::Copied));
    }

    #[test]
    fn start_while_processing_resets_machine() {
        let mut core = SessionCore::new();
        start(&mut core);
        core.on_event(SessionEvent::Hotkey(Action::Stop));
        assert_eq!(start(&mut core), vec![Effect::ResetMachine]);
    }

    #[test]
    fn processing_done_returns_to_idle() {
        let mut core = SessionCore::new();
        start(&mut core);
        core.on_event(SessionEvent::Hotkey(Action::Stop));
        core.on_event(SessionEvent::ProcessingDone);
        assert!(start(&mut core).contains(&Effect::BeginCapture(Target::Main)));
    }

    #[test]
    fn capture_failed_releases() {
        let mut core = SessionCore::new();
        start(&mut core);
        assert_eq!(
            core.on_event(SessionEvent::CaptureFailed),
            vec![
                Effect::MuteOff,
                Effect::ShowPill,
                Effect::PillState(PillState::NoMicrophone),
                Effect::ResetMachine
            ]
        );
        assert!(!core.capturing());
    }

    #[test]
    fn hands_free_pill_not_overwritten() {
        let mut core = SessionCore::new();
        start(&mut core);
        core.on_event(SessionEvent::Hotkey(Action::HandsFree(Target::Main)));
        assert!(core.on_event(SessionEvent::ShowPillTimer(0)).is_empty());
    }

    #[test]
    fn stale_cap_timer_ignored() {
        let mut core = SessionCore::new();
        start(&mut core);
        core.on_event(SessionEvent::Hotkey(Action::Abandon));
        start(&mut core);
        assert!(core.on_event(SessionEvent::CapTimer(0)).is_empty());
    }

    #[test]
    fn abandon_never_shows_pill() {
        let mut core = SessionCore::new();
        start(&mut core);
        let fx = core.on_event(SessionEvent::Hotkey(Action::Abandon));
        assert!(fx.contains(&Effect::CancelCapture));
        assert!(fx.contains(&Effect::MuteOff));
        assert!(!fx.contains(&Effect::ShowPill));
        assert!(core.on_event(SessionEvent::ShowPillTimer(0)).is_empty());
    }

    #[test]
    fn start_with_a_mode_begins_in_that_mode() {
        let mut core = SessionCore::new();
        let fx = core.on_event(SessionEvent::Hotkey(Action::Start(Target::Mode("x".into()))));
        assert!(fx.contains(&Effect::BeginCapture(Target::Mode("x".into()))));
    }

    #[test]
    fn retarget_while_capturing_sets_the_mode() {
        let mut core = SessionCore::new();
        start(&mut core);
        assert_eq!(
            core.on_event(SessionEvent::Hotkey(Action::Retarget(Target::Mode("y".into())))),
            vec![Effect::SetMode(Target::Mode("y".into()))]
        );
    }

    #[test]
    fn retarget_while_idle_or_processing_does_nothing() {
        let mut core = SessionCore::new();
        assert!(core
            .on_event(SessionEvent::Hotkey(Action::Retarget(Target::Mode("y".into()))))
            .is_empty());
        start(&mut core);
        core.on_event(SessionEvent::Hotkey(Action::Stop));
        assert!(core
            .on_event(SessionEvent::Hotkey(Action::Retarget(Target::Mode("y".into()))))
            .is_empty());
    }

    #[test]
    fn hands_free_with_a_mode_sets_the_mode() {
        let mut core = SessionCore::new();
        start(&mut core);
        let fx = core.on_event(SessionEvent::Hotkey(Action::HandsFree(Target::Mode("y".into()))));
        assert_eq!(fx.first(), Some(&Effect::SetMode(Target::Mode("y".into()))));
    }

    #[test]
    fn mode_id_resolves_main_to_the_default() {
        assert_eq!(resolve_mode_id(&Target::Main, "cleaned-up"), "cleaned-up");
        assert_eq!(resolve_mode_id(&Target::Mode("x".into()), "cleaned-up"), "x");
    }

    #[test]
    fn mute_taken_for_every_start() {
        let mut core = SessionCore::new();
        assert!(start(&mut core).contains(&Effect::MuteOn));
    }

    #[test]
    fn pill_stop_resets_machine() {
        let mut core = SessionCore::new();
        start(&mut core);
        core.on_event(SessionEvent::Hotkey(Action::HandsFree(Target::Main)));
        let fx = core.on_event(SessionEvent::PillStop);
        assert!(fx.contains(&Effect::EndCaptureAndProcess(Target::Main)));
        assert!(fx.contains(&Effect::ResetMachine));
    }

    #[test]
    fn disable_during_capture_cancels() {
        let mut core = SessionCore::new();
        start(&mut core);
        assert_eq!(
            core.on_event(SessionEvent::Disable),
            vec![Effect::CancelCapture, Effect::MuteOff, Effect::HidePill]
        );
        assert!(!core.capturing());
    }

    #[test]
    fn keeps_model_follows_switch() {
        let _guard = settings_mod::CACHE_TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let on = DictateSettings { enabled: true, ..DictateSettings::default() };
        settings::set_cache_for_test(on);
        assert!(super::super::keeps_model_loaded());
        settings::set_cache_for_test(DictateSettings::default());
        assert!(!super::super::keeps_model_loaded());
    }

    #[test]
    fn save_rejects_invalid_without_side_effects() {
        let mut s = DictateSettings::default();
        s.modes.push(CustomMode {
            id: "m".into(),
            name: "M".into(),
            instructions: String::new(),
            examples: vec![],
            hotkey: Some("Ctrl+Win".into()),
        });
        let mut stored = false;
        let res = validate_then_store(s, |v| {
            stored = true;
            Ok(v)
        });
        assert!(res.is_err());
        assert!(!stored);
    }
}
