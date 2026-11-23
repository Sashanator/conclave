//! Dictate's pill (spec §11): a small transparent window at the bottom of the screen that shows the dictation
//! state. It is built again for every dictation, because showing a hidden window would activate it.
use super::settings::LiveTextStyle;
use serde::Serialize;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, Runtime, WebviewUrl,
    WebviewWindowBuilder,
};

// Each pill has its own label. The old window's label is freed only when its close is processed, so reusing one
// label would fail to build the next pill whenever the previous one was still closing.
const LABEL_PREFIX: &str = "dictate-pill-";
const EVENT: &str = "dictate-pill";
// The panel opens at the no-text size and grows to PANEL_WIDTH with the first text, so no wider transparent strip
// sits above the taskbar. The line style is one fixed wide window.
const WIDTH: f64 = 360.0;
const PANEL_WIDTH: f64 = 480.0;
const LINE_WIDTH: f64 = 560.0;
const HEIGHT: f64 = 56.0;
const PANEL_MAX_HEIGHT: f64 = 140.0;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volatile: Option<String>,
    /// Set by `pill` from the style given to `show`; the caller's value is overwritten.
    pub style: LiveTextStyle,
}

impl PillView {
    pub fn new(state: PillState, mode: &str) -> Self {
        Self {
            state,
            elapsed_ms: 0,
            level: 0.0,
            mode: mode.to_string(),
            committed: None,
            volatile: None,
            style: LiveTextStyle::Panel,
        }
    }
}

/// `seq` moves on at every call, so a pending `hide_after` acts only if nothing came after it. `label` is the pill
/// on screen. `view` is the last state, handed to the page once its listener is set up.
struct State {
    seq: u64,
    label: Option<String>,
    view: Option<PillView>,
    style: LiveTextStyle,
}

static STATE: Mutex<State> = Mutex::new(State {
    seq: 0,
    label: None,
    view: None,
    style: LiveTextStyle::Panel,
});

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Logical top-left of a pill centred on a work area, `BOTTOM_MARGIN` above its bottom edge. The work area is in
/// physical pixels; the window position is logical.
fn bottom_centre(
    work_x: i32,
    work_y: i32,
    work_w: u32,
    work_h: u32,
    scale: f64,
    width: f64,
    height: f64,
) -> (f64, f64) {
    let x = work_x as f64 / scale + (work_w as f64 / scale - width) / 2.0;
    let bottom = (work_y as f64 + work_h as f64) / scale;
    (x, bottom - BOTTOM_MARGIN - height)
}

/// Opening size of the window for a style.
fn window_size(style: LiveTextStyle) -> (f64, f64) {
    match style {
        LiveTextStyle::Panel => (WIDTH, HEIGHT),
        LiveTextStyle::Line => (LINE_WIDTH, HEIGHT),
    }
}

/// Panel height from the page; NaN, infinity and out-of-range values clamp to the limits.
fn panel_height(h: f64) -> f64 {
    if h.is_nan() {
        HEIGHT
    } else {
        h.clamp(HEIGHT, PANEL_MAX_HEIGHT)
    }
}

/// Panel width is one of two steps: the no-text card or the text panel.
fn panel_width(w: f64) -> f64 {
    if w.is_finite() && w >= (WIDTH + PANEL_WIDTH) / 2.0 {
        PANEL_WIDTH
    } else {
        WIDTH
    }
}

/// The monitor under `near` (physical pixels), else the primary monitor.
fn position<R: Runtime>(app: &AppHandle<R>, near: Option<(i32, i32)>,
    size: (f64, f64),
) -> Option<(f64, f64)> {
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
        size.0,
        size.1,
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
pub fn show<R: Runtime>(app: &AppHandle<R>, near: Option<(i32, i32)>, style: LiveTextStyle) {
    let label = {
        let mut st = state();
        st.seq += 1;
        st.view = None;
        st.style = style;
        let label = format!("{LABEL_PREFIX}{}", st.seq);
        st.label = Some(label.clone());
        close_windows(app);
        label
    };
    let size = window_size(style);
    let mut builder =
        WebviewWindowBuilder::new(app, &label, WebviewUrl::App("dictate-pill".into()))
            .inner_size(size.0, size.1)
            .transparent(true)
            .shadow(false)
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false);
    if let Some((x, y)) = position(app, near, size) {
        builder = builder.position(x, y);
    }
    match builder.build() {
        Ok(window) => {
            // A close or a newer show may have run during the build, before this window was
            // registered. Then it is stale.
            if state().label.as_deref() != Some(label.as_str()) {
                let _ = window.close();
            }
        }
        Err(e) => log::error!("Dictate pill: cannot open window: {e}"),
    }
}

pub fn close<R: Runtime>(app: &AppHandle<R>) {
    forget(app, &mut state());
}

/// Sends the state to the pill window only.
pub fn update<R: Runtime>(app: &AppHandle<R>, view: &PillView) {
    let mut st = state();
    st.seq += 1;
    let mut view = view.clone();
    view.style = st.style;
    if let Some(old) = &st.view {
        keep_text(&mut view, old);
    }
    st.view = Some(view.clone());
    if let Some(label) = &st.label {
        if let Err(e) = app.emit_to(EventTarget::webview_window(label), EVENT, &view) {
            log::warn!("Dictate pill: cannot send state: {e}");
        }
    }
}

