//! App side of meeting detection (spec §5): the Off/Ask/Auto setting, the status Settings shows, and what each
//! detector action does. The detection loop lives in `crates/meeting-detector`.
use crate::banner::{self, BannerKind};
use meeting_detector::classify::{Action, Mode};
use meeting_detector::runner::Host;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Listener, Runtime};

/// Ask until the saved preference loads.
static MODE: AtomicU8 = AtomicU8::new(1);
static ERROR: Mutex<Option<String>> = Mutex::new(None);
/// Counts starts so the detector can tell two recordings apart: a stop and a new start can both fall between two of its steps.
static RECORDINGS_STARTED: AtomicU64 = AtomicU64::new(0);

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
        RECORDINGS_STARTED.load(Ordering::Relaxed)
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
        // Mic following arrives in Task 6.
        Action::FollowMic { .. } | Action::MicUnknown { .. } => {}
    }
}

/// Loads the saved mode, then starts the detector thread.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    app.listen_any("recording-started", |_| {
        RECORDINGS_STARTED.fetch_add(1, Ordering::Relaxed);
    });
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
    fn stored_mode_round_trips_and_unknown_is_ask() {
        for (s, m) in [("off", Mode::Off), ("ask", Mode::Ask), ("auto", Mode::Auto), ("??", Mode::Ask)] {
            set_mode(s);
            assert_eq!(mode(), m, "{s}");
        }
        set_mode("ask");
    }
}
