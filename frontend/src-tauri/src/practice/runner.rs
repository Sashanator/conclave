//! The Speaking Practice session runner. One task owns the session's microphone, speech and Claude
//! work. It feeds events to the pure state machine in `session.rs` and carries out the effects it returns.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use log::{info, warn};
use serde_json::Value;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Notify;
use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::audio::recording_commands::is_recording_now;
use crate::dictate;
use crate::summary::llm_client::TokenUsage;
use crate::audio::vad::{ContinuousVadProcessor, SpeechSegment};
use crate::whisper_engine::WhisperEngine;

use super::audio::{Player, TurnMic, TurnRecorder, TurnResampler};
use super::claude::{self, ChatMsg, ClaudeError};
use super::commands::{release_slot, PracticeStatePayload, PracticeTurnPayload};
use super::db::{self, SetupRow, TurnRow};
use super::prompts::{character_system, Persona, SentenceSplitter, OPENING_USER, WRAP_UP_SYSTEM};
use super::report_job;
use super::session::{Core, EndReason, Effect, Event, Notice, Phase};
use super::settings;
use super::tts::{Supertonic, Voice};
use super::{hold_model, ModelHold, LIVE_VAD_REDEMPTION_MS, MAX_TURN_SECS, TTS_SPEED, TTS_STEPS, YOU_WAV_RATE};

/// The first text from Claude must arrive within this long, or the reply counts as failed.
const FIRST_BYTE_SECS: u64 = 20;
/// How often a capture checks for its stop flag while the microphone is quiet.
const CAPTURE_POLL: Duration = Duration::from_millis(50);

/// A capture running on a blocking thread. It hands the microphone receiver back with its result.
type CaptureJob = JoinHandle<(Receiver<Vec<f32>>, Result<CapturedTurn, String>)>;

/// Everything a session holds once it has loaded. Built by `commands::practice_start`.
pub struct Loaded {
    pub session_id: String,
    pub setup: SetupRow,
    pub plan: Option<String>,
    pub key: String,
    pub pool: SqlitePool,
    pub hold: ModelHold,
    pub engine: Arc<WhisperEngine>,
    pub tts: Supertonic,
    pub voice: Voice,
    pub tts_dir: PathBuf,
    pub mic: TurnMic,
    pub mic_rx: Receiver<Vec<f32>>,
    pub mic_rate: u32,
    pub mic_pref: Option<String>,
    pub player: Player,
    pub finished: Receiver<u64>,
    pub recorder: TurnRecorder,
    pub seed: u32,
    pub tx: UnboundedSender<Event>,
    pub rx: UnboundedReceiver<Event>,
}

/// Stores the wire-visible history for the next Claude call. An interrupted reply is marked with a dash.
pub fn push_history(history: &mut Vec<ChatMsg>, role: &'static str, text: &str, interrupted: bool) {
    let content = if interrupted {
        format!("{text} —")
    } else {
        text.to_string()
    };
    history.push(ChatMsg { role, content });
}

/// Player ids pack the reply and the sentence number, so a finished id maps back to its event.
fn sentence_id(reply: u64, seq: u32) -> u64 {
    (reply << 32) | u64::from(seq)
}

