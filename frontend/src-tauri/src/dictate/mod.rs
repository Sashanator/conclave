// Dictate: hold a hotkey, speak, and the text is pasted where the cursor is.
// The tunable values from the spec live here so they can be found and changed in one place.

pub mod capture;
pub mod cleanup;
pub mod mute;
pub mod pill;
pub mod session;
pub mod settings;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use dictate_input::machine::Action;
use dictate_input::HookHandle;
use log::{info, warn};
use tauri::{AppHandle, Manager, Runtime};

use crate::api::api::api_get_transcript_config;
use crate::audio::common::acquire_engine_lifecycle_lock;
use crate::audio::import::is_import_in_progress;
use crate::audio::recording_commands::is_recording_now;
use crate::audio::retranscription::is_retranscription_in_progress;
use crate::audio::transcription::engine::get_or_init_whisper;
use crate::state::AppState;
use crate::whisper_engine::commands::WHISPER_ENGINE;
use crate::whisper_engine::WhisperEngine;
use settings::DictateSettings;
use session::SessionEvent;

/// A press shorter than this is a tap (hands-free), not a push-to-talk hold.
pub const HOLD_THRESHOLD_MS: u64 = 300;
/// Audio kept from just before the hotkey, so the first word is not clipped.
pub const PRE_ROLL_MS: u64 = 500;
/// Meeting transcription stays muted this long after a dictation ends.
pub const MUTE_TAIL_MS: u64 = 300;
/// The cleanup model gets this long before the as-spoken text is pasted instead.
pub const CLEANUP_TIMEOUT_SECS: u64 = 10;
/// The clipboard is put back this long after the paste.
pub const RESTORE_DELAY_MS: u64 = 500;
/// A dictation is cut off at this length.
pub const MAX_DICTATION_SECS: u64 = 600;
/// Shorter recordings are treated as silence and not sent to Whisper.
pub const MIN_SPEECH_SECS: f32 = 0.5;
/// The pill's "Pasted" state fades out after this long.
pub const PASTED_FADE_MS: u64 = 1000;
/// The pill's "Copied" state stays up this long.
pub const COPIED_SHOW_MS: u64 = 4000;

/// The hotkey hook, while Dictate is on.
static HOOK: Mutex<Option<HookHandle>> = Mutex::new(None);
/// Bumped when Dictate is turned off. A dictation that started before a bump does not paste.
static EPOCH: AtomicU64 = AtomicU64::new(0);
/// The Whisper model should be unloaded once the recording or batch job using it ends.
static UNLOAD_PENDING: AtomicBool = AtomicBool::new(false);
/// A Whisper model change arrived while the engine was busy. It is loaded once the job ends.
static RELOAD_PENDING: AtomicBool = AtomicBool::new(false);

/// Dictate on means the Whisper model stays loaded. Both unload paths (meeting stop and batch jobs) check this.
pub fn keeps_model_loaded() -> bool {
    settings::current().enabled
}

/// Unload only when Dictate does not need the model and no recording or batch job uses it.
pub fn should_unload_now(dictate_on: bool, recording: bool, importing: bool, retranscribing: bool) -> bool {
    !dictate_on && !recording && !importing && !retranscribing
}

pub(crate) fn whisper_engine() -> Option<Arc<WhisperEngine>> {
    WHISPER_ENGINE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn engine_busy() -> bool {
    is_recording_now() || is_import_in_progress() || is_retranscription_in_progress()
}

/// True when the transcript provider is the local Whisper engine. An unreadable config counts as not Whisper.
pub(crate) async fn engine_is_whisper<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    match api_get_transcript_config(app.clone(), state, None).await {
        Ok(Some(config)) => config.provider == crate::config::DEFAULT_TRANSCRIPT_PROVIDER,
        Ok(None) => true,
        Err(_) => false,
    }
}

/// Loads the configured Whisper model while Dictate is on. A busy engine keeps its loaded model; the
/// configured one loads when the job ends. Dictate off makes this a no-op.
pub async fn ensure_model<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let _lifecycle = acquire_engine_lifecycle_lock().await;
    if !keeps_model_loaded() {
        return Ok(());
    }
    if engine_busy() {
        if let Some(engine) = whisper_engine() {
            if engine.is_model_loaded().await {
                RELOAD_PENDING.store(true, Ordering::SeqCst);
                return Ok(());
            }
        }
    }
    get_or_init_whisper(app).await.map(|_| ())
}

