//! App side of meeting detection (spec §5): the Off/Ask/Auto setting, the status Settings shows, and what each
//! detector action does. The detection loop lives in `crates/meeting-detector`.
use crate::banner::{self, BannerKind};
use meeting_detector::classify::{Action, Mode};
use meeting_detector::runner::Host;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Runtime};

/// Ask until the saved preference loads.
static MODE: AtomicU8 = AtomicU8::new(1);
static ERROR: Mutex<Option<String>> = Mutex::new(None);
/// Set while the frontend saves the meeting that just stopped (`handleRecordingStop`). Stopping clears IS_RECORDING
/// before that save finishes, so a start in between would reload the page and lose the save.
static POST_PROCESSING_SINCE: Mutex<Option<Instant>> = Mutex::new(None);
/// Expiry: a crashed or reloaded frontend can never block detection forever.
const POST_PROCESSING_MAX: Duration = Duration::from_secs(600);

fn encode(m: Mode) -> u8 {
    match m {
        Mode::Off => 0,
        Mode::Ask => 1,
        Mode::Auto => 2,
    }
}

fn decode(v: u8) -> Mode {
    match v {
        0 => Mode::Off,
        2 => Mode::Auto,
        _ => Mode::Ask,
    }
}

/// Takes the stored value ("off", "ask" or "auto").
pub fn set_mode(stored: &str) {
    MODE.store(encode(Mode::parse(stored)), Ordering::Relaxed);
}

pub fn mode() -> Mode {
    decode(MODE.load(Ordering::Relaxed))
}

#[tauri::command]
pub fn set_recording_post_processing(active: bool) {
    *POST_PROCESSING_SINCE.lock().unwrap_or_else(|e| e.into_inner()) = active.then(Instant::now);
}

fn is_fresh(since: Option<Instant>, now: Instant) -> bool {
    since.is_some_and(|t| now.saturating_duration_since(t) < POST_PROCESSING_MAX)
}

pub fn post_processing() -> bool {
    is_fresh(*POST_PROCESSING_SINCE.lock().unwrap_or_else(|e| e.into_inner()), Instant::now())
}

/// True while a recording runs or its save is still going: nothing may start or reload the page then.
pub async fn busy() -> bool {
    crate::audio::recording_commands::is_recording().await || post_processing()
}

/// `None` while detection runs; otherwise the reason it doesn't, shown in Settings.
#[tauri::command]
pub fn get_meeting_detection_status() -> Option<String> {
    ERROR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

struct AppHost<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> Host for AppHost<R> {
    fn mode(&self) -> Mode {
        mode()
    }

    fn is_recording(&self) -> bool {
        tauri::async_runtime::block_on(crate::audio::recording_commands::is_recording())
    }

    fn recording_id(&self) -> u64 {
        crate::audio::recording_commands::recordings_started()
    }

    fn is_busy(&self) -> bool {
        post_processing()
    }

    fn apply(&self, action: Action) {
        apply(&self.app, action)
    }

    fn set_error(&self, reason: Option<String>) {
        match &reason {
            Some(r) => log::warn!("Meeting detection unavailable: {r}"),
            None => log::info!("Meeting detection running"),
        }
        *ERROR.lock().unwrap_or_else(|e| e.into_inner()) = reason;
    }
}

fn apply<R: Runtime>(app: &AppHandle<R>, action: Action) {
    log::info!("Meeting detection: {action:?}");
    match action {
        Action::Prompt { app: call, certain } => banner::show(app, BannerKind::Detected { call, certain }),
        Action::Start { app: call } => crate::tray::request_recording_start(app, Some(call.label())),
        Action::Stop { app: call } => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move { crate::tray::stop_recording_and_save(&handle).await });
            banner::show(app, BannerKind::Ended { call });
        }
        Action::FollowMic { app: call, device } => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::audio::recording_commands::follow_mic(handle.clone(), device.clone()).await {
                    Ok(()) => {
                        let _ = handle.emit("mic-following", serde_json::json!({ "device": device, "app": call.label() }));
                    }
                    Err(e) => {
                        log::warn!("Mic following: cannot switch to {device}: {e}");
                        let _ = handle.emit("mic-following-warning", serde_json::json!({
                            "message": format!("Couldn't switch to {device}, the microphone {} is using. Recording continues on the current microphone.", call.label())
                        }));
                    }
                }
            });
        }
        Action::MicUnknown { app: call } => {
            let _ = app.emit("mic-following-warning", serde_json::json!({
                "message": format!("Couldn't tell which microphone {} is using. Recording continues on the current microphone.", call.label())
            }));
        }
    }
}

/// Loads the saved mode, then starts the detector thread.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match crate::audio::recording_preferences::load_recording_preferences(&app).await {
            Ok(p) => set_mode(&p.meeting_detection),
            Err(e) => log::warn!("Meeting detection: preferences unreadable, using Ask: {e}"),
        }
        if let Err(e) = meeting_detector::runner::spawn(AppHost { app }) {
            *ERROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("Cannot start meeting detection: {e}"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_processing_flag_follows_the_command() {
        set_recording_post_processing(true);
        assert!(post_processing());
        set_recording_post_processing(false);
        assert!(!post_processing());
    }

    #[test]
    fn post_processing_is_busy_until_it_expires() {
        let t = Instant::now();
        assert!(!is_fresh(None, t));
        assert!(is_fresh(Some(t), t));
        assert!(is_fresh(Some(t), t + Duration::from_secs(599)));
        assert!(!is_fresh(Some(t), t + Duration::from_secs(601)), "a stale flag must not block forever");
    }

    #[test]
    fn stored_mode_round_trips_and_unknown_is_ask() {
        for (s, m) in [("off", Mode::Off), ("ask", Mode::Ask), ("auto", Mode::Auto), ("??", Mode::Ask)] {
            set_mode(s);
            assert_eq!(mode(), m, "{s}");
        }
        set_mode("ask");
    }
}