fn split_sentence_id(id: u64) -> (u64, u32) {
    (id >> 32, id as u32)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn ms(from: Option<Instant>, to: Option<Instant>) -> String {
    match (from, to) {
        (Some(a), Some(b)) => b.saturating_duration_since(a).as_millis().to_string(),
        _ => "-".to_string(),
    }
}

#[derive(Clone, Copy, Default)]
struct TurnAudio {
    at_secs: f64,
    /// Offset and length in samples within you.wav, when the audio was saved.
    span: Option<(i64, i64)>,
}

struct CapturedTurn {
    turn: Vec<f32>,
    text: String,
    speech_secs: f32,
}

struct SpeakJob {
    reply: u64,
    seq: u32,
    text: String,
    first_audio: Arc<OnceLock<Instant>>,
}

pub async fn run<R: Runtime>(app: AppHandle<R>, loaded: Loaded) {
    let mut r = Runner::new(app, loaded);
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    loop {
        let ev = match r.queue.pop_front() {
            Some(ev) => ev,
            None => tokio::select! {
                ev = r.rx.recv() => match ev {
                    Some(ev) => ev,
                    None => break,
                },
                _ = ticker.tick() => {
                    r.on_tick();
                    continue;
                }
                // A turn's transcript arrives here, so the UI stays live while Whisper finishes.
                joined = pending_capture(&mut r.capture) => {
                    r.capture_joined(joined);
                    continue;
                }
            },
        };
        r.dispatch(ev).await;
        if r.core.phase() == Phase::Ended && r.queue.is_empty() && r.capture.is_none() {
            break;
        }
    }
    r.finish().await;
}

/// Runs a session. If it panics, the session still ends: the row is closed, the slot is freed and the model
/// is released. Its hold is dropped while the panic unwinds.
pub async fn supervise<R: Runtime>(app: AppHandle<R>, loaded: Loaded) {
    let (session_id, pool, started) = (loaded.session_id.clone(), loaded.pool.clone(), Instant::now());
    let (character, emitter) = (loaded.setup.character.clone(), app.clone());
    supervised(run(app, loaded), |e| async move {
        warn!("practice: the session stopped unexpectedly: {e}");
        let duration = started.elapsed().as_secs_f64();
        if let Err(e) = db::end_session_if_running(&pool, &session_id, duration, EndReason::User.as_str()).await {
            warn!("practice: could not close the session: {e}");
        }
        release_slot(&session_id);
        dictate::unload_if_idle().await;
        // Tells an open call view and report view what happened, even though the runner is gone.
        let (state, report) = stopped_events(&session_id, &character, started.elapsed().as_secs());
        let _ = emitter.emit("practice-state", state);
        let _ = emitter.emit("practice-report", report);
    })
    .await;
}

/// The two events a stopped session leaves for the UI: its state is Ended, and its report is pending.
/// A session that ran to its end has no report yet, so the status is always pending here.
fn stopped_events(session_id: &str, character: &str, elapsed_secs: u64) -> (PracticeStatePayload, report_job::ReportStatusPayload) {
    (
        PracticeStatePayload {
            session_id: session_id.to_string(),
            phase: Phase::Ended,
            character: character.to_string(),
            elapsed_secs,
            caption: None,
            notice: None,
        },
        report_job::ReportStatusPayload {
            session_id: session_id.to_string(),
            status: "pending",
        },
    )
}

/// Awaits `task` on its own task. If that task panics or is aborted, `on_stop` runs with the error text.
pub async fn supervised<F, C, Fut>(task: F, on_stop: C)
where
    F: Future<Output = ()> + Send + 'static,
    C: FnOnce(String) -> Fut,
    Fut: Future<Output = ()>,
{
    if let Err(e) = tauri::async_runtime::spawn(task).await {
        on_stop(e.to_string()).await;
    }
}

/// Resolves when the capture finishes. Without a capture it never resolves.
async fn pending_capture(capture: &mut Option<CaptureJob>) -> Result<(Receiver<Vec<f32>>, Result<CapturedTurn, String>), JoinError> {
    match capture {
        Some(job) => job.await,
        None => std::future::pending().await,
    }
}

/// The event that ends a reply whose speaker died while the reply was still being spoken or written.
pub fn speaker_failure(phase: Phase, speaker_alive: bool, reply: u64) -> Option<Event> {
    let speaking = matches!(phase, Phase::CharacterSpeaking | Phase::Thinking);
    (speaking && !speaker_alive).then_some(Event::ReplyFailed { reply })
}

/// Clears the speech gate only if `reply` still owns it. Returns whether it did.
pub fn clear_gate_if_current(gate: &mut u64, reply: u64) -> bool {
    if *gate == reply {
        *gate = 0;
        true
    } else {
        false
    }
}

/// Ends a reply whose speech failed. A late failure of an old reply must not silence the current one.
fn fail_reply(gate: &Mutex<u64>, tx: &UnboundedSender<Event>, reply: u64) {
    if clear_gate_if_current(&mut lock(gate), reply) {
        let _ = tx.send(Event::ReplyFailed { reply });
    }
}

struct Runner<R: Runtime> {
    app: AppHandle<R>,
    session_id: String,
    character: String,
    system: String,
    /// Read once at session start, so every reply of the session uses the same model.
    chat_model: String,
    key: String,
    client: reqwest::Client,
    pool: SqlitePool,
    hold: Option<ModelHold>,
    engine: Arc<WhisperEngine>,
    core: Core,
    started: Instant,
    tx: UnboundedSender<Event>,
    rx: UnboundedReceiver<Event>,
    /// Events the runner queued for itself (a finished transcript, a tick). Handled before `rx`.
    queue: VecDeque<Event>,
    mic: TurnMic,
    mic_rx: Option<Receiver<Vec<f32>>>,
    mic_rate: u32,
    mic_pref: Option<String>,
    capture: Option<CaptureJob>,
    capture_stop: Arc<AtomicBool>,
    turn_timer: Option<JoinHandle<()>>,
    last_turn: TurnAudio,
    recorder: Option<TurnRecorder>,
    /// Shared with the speech worker, which is handed a fresh player when the old one dies.
    speaker: Arc<Mutex<Arc<Player>>>,
    gate: Arc<Mutex<u64>>,
    speak_tx: UnboundedSender<SpeakJob>,
    captions_pending: HashMap<u64, String>,
    caption: Option<String>,
    reply_cancel: Option<CancellationToken>,
    /// The reply the state machine last asked for.
    reply_no: u64,
    reply_started: f64,
    reply_first_sentence: Option<Instant>,
    reply_first_token: Arc<OnceLock<Instant>>,
    reply_first_audio: Arc<OnceLock<Instant>>,
    history: Vec<ChatMsg>,
    turn_idx: i64,
    tap: Option<Instant>,
    stt: Option<Instant>,
    end_reason: Option<EndReason>,
}

impl<R: Runtime> Runner<R> {
    fn new(app: AppHandle<R>, l: Loaded) -> Self {
        let system = character_system(
            &Persona {
                character: &l.setup.character,
                situation: &l.setup.situation,
                plan: l.plan.as_deref(),
            },
            l.seed,
        );
        let speaker = Arc::new(Mutex::new(Arc::new(l.player)));
        let gate = Arc::new(Mutex::new(0));
        let (speak_tx, speak_rx) = unbounded_channel();
        tauri::async_runtime::spawn(speak_worker(
            speak_rx,
            Arc::new(Mutex::new(l.tts)),
            Arc::new(l.voice),
            l.tts_dir,
            speaker.clone(),
            gate.clone(),
            l.tx.clone(),
        ));
        let fin_tx = l.tx.clone();
        std::thread::spawn(move || forward_finished(l.finished, fin_tx));
        Self {
            app,
            session_id: l.session_id,
            character: l.setup.character.clone(),
            system,
            chat_model: settings::current().chat_model,
            key: l.key,
            client: claude::http_client(),
            pool: l.pool,
            hold: Some(l.hold),
            engine: l.engine,
            core: Core::new(),
            started: Instant::now(),
            tx: l.tx,
            rx: l.rx,
            queue: VecDeque::new(),
            mic: l.mic,
            mic_rx: Some(l.mic_rx),
            mic_rate: l.mic_rate,
            mic_pref: l.mic_pref,
            capture: None,
            capture_stop: Arc::new(AtomicBool::new(false)),
            turn_timer: None,
            last_turn: TurnAudio::default(),
            recorder: Some(l.recorder),
            speaker,
            gate,
            speak_tx,
            captions_pending: HashMap::new(),
            caption: None,
            reply_cancel: None,
            reply_no: 0,
            reply_started: 0.0,
            reply_first_sentence: None,
            reply_first_token: Arc::new(OnceLock::new()),
            reply_first_audio: Arc::new(OnceLock::new()),
            history: Vec::new(),
            turn_idx: 0,
            tap: None,
            stt: None,
            end_reason: None,
        }
    }

    fn elapsed_f64(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn elapsed(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Runs one event through the state machine, carries out its effects, then reports the new state once.
    async fn dispatch(&mut self, ev: Event) {
        if let Event::SentencePlayed { reply, seq } = &ev {
            if let Some(text) = self.captions_pending.remove(&sentence_id(*reply, *seq)) {
                if !text.trim().is_empty() {
                    self.caption = Some(text);
                }
            }
        }
        let mut notice = None;
        for effect in self.core.on_event(ev) {
            match effect {
                Effect::Notice(n) => notice = Some(n),
                other => self.apply(other).await,
            }
        }
        self.emit(notice);
    }

    fn emit(&self, notice: Option<Notice>) {
        let payload = PracticeStatePayload {
            session_id: self.session_id.clone(),
            phase: self.core.phase(),
            character: self.character.clone(),
            elapsed_secs: self.elapsed(),
            caption: self.caption.clone(),
            notice,
        };
        let _ = self.app.emit("practice-state", payload);
    }

    async fn apply(&mut self, effect: Effect) {
        match effect {
            Effect::Phase(_) | Effect::Notice(_) => {}
            Effect::StartCapture => self.start_capture(),
            Effect::StopCaptureAndTranscribe => self.stop_capture(),
            Effect::AskCharacter { reply, wrap_up } => self.ask(reply, wrap_up),
            Effect::Speak { reply, seq, text } => self.speak(reply, seq, text),
            Effect::StopSpeaking => self.stop_speaking(),
            Effect::CancelReply { .. } => self.cancel_reply(),
            Effect::SaveUserTurn { text } => self.save_user(text).await,
            Effect::SaveCharacterTurn { text, interrupted } => {
                self.save_character(text, interrupted).await
            }
            Effect::End(reason) => self.end_reason = Some(reason),
        }
    }

    fn start_capture(&mut self) {
        // A microphone whose capture task was lost (no receiver handed back) is reopened too.
        if !self.mic.is_alive() || self.mic_rx.is_none() {
            match TurnMic::open(self.mic_pref.clone()) {
                Ok((mic, rx, rate)) => {
                    self.mic = mic;
                    self.mic_rx = Some(rx);
                    self.mic_rate = rate;
                }
                Err(e) => warn!("practice: microphone reopen failed: {e}"),
            }
        }
        let Some(rx) = self.mic_rx.take() else {
            warn!("practice: no microphone; this turn is not captured");
            return;
        };
        // Chunks left from before this turn must not be prepended to it.
        while rx.try_recv().is_ok() {}
        self.last_turn.at_secs = self.elapsed_f64();
        self.capture_stop = Arc::new(AtomicBool::new(false));
        let (stop, engine, rate) = (self.capture_stop.clone(), self.engine.clone(), self.mic_rate);
        self.mic.set_recording(true);
        self.capture = Some(tokio::task::spawn_blocking(move || {
            let turn = capture_turn(&rx, rate, &stop, &engine);
            (rx, turn)
        }));
        let tx = self.tx.clone();
        self.turn_timer = Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs_f64(MAX_TURN_SECS)).await;
            let _ = tx.send(Event::TurnTooLong);
        }));
    }

    /// Stops the capture. The transcript comes back through `capture_joined`, so the UI keeps updating
    /// while Whisper finishes the last segment.
    fn stop_capture(&mut self) {
        self.mic.set_recording(false);
        if let Some(timer) = self.turn_timer.take() {
            timer.abort();
        }
        self.capture_stop.store(true, Ordering::SeqCst);
        self.tap = Some(Instant::now());
        if self.capture.is_none() {
            self.settle_turn(Err("no capture was running".to_string()));
        }
    }

    fn capture_joined(&mut self, joined: Result<(Receiver<Vec<f32>>, Result<CapturedTurn, String>), JoinError>) {
        self.capture = None;
        match joined {
            Ok((rx, captured)) => {
                self.mic_rx = Some(rx);
                self.settle_turn(captured);
            }
            // The receiver went down with the task, so the next turn reopens the microphone.
            Err(e) => self.settle_turn(Err(format!("capture task failed: {e}"))),
        }
    }

    /// Saves the turn's audio and queues its transcript. Every turn ends here, so Transcribing always ends.
    fn settle_turn(&mut self, captured: Result<CapturedTurn, String>) {
        let (text, speech_secs) = match captured {
            Ok(turn) => {
                let span = match self.recorder.as_mut().map(|r| r.append(&turn.turn)) {
                    Some(Ok(span)) => Some(span),
                    Some(Err(e)) => {
                        warn!("practice: turn audio not saved: {e}");
                        None
                    }
                    None => None,
                };
                self.last_turn.span = span;
                (turn.text, turn.speech_secs)
            }
            Err(e) => {
                warn!("practice: turn not transcribed: {e}");
                self.last_turn.span = None;
                (String::new(), 0.0)
            }
        };
        self.stt = Some(Instant::now());
        self.queue.push_back(Event::TurnTranscribed { text, speech_secs });
    }

    fn on_tick(&mut self) {
        self.queue.push_back(Event::Tick { elapsed_secs: self.elapsed() });
        if is_recording_now() {
            self.queue.push_back(Event::RecordingStarted);
        }
        if let Some(ev) = speaker_failure(self.core.phase(), self.speaker_alive(), self.reply_no) {
            self.queue.push_back(ev);
        }
    }

    fn speaker_alive(&self) -> bool {
        lock(&self.speaker).is_alive()
    }

    /// Opens a fresh speaker when the old one died. Called before each reply; a failed open leaves the
    /// dead one, and the reply then fails on its first sentence.
    fn ensure_speaker(&mut self) {
        if self.speaker_alive() {
            return;
        }
        match Player::open() {
            Ok((player, finished)) => {
                info!("practice: speaker reopened");
                *lock(&self.speaker) = Arc::new(player);
                let fin_tx = self.tx.clone();
                std::thread::spawn(move || forward_finished(finished, fin_tx));
            }
            Err(e) => warn!("practice: speaker reopen failed: {e}"),
        }
    }

    fn ask(&mut self, reply: u64, wrap_up: bool) {
        self.reply_no = reply;
        self.ensure_speaker();
        if self.history.is_empty() {
            push_history(&mut self.history, "user", OPENING_USER, false);
        }
        *lock(&self.gate) = reply;
        let cancel = CancellationToken::new();
        self.reply_cancel = Some(cancel.clone());
        self.reply_started = self.elapsed_f64();
        self.reply_first_sentence = None;
        self.reply_first_token = Arc::new(OnceLock::new());
        self.reply_first_audio = Arc::new(OnceLock::new());
        let system = if wrap_up {
            format!("{}\n\n{WRAP_UP_SYSTEM}", self.system)
        } else {
            self.system.clone()
        };
        let body = claude::chat_body(&self.chat_model, &system, &self.history);
        tauri::async_runtime::spawn(chat_reply(
            self.client.clone(),
            self.key.clone(),
            body,
            self.pool.clone(),
            reply,
            cancel,
            self.tx.clone(),
            self.reply_first_token.clone(),
        ));
    }

    fn speak(&mut self, reply: u64, seq: u32, text: String) {
        self.reply_first_sentence.get_or_insert_with(Instant::now);
        if !text.trim().is_empty() {
            self.captions_pending.insert(sentence_id(reply, seq), text.clone());
        }
        let _ = self.speak_tx.send(SpeakJob {
            reply,
            seq,
            text,
            first_audio: self.reply_first_audio.clone(),
        });
    }

    fn stop_speaking(&mut self) {
        *lock(&self.gate) = 0;
        lock(&self.speaker).stop_all();
        self.captions_pending.clear();
        self.cancel_reply();
    }

    fn cancel_reply(&mut self) {
        if let Some(cancel) = self.reply_cancel.take() {
            cancel.cancel();
        }
    }

    async fn save_user(&mut self, text: String) {
        let turn = self.last_turn;
        self.save_turn("user", &text, turn.at_secs, turn.span).await;
        push_history(&mut self.history, "user", &text, false);
    }

    async fn save_character(&mut self, text: String, interrupted: bool) {
        self.save_turn("character", &text, self.reply_started, None).await;
        push_history(&mut self.history, "assistant", &text, interrupted);
        self.log_latency();
    }

    async fn save_turn(&mut self, speaker: &'static str, text: &str, at_secs: f64, span: Option<(i64, i64)>) {
        let idx = self.turn_idx;
        self.turn_idx += 1;
        let row = TurnRow {
            session_id: self.session_id.clone(),
            idx,
            speaker: speaker.to_string(),
            live_text: text.to_string(),
            verbatim_text: None,
            at_secs,
            wav_offset: span.map(|s| s.0),
            wav_len: span.map(|s| s.1),
        };
        if let Err(e) = db::add_turn(&self.pool, &row).await {
            warn!("practice: could not save a turn: {e}");
        }
        let _ = self.app.emit(
            "practice-turn",
            PracticeTurnPayload {
                session_id: self.session_id.clone(),
                idx,
                speaker,
                text: text.to_string(),
            },
        );
    }

    fn log_latency(&self) {
        let Some(tap) = self.tap else { return };
        info!(
            "practice: turn stt_ms={} first_token_ms={} first_sentence_ms={} first_audio_ms={}",
            ms(Some(tap), self.stt),
            ms(Some(tap), self.reply_first_token.get().copied()),
            ms(Some(tap), self.reply_first_sentence),
            ms(Some(tap), self.reply_first_audio.get().copied()),
        );
    }

    async fn finish(mut self) {
        self.mic.set_recording(false);
        // A capture still running when the session ends is settled and saved before the file closes.
        if let Some(job) = self.capture.take() {
            let joined = job.await;
            self.capture_joined(joined);
        }
        while let Some(ev) = self.queue.pop_front() {
            self.dispatch(ev).await;
        }
        self.stop_speaking();
        let reason = self.end_reason.unwrap_or(EndReason::User).as_str();
        let duration = self.elapsed_f64();
        if let Some(recorder) = self.recorder.take() {
            if let Err(e) = recorder.finish() {
                warn!("practice: could not finish you.wav: {e}");
            }
        }
        if let Err(e) = db::end_session(&self.pool, &self.session_id, duration, reason).await {
            warn!("practice: could not close the session: {e}");
        }
        // Dropping the speech worker's sender lets it exit and release the TTS model.
        drop(self.speak_tx);
        // The report job takes over the Whisper hold, so the model stays loaded across the hand-off.
        let hold = self.hold.take().unwrap_or_else(hold_model);
        tauri::async_runtime::spawn(report_job::run(self.app.clone(), self.session_id.clone(), hold));
        release_slot(&self.session_id);
    }
}

