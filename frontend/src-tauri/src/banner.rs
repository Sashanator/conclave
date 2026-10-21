//! Conclave's own top-centre meeting banner (spec §5.1 Ask mode). It replaces Windows notifications, which do
//! nothing when clicked (probe check D, 2026-10-13).
use crate::tray::focus_main_window;
use meeting_detector::classify::CallApp;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const LABEL_PREFIX: &str = "meeting-banner-";
const WIDTH: f64 = 460.0;
const HEIGHT: f64 = 72.0;
const TOP_MARGIN: f64 = 16.0;
const AUTO_HIDE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BannerKind {
    Detected { call: CallApp, certain: bool },
    Ended { call: CallApp },
    /// A transcript line that said one of the user's alert words (copilot name alert).
    Mention { line: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BannerView {
    pub kind: &'static str,
    pub app: &'static str,
    pub certain: bool,
    pub line: Option<String>,
}

impl BannerKind {
    fn view(&self) -> BannerView {
        let (kind, app, certain, line) = match self {
            BannerKind::Detected { call, certain } => ("detected", call.label(), *certain, None),
            BannerKind::Ended { call } => ("ended", call.label(), true, None),
            BannerKind::Mention { line } => ("mention", "", true, Some(line.clone())),
        };
        BannerView { kind, app, certain, line }
    }
}

/// The banner on screen: its id (window label suffix) and content. One at a time.
static CURRENT: Mutex<Option<(u64, BannerKind)>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn label_for(id: u64) -> String {
    format!("{LABEL_PREFIX}{id}")
}

fn id_from_label(label: &str) -> Option<u64> {
    label.strip_prefix(LABEL_PREFIX)?.parse().ok()
}

/// Logical top-left position that centres the banner at the top of a monitor (monitor values are physical).
fn top_centre(monitor_x: i32, monitor_y: i32, monitor_width: u32, scale: f64) -> (f64, f64) {
    let (x, y, w) = (monitor_x as f64 / scale, monitor_y as f64 / scale, monitor_width as f64 / scale);
    (x + (w - WIDTH) / 2.0, y + TOP_MARGIN)
}

/// Makes `kind` the current banner and returns its id. The id is allocated under the lock, so two calls never
/// share one and `CURRENT` always holds the later banner.
fn begin(kind: BannerKind) -> u64 {
    let mut current = CURRENT.lock().unwrap();
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    *current = Some((id, kind));
    id
}

/// Removes and returns the banner's content if `label` is still the current banner.
fn take_if_current(label: &str) -> Option<BannerKind> {
    let id = id_from_label(label)?;
    let mut current = CURRENT.lock().unwrap();
    let is_current = matches!(&*current, Some((cur, _)) if *cur == id);
    if is_current {
        current.take().map(|(_, kind)| kind)
    } else {
        None
    }
}

fn close_all<R: Runtime>(app: &AppHandle<R>) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(LABEL_PREFIX) {
            let _ = window.close();
        }
    }
}

/// Shows a banner, replacing any banner already on screen. Never takes keyboard focus from the call app.
pub fn show<R: Runtime>(app: &AppHandle<R>, kind: BannerKind) {
    let id = begin(kind);
    close_all(app);

    let label = label_for(id);
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("banner".into()))
        .title("Conclave")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // Shown with SW_SHOWNOACTIVATE and WS_EX_NOACTIVATE: clicks work, focus stays in the call app.
        .focused(false)
        .focusable(false);
    if let Ok(Some(m)) = app.primary_monitor() {
        let (x, y) = top_centre(m.position().x, m.position().y, m.size().width, m.scale_factor());
        builder = builder.position(x, y);
    }
    if let Err(e) = builder.build() {
        log::error!("Meeting banner: cannot open window: {e}");
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(AUTO_HIDE).await;
        // Clears CURRENT only if this banner is still current. The window closes either way, so it cannot outlive the timer.
        take_if_current(&label);
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.close();
        }
    });
}

#[tauri::command]
pub fn get_meeting_banner<R: Runtime>(window: WebviewWindow<R>) -> Option<BannerView> {
    let id = id_from_label(window.label())?;
    match &*CURRENT.lock().unwrap() {
        Some((cur, kind)) if *cur == id => Some(kind.view()),
        _ => None,
    }
}

/// `action` is "record", "open" or "dismiss". Async: a sync command that touches windows can deadlock on Windows.
#[tauri::command]
pub async fn meeting_banner_action<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    action: String,
) -> Result<(), String> {
    let kind = take_if_current(window.label());
    // Focus before the close: Windows grants foreground only while the click's process still owns a window.
    if action == "open" && matches!(kind, Some(BannerKind::Mention { .. })) {
        focus_main_window(&app);
        let _ = app.emit_to("main", "copilot-open", ());
    }
    let _ = window.close();
    if action == "record" {
        if let Some(BannerKind::Detected { call, .. }) = kind {
            // Starting again would reload the main webview mid-recording and queue a recording nobody asked for.
            if crate::meeting_detector::busy().await {
                log::info!("Meeting banner: Record ignored, a recording is already active");
            } else {
                crate::tray::request_recording_start(&app, Some(call.label()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_round_trip_and_reject_other_windows() {
        assert_eq!(id_from_label(&label_for(42)), Some(42));
        assert_eq!(id_from_label("main"), None);
        assert_eq!(id_from_label("meeting-banner-x"), None);
    }

    #[test]
    fn centred_at_the_top_for_common_monitors() {
        assert_eq!(top_centre(0, 0, 1920, 1.0), (730.0, 16.0));
        assert_eq!(top_centre(0, 0, 3840, 2.0), (730.0, 16.0));
        let (x, y) = top_centre(0, 0, 2560, 1.5);
        assert!((x - 623.333).abs() < 0.01 && y == 16.0);
        assert_eq!(top_centre(-1920, 0, 1920, 1.0), (-1190.0, 16.0));
    }

    /// `CURRENT` is one static shared by the tests below, so they take turns.
    fn current_lock() -> std::sync::MutexGuard<'static, ()> {
        static TURNS: Mutex<()> = Mutex::new(());
        TURNS.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn begin_gives_distinct_ids_and_current_holds_the_later() {
        let _turn = current_lock();
        let first = begin(BannerKind::Ended { call: CallApp::Zoom });
        let second = begin(BannerKind::Ended { call: CallApp::Meet });
        assert_ne!(first, second);
        assert_eq!(CURRENT.lock().unwrap().as_ref().map(|(id, _)| *id), Some(second));
    }

    #[test]
    fn only_the_current_banner_can_act() {
        let _turn = current_lock();
        *CURRENT.lock().unwrap() = Some((7, BannerKind::Ended { call: CallApp::Zoom }));
        assert_eq!(take_if_current(&label_for(6)), None, "stale banner");
        assert!(matches!(take_if_current(&label_for(7)), Some(BannerKind::Ended { call: CallApp::Zoom })));
        assert_eq!(take_if_current(&label_for(7)), None, "acts once");
    }

    #[test]
    fn view_texts() {
        let v = BannerKind::Detected { call: CallApp::Meet, certain: true }.view();
        assert_eq!((v.kind, v.app, v.certain), ("detected", "Google Meet", true));
        assert_eq!(BannerKind::Ended { call: CallApp::Teams }.view().kind, "ended");
    }

    #[test]
    fn mention_view() {
        let v = BannerKind::Mention { line: "x".into() }.view();
        assert_eq!((v.kind, v.line.as_deref()), ("mention", Some("x")));
    }
}
