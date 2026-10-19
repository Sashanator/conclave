//! Dictate's pill (spec §11): a small transparent window at the bottom of the screen that shows the dictation
//! state. It is built again for every dictation, because showing a hidden window would activate it.
use serde::Serialize;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Emitter, EventTarget, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

// Each pill has its own label. The old window's label is freed only when its close is processed, so reusing one
// label would fail to build the next pill whenever the previous one was still closing.
const LABEL_PREFIX: &str = "dictate-pill-";
const EVENT: &str = "dictate-pill";
const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 56.0;
const BOTTOM_MARGIN: f64 = 12.0;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PillState {
    Listening,
    HandsFree,
    LoadingModel,
    Transcribing,
    Pasted,
    PastedAsSpoken,
    Copied,
    NothingHeard,
    NoMicrophone,
}

#[derive(Serialize, Clone, Debug)]
pub struct PillView {
    pub state: PillState,
    pub elapsed_ms: u64,
    pub level: f32,
    pub mode: String,
}

/// `seq` moves on at every call, so a pending `hide_after` acts only if nothing came after it. `label` is the pill
/// on screen. `view` is the last state, handed to the page once its listener is set up.
struct State {
    seq: u64,
    label: Option<String>,
    view: Option<PillView>,
}

static STATE: Mutex<State> = Mutex::new(State {
    seq: 0,
    label: None,
    view: None,
});

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Logical top-left of a pill centred on a work area, `BOTTOM_MARGIN` above its bottom edge. The work area is in
/// physical pixels; the window position is logical.
fn bottom_centre(work_x: i32, work_y: i32, work_w: u32, work_h: u32, scale: f64) -> (f64, f64) {
    let x = work_x as f64 / scale + (work_w as f64 / scale - WIDTH) / 2.0;
    let bottom = (work_y as f64 + work_h as f64) / scale;
    (x, bottom - BOTTOM_MARGIN - HEIGHT)
}

/// The monitor under `near` (physical pixels), else the primary monitor.
fn position<R: Runtime>(app: &AppHandle<R>, near: Option<(i32, i32)>) -> Option<(f64, f64)> {
    let by_point =
        near.and_then(|(x, y)| app.monitor_from_point(x as f64, y as f64).ok().flatten());
    let monitor = by_point.or_else(|| app.primary_monitor().ok().flatten())?;
    let area = monitor.work_area();
    let (x, y) = (area.position.x, area.position.y);
    Some(bottom_centre(
        x,
        y,
        area.size.width,
        area.size.height,
        monitor.scale_factor(),
    ))
}

fn close_windows<R: Runtime>(app: &AppHandle<R>) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(LABEL_PREFIX) {
            let _ = window.close();
        }
    }
}

/// Forgets the pill and closes its windows. The caller holds the lock, so no `show` can run in between.
fn forget<R: Runtime>(app: &AppHandle<R>, st: &mut State) {
    st.seq += 1;
    st.label = None;
    st.view = None;
    close_windows(app);
}

/// Shows a new pill at the bottom of the screen and closes any old one. Never takes focus from the app being
/// dictated into, as the meeting banner.
pub fn show<R: Runtime>(app: &AppHandle<R>, near: Option<(i32, i32)>) {
    let label = {
        let mut st = state();
        st.seq += 1;
        st.view = None;
        let label = format!("{LABEL_PREFIX}{}", st.seq);
        st.label = Some(label.clone());
        close_windows(app);
        label
    };
    let mut builder =
        WebviewWindowBuilder::new(app, &label, WebviewUrl::App("dictate-pill".into()))
            .inner_size(WIDTH, HEIGHT)
            .transparent(true)
            .shadow(false)
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false);
    if let Some((x, y)) = position(app, near) {
        builder = builder.position(x, y);
    }
    if let Err(e) = builder.build() {
        log::error!("Dictate pill: cannot open window: {e}");
    }
}

pub fn close<R: Runtime>(app: &AppHandle<R>) {
    forget(app, &mut state());
}

/// Sends the state to the pill window only.
pub fn update<R: Runtime>(app: &AppHandle<R>, view: &PillView) {
    let mut st = state();
    st.seq += 1;
    st.view = Some(view.clone());
    if let Some(label) = &st.label {
        if let Err(e) = app.emit_to(EventTarget::webview_window(label), EVENT, view) {
            log::warn!("Dictate pill: cannot send state: {e}");
        }
    }
}

/// Closes the pill after `ms`, unless another call (show, update, close or hide_after) came first.
pub fn hide_after<R: Runtime>(app: &AppHandle<R>, ms: u64) {
    let seq = {
        let mut st = state();
        st.seq += 1;
        st.seq
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(ms)).await;
        let mut st = state();
        if st.seq == seq {
            forget(&app, &mut st);
        }
    });
}

/// The page asks for the current state once its `dictate-pill` listener is set up. Events sent before that are lost.
#[tauri::command]
pub fn dictate_pill_view() -> Option<PillView> {
    state().view.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_and_view_use_the_names_the_page_expects() {
        let states = [
            PillState::Listening,
            PillState::HandsFree,
            PillState::LoadingModel,
            PillState::Transcribing,
            PillState::Pasted,
            PillState::PastedAsSpoken,
            PillState::Copied,
            PillState::NothingHeard,
            PillState::NoMicrophone,
        ];
        let names: Vec<String> = states
            .iter()
            .map(|s| serde_json::to_value(s).unwrap().as_str().unwrap().into())
            .collect();
        assert_eq!(
            names,
            [
                "listening",
                "hands-free",
                "loading-model",
                "transcribing",
                "pasted",
                "pasted-as-spoken",
                "copied",
                "nothing-heard",
                "no-microphone"
            ]
        );
        let view = PillView {
            state: PillState::HandsFree,
            elapsed_ms: 42000,
            level: 0.5,
            mode: "cleaned-up".into(),
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["state"], "hands-free");
        assert_eq!(json["elapsed_ms"], 42000);
        assert_eq!(json["mode"], "cleaned-up");
    }

    #[test]
    fn centred_twelve_px_above_the_work_area_bottom() {
        // 1080p with a 40 px taskbar: work area 1920x1040 at the top.
        assert_eq!(bottom_centre(0, 0, 1920, 1040, 1.0), (780.0, 972.0));
        // 4K at 200%: the same logical work area.
        assert_eq!(bottom_centre(0, 0, 3840, 2080, 2.0), (780.0, 972.0));
        // A monitor left of the primary, with a 48 px taskbar at the top.
        assert_eq!(bottom_centre(-1920, 48, 1920, 1032, 1.0), (-1140.0, 1012.0));
        // 1440p at 150%, physical work area 2560x1400.
        let (x, y) = bottom_centre(0, 0, 2560, 1400, 1.5);
        assert!(
            (x - 673.333).abs() < 0.01 && (y - 865.333).abs() < 0.01,
            "got ({x}, {y})"
        );
    }
}