/// Unloads the Whisper model if nothing needs it. The check is repeated under the lifecycle lock, so a
/// recording that starts meanwhile is never unloaded. A busy engine sets UNLOAD_PENDING instead.
pub async fn unload_if_idle() {
    let _lifecycle = acquire_engine_lifecycle_lock().await;
    let dictate_on = keeps_model_loaded();
    if !should_unload_now(
        dictate_on,
        is_recording_now(),
        is_import_in_progress(),
        is_retranscription_in_progress(),
    ) {
        UNLOAD_PENDING.store(!dictate_on, Ordering::SeqCst);
        return;
    }
    UNLOAD_PENDING.store(false, Ordering::SeqCst);
    if let Some(engine) = whisper_engine() {
        if engine.unload_model().await {
            info!("dictate: Whisper model unloaded");
        }
    }
}

/// Completes what a busy engine deferred. Call without the lifecycle lock held, after a recording or batch
/// job has ended.
pub async fn finish_pending_unload<R: Runtime>(app: &AppHandle<R>) {
    if UNLOAD_PENDING.swap(false, Ordering::SeqCst) {
        unload_if_idle().await;
    }
    if RELOAD_PENDING.swap(false, Ordering::SeqCst) {
        if ensure_model(app).await.is_err() {
            warn!("dictate: Whisper model reload failed");
        }
    }
}

/// Applies a settings change: starts or stops the hook and the mic, and keeps the model in step.
pub async fn apply<R: Runtime>(app: &AppHandle<R>, old: &DictateSettings, new: &DictateSettings) {
    match (old.enabled, new.enabled) {
        (false, true) => turn_on(app, new).await,
        (true, false) => turn_off(app).await,
        (true, true) => {
            if let Some(hook) = HOOK.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                hook.set_bindings(settings::bindings(new));
            }
        }
        (false, false) => {}
    }
}

async fn turn_on<R: Runtime>(app: &AppHandle<R>, new: &DictateSettings) {
    let events = session::start(app);
    let hotkeys = settings::bindings(new);
    let started = tokio::task::spawn_blocking(move || {
        dictate_input::start_hook(
            hotkeys,
            HOLD_THRESHOLD_MS,
            Box::new(move |action: Action| {
                if action != Action::None {
                    let _ = events.send(SessionEvent::Hotkey(action));
                }
            }),
        )
    })
    .await;
    match started {
        Ok(Ok(hook)) => *HOOK.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook),
        Ok(Err(e)) => warn!("dictate: hotkeys not started: {e}"),
        Err(_) => warn!("dictate: hotkeys not started"),
    }
    session::send(SessionEvent::ReopenMic);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if ensure_model(&app).await.is_err() {
            warn!("dictate: Whisper model not loaded");
        }
    });
}

async fn turn_off<R: Runtime>(_app: &AppHandle<R>) {
    EPOCH.fetch_add(1, Ordering::SeqCst);
    session::send(SessionEvent::Disable);
    let hook = HOOK.lock().unwrap_or_else(PoisonError::into_inner).take();
    let mic = session::take_mic();
    // Stopping the hook and dropping the mic join their threads, so they run off the async runtime.
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(hook) = hook {
            hook.stop();
        }
        drop(mic);
    })
    .await;
    unload_if_idle().await;
}

/// Turns Dictate off when the transcript provider is not Whisper (Dictate only works on Whisper).
pub async fn disable_for_non_whisper<R: Runtime>(app: &AppHandle<R>) {
    let old = settings::current();
    let new = DictateSettings {
        enabled: false,
        ..old.clone()
    };
    if settings::save(app, new.clone()).is_err() {
        warn!("dictate: could not save the switch-off, Dictate stays off until restart");
    }
    apply(app, &old, &new).await;
}

/// Startup: Dictate that was on comes back on once the database is ready.
pub fn apply_after_db_ready<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let new = settings::current();
        if !new.enabled {
            return;
        }
        if !engine_is_whisper(&app).await {
            disable_for_non_whisper(&app).await;
            return;
        }
        apply(&app, &DictateSettings { enabled: false, ..new.clone() }, &new).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unload_rule() {
        assert!(should_unload_now(false, false, false, false));
        assert!(!should_unload_now(true, false, false, false));
        assert!(!should_unload_now(false, true, false, false));
        assert!(!should_unload_now(false, false, true, false));
        assert!(!should_unload_now(false, false, false, true));
    }
}
