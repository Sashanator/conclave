// The async side of live dictation: drains the mic while it records, feeds a `LiveCore`, runs its decode jobs on
// Whisper and hands back the finished transcript. Any failure ends in `failed: true`, so the caller decodes the
// whole recording instead. Dictated text is never logged.

use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{info, warn};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::MissedTickBehavior;

use super::live::{tail_chars, Job, LiveCore, PILL_TAIL_CHARS};
use super::session::with_live_mic;
use crate::audio::audio_processing::resample_audio;
use crate::audio::recording_commands::is_recording_now;
use crate::audio::vad::ContinuousVadProcessor;
use crate::get_allowed_languages_internal;
use crate::whisper_engine::language_policy::LanguagePolicy;
use crate::whisper_engine::WhisperEngine;

const TICK_MS: u64 = 150;

/// What the pill shows: the committed tail and the grey preview.
#[derive(Clone, Default, PartialEq)]
pub struct LiveText {
    pub committed: String,
    pub volatile: String,
}

#[derive(Clone, Debug)]
pub struct LiveOutcome {
    pub transcript: String,
    pub speech_secs: f32,
    pub language: Option<String>,
    pub failed: bool,
}

/// Dropping the handle without `finish` cancels the worker.
pub struct LiveHandle {
    pub text: watch::Receiver<LiveText>,
    finish: oneshot::Sender<Arc<Vec<f32>>>,
    task: tauri::async_runtime::JoinHandle<LiveOutcome>,
}

impl LiveHandle {
    /// Hands the worker the whole recording and waits for its transcript. None: the worker is gone or panicked.
    pub async fn finish(self, all: Arc<Vec<f32>>) -> Option<LiveOutcome> {
        self.finish.send(all).ok()?;
        self.task.await.ok()
    }
}

enum Done {
    Commit { index: u64, text: Result<String, ()>, policy: Option<LanguagePolicy> },
    Preview { from: usize, text: String },
}

pub fn start(rate: u32) -> LiveHandle {
    let (text_tx, text) = watch::channel(LiveText::default());
    let (finish, finish_rx) = oneshot::channel();
    let task = tauri::async_runtime::spawn(run(rate, text_tx, finish_rx));
    LiveHandle { text, finish, task }
}

/// VAD first, then the core, then the segments the VAD closed: `LiveCore` relies on this order.
fn feed(core: &mut LiveCore, vad: &mut Option<ContinuousVadProcessor>, s: &[f32]) {
    let Some(v) = vad.as_mut() else { return };
    match v.process_audio(s) {
        Ok(segments) => {
            core.push_audio(s, v.in_speech());
            for seg in segments {
                core.on_vad_segment(seg.start_timestamp_ms, seg.end_timestamp_ms);
            }
        }
        Err(_) => {
            warn!("dictate: live VAD failed");
            core.fail();
        }
    }
}

async fn run(rate: u32, text_tx: watch::Sender<LiveText>, mut finish_rx: oneshot::Receiver<Arc<Vec<f32>>>) -> LiveOutcome {
    let mut core = LiveCore::new(rate);
    let mut vad = match ContinuousVadProcessor::new(rate, 400) {
        Ok(v) => Some(v),
        Err(_) => {
            warn!("dictate: live VAD did not start");
            core.fail();
            None
        }
    };
    let mut policy = LanguagePolicy::new(get_allowed_languages_internal());
    let (mut drained, t0) = (0usize, Instant::now());
    let (res_tx, mut res_rx) = mpsc::unbounded_channel::<Done>();
    let mut tick = tokio::time::interval(Duration::from_millis(TICK_MS));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // Set when the finish message arrives: from then on nothing drains the mic (a following dictation's audio must
    // not enter this core) and the completed oneshot is not polled again.
    let mut finishing = false;
    let mut published = LiveText::default();

    loop {
        tokio::select! {
            _ = tick.tick(), if !finishing => {
                if let Some(s) = with_live_mic(|m| m.samples_since(drained)).flatten() {
                    drained += s.len();
                    feed(&mut core, &mut vad, &s);
                }
            }
            msg = &mut finish_rx, if !finishing => match msg {
                Ok(all) => {
                    feed(&mut core, &mut vad, &all[drained.min(all.len())..]);
                    if let Some(v) = vad.as_mut() {
                        match v.flush() {
                            Ok(segs) => {
                                for seg in segs {
                                    core.on_vad_segment(seg.start_timestamp_ms, seg.end_timestamp_ms);
                                }
                            }
                            Err(_) => {
                                warn!("dictate: live VAD flush failed");
                                core.fail();
                            }
                        }
                    }
                    core.finish();
                    finishing = true;
                }
                Err(_) => {
                    core.cancel();
                    break;
                }
            },
            Some(done) = res_rx.recv() => match done {
                Done::Commit { index, text, policy: returned } => {
                    let ok = text.is_ok();
                    if ok {
                        if let Some(p) = returned {
                            policy = p;
                        }
                        policy.lock_if_unset();
                    }
                    core.on_commit_done(index, text, policy.current().map(str::to_string));
                }
                Done::Preview { from, text } => core.on_preview_done(from, text),
            },
        }

        match super::whisper_engine() {
            Some(engine) if engine.is_model_loaded().await => {
                let ms = t0.elapsed().as_millis() as u64;
                while let Some(job) = core.next_job(ms, is_recording_now(), engine.inference_idle()) {
                    spawn_decode(engine.clone(), rate, policy.clone(), job, res_tx.clone());
                }
            }
            // Nothing can decode what is still queued: fall back instead of waiting forever.
            _ if finishing => core.fail(),
            _ => {}
        }
        let now = LiveText {
            committed: tail_chars(core.transcript(), PILL_TAIL_CHARS).to_string(),
            volatile: core.volatile().to_string(),
        };
        if now != published {
            published = now.clone();
            let _ = text_tx.send(now);
        }
        if core.is_done() {
            break;
        }
    }

    let (commits, previews, skipped) = core.stats();
    let language = policy.current().map(str::to_string);
    info!(
        "dictate: live {} segments, {} previews ({} skipped), language {}, fallback {}",
        commits,
        previews,
        skipped,
        language.as_deref().unwrap_or("undecided"),
        core.failed()
    );
    LiveOutcome { transcript: core.transcript().trim().to_string(), speech_secs: core.speech_secs(), language, failed: core.failed() }
}

/// A commit sends its policy back; a preview works on a copy that is dropped. Commits are never cancelled.
fn spawn_decode(engine: Arc<WhisperEngine>, rate: u32, mut p: LanguagePolicy, job: Job, tx: mpsc::UnboundedSender<Done>) {
    tauri::async_runtime::spawn(async move {
        let (samples, prompt) = match &job {
            Job::Commit { samples, prompt, .. } | Job::Preview { samples, prompt, .. } => (samples.clone(), prompt.clone()),
        };
        let a16 = tokio::task::spawn_blocking(move || resample_audio(&samples, rate, 16_000)).await.ok();
        let text = match a16 {
            Some(a16) => engine
                .transcribe_audio_with_confidence(a16, Some("auto".to_string()), Some(&mut p), (!prompt.is_empty()).then_some(prompt.as_str()))
                .await
                .map(|(t, ..)| t)
                .map_err(|_| ()),
            None => Err(()),
        };
        let done = match job {
            Job::Commit { index, .. } => {
                let ok = text.is_ok();
                Done::Commit { index, text, policy: ok.then_some(p) }
            }
            Job::Preview { from, .. } => Done::Preview { from, text: text.unwrap_or_default() },
        };
        let _ = tx.send(done);
    });
}