/// A tick or state change that carries no text keeps the text already shown, except in the states that end
/// without a transcript (nothing heard, errors), where old words would be stale.
fn keep_text(view: &mut PillView, old: &PillView) {
    if matches!(view.state, PillState::NothingHeard | PillState::NoMicrophone) {
        view.committed = Some(String::new());
        view.volatile = Some(String::new());
        return;
    }
    view.committed = view.committed.take().or_else(|| old.committed.clone());
    view.volatile = view.volatile.take().or_else(|| old.volatile.clone());
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

/// The page sizes the window to its text. The bottom edge stays put and the window is re-centred, so it grows
/// upward. The line style is one fixed window and ignores this.
#[tauri::command]
pub fn dictate_pill_resize<R: Runtime>(app: AppHandle<R>, width: f64, height: f64) {
    let label = {
        let st = state();
        if st.style == LiveTextStyle::Line {
            return;
        }
        match &st.label {
            Some(l) => l.clone(),
            None => return,
        }
    };
    let Some(window) = app.get_webview_window(&label) else { return };
    let (Ok(pos), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.outer_size(),
        window.scale_factor(),
    ) else {
        log::warn!("Dictate pill: cannot read the window geometry to resize");
        return;
    };
    let w = panel_width(width);
    let h = panel_height(height);
    let centre = (pos.x as f64 + size.width as f64 / 2.0) / scale;
    let bottom = (pos.y as f64 + size.height as f64) / scale;
    if let Err(e) = window.set_size(LogicalSize::new(w, h)) {
        log::warn!("Dictate pill: cannot resize: {e}");
    }
    if let Err(e) = window.set_position(LogicalPosition::new(centre - w / 2.0, bottom - h)) {
        log::warn!("Dictate pill: cannot move after resize: {e}");
    }
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
            elapsed_ms: 42000,
            level: 0.5,
            ..PillView::new(PillState::HandsFree, "cleaned-up")
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["state"], "hands-free");
        assert_eq!(json["elapsed_ms"], 42000);
        assert_eq!(json["mode"], "cleaned-up");
        assert_eq!(json["style"], "panel");
        assert!(json.get("committed").is_none() && json.get("volatile").is_none());
    }

    #[test]
    fn centred_twelve_px_above_the_work_area_bottom() {
        // 1080p with a 40 px taskbar: work area 1920x1040 at the top.
        assert_eq!(bottom_centre(0, 0, 1920, 1040, 1.0, 360.0, 56.0), (780.0, 972.0));
        // 4K at 200%: the same logical work area.
        assert_eq!(bottom_centre(0, 0, 3840, 2080, 2.0, 360.0, 56.0), (780.0, 972.0));
        // A monitor left of the primary, with a 48 px taskbar at the top.
        assert_eq!(bottom_centre(-1920, 48, 1920, 1032, 1.0, 360.0, 56.0), (-1140.0, 1012.0));
        // 1440p at 150%, physical work area 2560x1400.
        let (x, y) = bottom_centre(0, 0, 2560, 1400, 1.5, 360.0, 56.0);
        assert!(
            (x - 673.333).abs() < 0.01 && (y - 865.333).abs() < 0.01,
            "got ({x}, {y})"
        );
    }

    #[test]
    fn panel_height_is_clamped() {
        assert_eq!(panel_height(f64::NAN), 56.0);
        assert_eq!(panel_height(f64::INFINITY), 140.0);
        assert_eq!(panel_height(f64::NEG_INFINITY), 56.0);
        assert_eq!(panel_height(0.0), 56.0);
        assert_eq!(panel_height(10_000.0), 140.0);
        assert_eq!(panel_height(100.0), 100.0);
    }

    #[test]
    fn panel_width_is_one_of_two_steps() {
        assert_eq!(panel_width(f64::NAN), 360.0);
        assert_eq!(panel_width(f64::INFINITY), 360.0);
        assert_eq!(panel_width(100.0), 360.0);
        assert_eq!(panel_width(360.0), 360.0);
        assert_eq!(panel_width(480.0), 480.0);
        assert_eq!(panel_width(9999.0), 480.0);
    }

    #[test]
    fn stale_text_is_cleared_when_nothing_was_heard() {
        let mut old = PillView::new(PillState::Transcribing, "m");
        old.committed = Some("hello".into());
        old.volatile = Some("wor".into());
        let mut v = PillView::new(PillState::NothingHeard, "m");
        keep_text(&mut v, &old);
        assert_eq!((v.committed.as_deref(), v.volatile.as_deref()), (Some(""), Some("")));
        let mut v = PillView::new(PillState::NoMicrophone, "m");
        keep_text(&mut v, &old);
        assert_eq!(v.committed.as_deref(), Some(""));
        let mut v = PillView::new(PillState::Pasted, "m");
        keep_text(&mut v, &old);
        assert_eq!((v.committed.as_deref(), v.volatile.as_deref()), (Some("hello"), Some("wor")));
    }

    #[test]
    fn sizes_per_style() {
        assert_eq!(window_size(LiveTextStyle::Panel), (360.0, 56.0));
        assert_eq!(window_size(LiveTextStyle::Line), (560.0, 56.0));
        // A 480-wide panel is centred and its bottom stays 12 px above the work area.
        assert_eq!(bottom_centre(0, 0, 1920, 1040, 1.0, 480.0, 56.0), (720.0, 972.0));
    }
}