/// Speech for one reply, one sentence at a time, so the sentences play in order.
async fn speak_worker(
    mut jobs: UnboundedReceiver<SpeakJob>,
    tts: Arc<Mutex<Supertonic>>,
    voice: Arc<Voice>,
    dir: PathBuf,
    speaker: Arc<Mutex<Arc<Player>>>,
    gate: Arc<Mutex<u64>>,
    tx: UnboundedSender<Event>,
) {
    let mut fell_back = false;
    while let Some(job) = jobs.recv().await {
        let SpeakJob { reply, seq, text, first_audio } = job;
        let player = lock(&speaker).clone();
        // Only the reply that is still current may speak.
        if *lock(&gate) != reply {
            continue;
        }
        if text.trim().is_empty() {
            let _ = tx.send(Event::SentencePlayed { reply, seq });
            continue;
        }
        if !player.is_alive() {
            warn!("practice: speaker stream is down");
            fail_reply(&gate, &tx, reply);
            continue;
        }
        let wav = match synthesize(&tts, &dir, &voice, text, &mut fell_back).await {
            Ok(wav) => wav,
            Err(e) => {
                warn!("practice: speech failed: {e}");
                fail_reply(&gate, &tx, reply);
                continue;
            }
        };
        // A cancel that landed during synthesis must not reach the speaker.
        let current = lock(&gate);
        if *current == reply {
            let _ = first_audio.set(Instant::now());
            player.play(sentence_id(reply, seq), wav);
        }
    }
}

/// Synthesizes one sentence. On the first failure the TTS reloads on the CPU and retries once.
async fn synthesize(
    tts: &Arc<Mutex<Supertonic>>,
    dir: &Path,
    voice: &Arc<Voice>,
    text: String,
    fell_back: &mut bool,
) -> Result<Vec<f32>, String> {
    match run_synth(tts, voice, text.clone()).await {
        Ok(wav) => Ok(wav),
        Err(e) if !*fell_back => {
            *fell_back = true;
            warn!("practice: speech failed on the accelerator, retrying on the CPU: {e}");
            let dir = dir.to_path_buf();
            let cpu = tokio::task::spawn_blocking(move || Supertonic::load(&dir, false))
                .await
                .map_err(|e| e.to_string())??;
            *lock(tts) = cpu;
            run_synth(tts, voice, text).await
        }
        Err(e) => Err(e),
    }
}

async fn run_synth(tts: &Arc<Mutex<Supertonic>>, voice: &Arc<Voice>, text: String) -> Result<Vec<f32>, String> {
    let (tts, voice) = (tts.clone(), voice.clone());
    tokio::task::spawn_blocking(move || {
        let result = lock(&tts).synthesize(&text, &voice, TTS_STEPS, TTS_SPEED);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

fn forward_finished(finished: Receiver<u64>, tx: UnboundedSender<Event>) {
    while let Ok(id) = finished.recv() {
        let (reply, seq) = split_sentence_id(id);
        if tx.send(Event::SentencePlayed { reply, seq }).is_err() {
            break;
        }
    }
}

/// Streams one character reply. Sentences go out as they complete; the reply ends with `ReplyDone`,
/// or `ReplyFailed` on any error, including no first text within `FIRST_BYTE_SECS`.
async fn chat_reply(
    client: reqwest::Client,
    key: String,
    body: Value,
    pool: SqlitePool,
    reply: u64,
    cancel: CancellationToken,
    tx: UnboundedSender<Event>,
    first_token: Arc<OnceLock<Instant>>,
) {
    let got_text = Arc::new(Notify::new());
    let watchdog = {
        let (got_text, cancel) = (got_text.clone(), cancel.clone());
        tokio::spawn(async move {
            let first = tokio::time::timeout(Duration::from_secs(FIRST_BYTE_SECS), got_text.notified()).await;
            if first.is_err() {
                warn!("practice: no text from Claude within {FIRST_BYTE_SECS} s");
                cancel.cancel();
            }
        })
    };
    let mut splitter = SentenceSplitter::new();
    let mut seq = 0u32;
    let mut usage = TokenUsage::default();
    let result = claude::stream(&client, &key, &body, &cancel, &mut usage, |delta| {
        got_text.notify_one();
        let _ = first_token.set(Instant::now());
        for text in splitter.push(delta) {
            let _ = tx.send(Event::ReplySentence { reply, seq, text });
            seq += 1;
        }
    })
    .await;
    watchdog.abort();
    // Recorded whatever the outcome: a reply cut at the token cap or cancelled mid-stream was billed.
    claude::record_usage(&pool, &body, &usage).await;
    // Hitting the CHAT_MAX_TOKENS cap ends the reply normally: the text so far is spoken.
    let result = match result {
        Err(e) if is_token_cap(&e) => Ok(String::new()),
        other => other,
    };
    match result {
        Ok(_) => {
            if let Some(text) = splitter.finish() {
                let _ = tx.send(Event::ReplySentence { reply, seq, text });
            }
            let _ = tx.send(Event::ReplyDone { reply });
        }
        Err(e) => {
            warn!("practice: character reply failed: {e:?}");
            let _ = tx.send(Event::ReplyFailed { reply });
        }
    }
}

/// True when Claude stopped at the token cap, which ends a chat reply normally rather than failing it.
pub fn is_token_cap(e: &ClaudeError) -> bool {
    matches!(e, ClaudeError::Stream(reason) if reason == "max_tokens")
}

/// Transcribes the segments of one turn in order. Their lengths add to `speech_secs`. A segment that fails
/// is logged and skipped, so the rest of the turn still counts.
fn transcribe_segments(
    engine: &WhisperEngine,
    segments: Vec<SpeechSegment>,
    texts: &mut Vec<String>,
    speech_secs: &mut f32,
) {
    for segment in segments {
        *speech_secs += segment.samples.len() as f32 / YOU_WAV_RATE as f32;
        let result = tauri::async_runtime::block_on(engine.transcribe_audio_with_confidence(
            segment.samples,
            Some("en".to_string()),
            None,
            None,
        ));
        match result {
            Ok((text, _, _, _)) => {
                let text = text.trim();
                if !text.is_empty() {
                    texts.push(text.to_string());
                }
            }
            Err(e) => warn!("practice: a speech segment was not transcribed: {e}"),
        }
    }
}

/// Runs on a blocking thread for one turn: resamples to 16 kHz, keeps the whole turn, and transcribes
/// each speech segment as the VAD closes it. A VAD or Whisper error loses only its segment, never the audio.
fn capture_turn(rx: &Receiver<Vec<f32>>, rate: u32, stop: &AtomicBool, engine: &WhisperEngine) -> Result<CapturedTurn, String> {
    let mut resampler = TurnResampler::new(rate)?;
    let mut vad = match ContinuousVadProcessor::new(YOU_WAV_RATE, LIVE_VAD_REDEMPTION_MS as u32) {
        Ok(vad) => Some(vad),
        Err(e) => {
            warn!("practice: VAD unavailable, this turn is not transcribed: {e}");
            None
        }
    };
    let mut turn = CapturedTurn {
        turn: Vec::new(),
        text: String::new(),
        speech_secs: 0.0,
    };
    let mut texts: Vec<String> = Vec::new();
    let mut feed = |samples: &[f32]| {
        turn.turn.extend_from_slice(samples);
        if let Some(vad) = vad.as_mut() {
            match vad.process_audio(samples) {
                Ok(segments) => transcribe_segments(engine, segments, &mut texts, &mut turn.speech_secs),
                Err(e) => warn!("practice: VAD failed on one chunk, it is skipped: {e}"),
            }
        }
    };
    loop {
        match rx.recv_timeout(CAPTURE_POLL) {
            Ok(chunk) => feed(&resampler.push(&chunk)?),
            // The stop flag is read only once the queue is empty, so the tail of the turn is kept.
            Err(RecvTimeoutError::Timeout) if stop.load(Ordering::SeqCst) => break,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    feed(&resampler.finish()?);
    if let Some(vad) = vad.as_mut() {
        match vad.flush() {
            Ok(segments) => transcribe_segments(engine, segments, &mut texts, &mut turn.speech_secs),
            Err(e) => warn!("practice: VAD flush failed, the last segment is lost: {e}"),
        }
    }
    turn.text = texts.join(" ");
    Ok(turn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_marks_interrupted_turns() {
        let mut history = Vec::new();
        push_history(&mut history, "user", OPENING_USER, false);
        push_history(&mut history, "assistant", "Hi there.", true);
        push_history(&mut history, "user", "Hello", false);
        // A user turn may follow a user turn: nothing is merged or dropped.
        push_history(&mut history, "user", "Anyone?", false);

        let got: Vec<(&str, &str)> = history.iter().map(|m| (m.role, m.content.as_str())).collect();
        assert_eq!(
            got,
            vec![
                ("user", OPENING_USER),
                ("assistant", "Hi there. —"),
                ("user", "Hello"),
                ("user", "Anyone?"),
            ]
        );
    }

    #[test]
    fn sentence_ids_round_trip() {
        for (reply, seq) in [(1u64, 0u32), (7, 3), (u32::MAX as u64, u32::MAX)] {
            assert_eq!(split_sentence_id(sentence_id(reply, seq)), (reply, seq));
        }
    }

    #[test]
    fn gate_is_cleared_only_by_the_reply_that_owns_it() {
        let mut gate = 2;
        assert!(!clear_gate_if_current(&mut gate, 1), "a stale failure leaves the current reply's gate");
        assert_eq!(gate, 2);
        assert!(clear_gate_if_current(&mut gate, 2));
        assert_eq!(gate, 0);
    }

    #[test]
    fn dead_speaker_fails_only_a_reply_in_progress() {
        use Phase::*;
        for phase in [CharacterSpeaking, Thinking] {
            assert_eq!(speaker_failure(phase, false, 3), Some(Event::ReplyFailed { reply: 3 }));
            assert_eq!(speaker_failure(phase, true, 3), None);
        }
        for phase in [Loading, UserTurn, Listening, Transcribing, Ended] {
            assert_eq!(speaker_failure(phase, false, 3), None, "{phase:?}");
        }
    }

    #[tokio::test]
    async fn a_panicking_session_still_runs_its_stop_handler() {
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = stopped.clone();
        supervised(async { panic!("boom") }, move |_| async move {
            flag.store(true, Ordering::SeqCst);
        })
        .await;
        assert!(stopped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_session_that_ends_normally_skips_the_stop_handler() {
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = stopped.clone();
        supervised(async {}, move |_| async move {
            flag.store(true, Ordering::SeqCst);
        })
        .await;
        assert!(!stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn a_stopped_session_tells_the_ui_it_ended_with_a_pending_report() {
        let (state, report) = stopped_events("s1", "Anna", 42);
        assert_eq!(state.session_id, "s1");
        assert_eq!(state.phase, Phase::Ended);
        assert_eq!(state.character, "Anna");
        assert_eq!(state.elapsed_secs, 42);
        assert!(state.caption.is_none() && state.notice.is_none());
        assert_eq!(report.session_id, "s1");
        assert_eq!(report.status, "pending");
    }

    #[test]
    fn only_the_token_cap_ends_a_chat_reply_normally() {
        assert!(is_token_cap(&ClaudeError::Stream("max_tokens".into())));
        assert!(!is_token_cap(&ClaudeError::Stream("refusal".into())));
        assert!(!is_token_cap(&ClaudeError::Http(500)));
    }

    #[test]
    fn latency_fields_are_dashes_until_measured() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(250);
        assert_eq!(ms(None, Some(t1)), "-");
        assert_eq!(ms(Some(t0), None), "-");
        assert_eq!(ms(Some(t0), Some(t1)), "250");
    }
}
