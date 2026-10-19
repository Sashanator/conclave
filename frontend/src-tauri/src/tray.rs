use tauri::{
    Emitter,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};

use crate::dictate::cleanup::{mode_name, resolve_mode};
use crate::dictate::session;
use crate::dictate::settings::{self, DictateSettings, AS_SPOKEN_ID, CLEANED_UP_ID};

use std::sync::atomic::{AtomicBool, Ordering};

const DICTATE_COPY_LAST: &str = "dictate_copy_last";
const DICTATE_COPY_LAST_RAW: &str = "dictate_copy_last_raw";
const DICTATE_MODE_PREFIX: &str = "dictate_mode:";

/// One pending tray re-check while a save shows "Stopping and saving".
static SAVE_RECHECK: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone)]
pub enum RecordingState {
    Stopped,
    Starting,
    Recording,
    Pausing,
    Paused,
    Resuming,
    Stopping,
}

pub fn create_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    // Start with default menu, will update with actual state after initialization
    // Pass can_record=true initially, will be updated by update_tray_menu immediately
    let menu = build_menu(app, RecordingState::Stopped, true)?;

    TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("Conclave")
        .icon(app.default_window_icon().unwrap().clone())
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .build(app)?;

    // Update tray menu with actual recording state after creation
    update_tray_menu(app);

    Ok(())
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, item_id: &str) {
    if let Some(mode) = item_id.strip_prefix(DICTATE_MODE_PREFIX) {
        set_dictate_mode(app, mode);
        return;
    }
    match item_id {
        DICTATE_COPY_LAST => copy_last_dictation(false),
        DICTATE_COPY_LAST_RAW => copy_last_dictation(true),
        "toggle_recording" => toggle_recording_handler(app),
        "pause_recording" => pause_recording_handler(app),
        "resume_recording" => resume_recording_handler(app),
        "stop_recording" => stop_recording_handler(app),
        "open_window" => focus_main_window(app),
        "settings" => {
            focus_main_window(app);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.eval("window.location.assign('/settings')");
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}
fn toggle_recording_handler<R: Runtime>(app: &AppHandle<R>) {
    focus_main_window(app);
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if crate::is_recording().await {
            // Immediately show stopping state
            set_tray_state(&app_clone, RecordingState::Stopping);

            stop_recording_and_save(&app_clone).await;
        } else {
            log::info!("Emitting start recording event from tray");
            request_recording_start(&app_clone, None);
        }
    });
}

/// Starts recording through the main webview's normal start path (model check, recording notice), without
/// bringing the window forward. `title_prefix` (a call app name) goes in front of the generated meeting title.
pub fn request_recording_start<R: Runtime>(app: &AppHandle<R>, title_prefix: Option<&str>) {
    // An automatic start must never reload the page over a live, starting or saving recording.
    if crate::audio::recording_commands::is_recording_now()
        || crate::audio::recording_commands::start_in_flight()
        || crate::meeting_detector::post_processing()
    {
        log::info!("Start ignored: a recording is starting, running or saving");
        return;
    }
    set_tray_state(app, RecordingState::Starting);
    if let Some(window) = app.get_webview_window("main") {
        // JSON-encode so the prefix is always a safe JS string literal.
        let prefix = serde_json::to_string(title_prefix.unwrap_or("")).unwrap_or_else(|_| "\"\"".to_string());
        let _ = window.eval(&format!(
            "sessionStorage.setItem('autoStartTitlePrefix', {prefix}); sessionStorage.setItem('autoStartRecording', 'true'); window.location.assign('/')"
        ));
    }
}

fn pause_recording_handler<R: Runtime>(app: &AppHandle<R>) {
    // Immediately show pausing state
    set_tray_state(app, RecordingState::Pausing);

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::audio::recording_commands::pause_recording(app_clone.clone()).await {
            log::error!("Failed to pause recording from tray: {}", e);
            // Revert to current state on error
            update_tray_menu_async(&app_clone).await;
        } else {
            log::info!("Recording paused from tray");
            // The pause_recording function will call update_tray_menu, so no need to call it here
        }
    });
}

fn resume_recording_handler<R: Runtime>(app: &AppHandle<R>) {
    // Immediately show resuming state
    set_tray_state(app, RecordingState::Resuming);

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::audio::recording_commands::resume_recording(app_clone.clone()).await
        {
            log::error!("Failed to resume recording from tray: {}", e);
            // Revert to current state on error
            update_tray_menu_async(&app_clone).await;
        } else {
            log::info!("Recording resumed from tray");
            // The resume_recording function will call update_tray_menu, so no need to call it here
        }
    });
}

fn stop_recording_handler<R: Runtime>(app: &AppHandle<R>) {
    // Immediately show stopping state
    set_tray_state(app, RecordingState::Stopping);

    focus_main_window(app);
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move { stop_recording_and_save(&app_clone).await });
}

/// Stops the recording and hands it to the frontend's save flow via `recording-stop-complete` (works from any page).
pub async fn stop_recording_and_save<R: Runtime>(app: &AppHandle<R>) {
    log::info!("Stop: Stopping recording...");

    // Generate save path (same as RecordingControls.tsx)
    let data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("Stop: Failed to get app data dir: {}", e);
            update_tray_menu_async(app).await;
            return;
        }
    };

    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();
    let save_path = data_dir.join(format!("recording-{}.wav", timestamp));

    // Call Rust stop_recording command (like pause/resume pattern)
    let stop_result = crate::audio::recording_commands::stop_recording(
        app.clone(),
        crate::audio::recording_commands::RecordingArgs {
            save_path: save_path.to_string_lossy().to_string(),
        },
    )
    .await;

    // Handle result
    match stop_result {
        Ok(_) => {
            log::info!("Stop: Recording stopped successfully");

            // Trigger frontend post-processing via event (works from any page)
            // (SQLite save, navigation)
            if let Err(e) = app.emit("recording-stop-complete", true) {
                log::error!("Stop: Failed to emit recording-stop-complete event: {}", e);
            }
        }
        Err(e) => {
            log::error!("Stop: Failed to stop recording: {}", e);
            // Revert tray state on error
            update_tray_menu_async(app).await;
        }
    }
}

pub fn update_tray_menu<R: Runtime>(app: &AppHandle<R>) {
    // For sync update, spawn async task to get current state
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        // Small delay to ensure recording state has been updated
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        update_tray_menu_async(&app_clone).await;
    });
}

pub fn set_tray_state<R: Runtime>(app: &AppHandle<R>, state: RecordingState) {
    log::info!("Tray: Setting intermediate state: {:?}", state);
    // During recording state transitions, we assume recording is allowed (we're already recording)
    if let Ok(menu) = build_menu(app, state, true) {
        if let Some(tray) = app.tray_by_id("main-tray") {
            let result = tray.set_menu(Some(menu));
            log::info!("Tray: Intermediate state menu update result: {:?}", result);
        } else {
            log::warn!("Tray: Could not find tray with id 'main-tray'");
        }
    } else {
        log::error!("Tray: Failed to build menu for intermediate state");
    }
}

async fn get_current_recording_state() -> RecordingState {
    // Check if currently recording
    let is_recording = crate::audio::recording_commands::is_recording().await;
    log::info!(
        "Tray: get_current_recording_state - is_recording: {}",
        is_recording
    );

    if !is_recording {
        // A Start during the previous meeting's save would be ignored, so show it as still stopping.
        if crate::meeting_detector::post_processing() {
            return RecordingState::Stopping;
        }
        log::info!("Tray: Recording state is Stopped");
        return RecordingState::Stopped;
    }

    // Check if paused
    let is_paused = crate::audio::recording_commands::is_recording_paused().await;
    log::info!("Tray: is_paused: {}", is_paused);

    if is_paused {
        log::info!("Tray: Recording state is Paused");
        RecordingState::Paused
    } else {
        log::info!("Tray: Recording state is Recording");
        RecordingState::Recording
    }
}

/// Check if recording is allowed based on onboarding status and transcription model availability
/// Returns true if:
/// - Onboarding is complete (user may prefer Whisper later), OR
/// - Parakeet transcription model is ready (downloaded)
async fn check_can_record<R: Runtime>(app: &AppHandle<R>) -> bool {
    // First check if onboarding is complete
    let onboarding_complete = match crate::onboarding::load_onboarding_status(app).await {
        Ok(status) => status.completed,
        Err(e) => {
            log::warn!("Tray: Failed to load onboarding status: {}, assuming complete", e);
            true // Assume complete if we can't check (safe default)
        }
    };

    // If onboarding is complete, always allow recording
    // (user may prefer Whisper or have their own transcription setup)
    if onboarding_complete {
        return true;
    }

    // During onboarding, check if Parakeet transcription model is ready
    match crate::parakeet_engine::commands::parakeet_has_available_models().await {
        Ok(has_models) => has_models,
        Err(e) => {
            log::warn!("Tray: Failed to check Parakeet models: {}, assuming not ready", e);
            false
        }
    }
}

pub async fn update_tray_menu_async<R: Runtime>(app: &AppHandle<R>) {
    log::info!("Tray: update_tray_menu_async called");
    // Get the current recording state
    let recording_state = get_current_recording_state().await;
    log::info!("Tray: Current recording state: {:?}", recording_state);

    // Determine if recording should be allowed
    // Only block recording during incomplete onboarding when no transcription model is ready
    let can_record = check_can_record(app).await;
    log::info!("Tray: can_record: {}", can_record);

    // "Stopping and saving" comes from the save flag, which can expire without being cleared: re-check until it's gone.
    if matches!(recording_state, RecordingState::Stopping) && !SAVE_RECHECK.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            SAVE_RECHECK.store(false, Ordering::SeqCst);
            update_tray_menu(&app);
        });
    }

    if let Ok(menu) = build_menu(app, recording_state, can_record) {
        if let Some(tray) = app.tray_by_id("main-tray") {
            let result = tray.set_menu(Some(menu));
            log::info!("Tray: Menu update result: {:?}", result);
        } else {
            log::warn!("Tray: Could not find tray with id 'main-tray'");
        }
    } else {
        log::error!("Tray: Failed to build menu");
    }
}

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    state: RecordingState,
    can_record: bool, // True if recording is allowed (onboarding complete OR transcription model ready)
) -> tauri::Result<tauri::menu::Menu<R>> {
    let mut builder = MenuBuilder::new(app);

    // If recording is not allowed (during onboarding, no transcription model), show disabled message
    if !can_record {
        builder = builder.item(
            &MenuItemBuilder::new("⏳ Downloading transcription model...")
                .enabled(false)
                .build(app)?,
        );
    } else {
        match state {
            RecordingState::Stopped => {
                builder = builder
                    .item(&MenuItemBuilder::with_id("toggle_recording", "Start Recording").build(app)?);
            }
            RecordingState::Starting => {
                builder = builder.item(
                    &MenuItemBuilder::new("🔄 Starting Recording...")
                        .enabled(false)
                        .build(app)?,
                );
            }
            RecordingState::Recording => {
                builder = builder
                    .item(&MenuItemBuilder::with_id("pause_recording", "⏸ Pause Recording").build(app)?)
                    .item(&MenuItemBuilder::with_id("stop_recording", "⏹ Stop Recording").build(app)?);
            }
            RecordingState::Pausing => {
                builder = builder
                    .item(
                        &MenuItemBuilder::new("⏸ Pausing...")
                            .enabled(false)
                            .build(app)?,
                    )
                    .item(&MenuItemBuilder::with_id("stop_recording", "⏹ Stop Recording").build(app)?);
            }
            RecordingState::Paused => {
                builder = builder
                    .item(
                        &MenuItemBuilder::with_id("resume_recording", "▶ Resume Recording")
                            .build(app)?,
                    )
                    .item(&MenuItemBuilder::with_id("stop_recording", "⏹ Stop Recording").build(app)?);
            }
            RecordingState::Resuming => {
                builder = builder
                    .item(
                        &MenuItemBuilder::new("▶ Resuming...")
                            .enabled(false)
                            .build(app)?,
                    )
                    .item(&MenuItemBuilder::with_id("stop_recording", "⏹ Stop Recording").build(app)?);
            }
            RecordingState::Stopping => {
                builder = builder.item(
                    &MenuItemBuilder::new("⏹ Stopping and saving...")
                        .enabled(false)
                        .build(app)?,
                );
            }
        }
    }

    let spec = dictate_menu_spec(&settings::current(), session::last_dictation().is_some());
    if !spec.is_empty() {
        let modes = Submenu::new(app, "Dictate mode", true)?;
        let mut copies = Vec::new();
        for (id, label, enabled, checked) in &spec {
            if id.starts_with(DICTATE_MODE_PREFIX) {
                modes.append(
                    &CheckMenuItemBuilder::with_id(id.as_str(), label.as_str())
                        .enabled(*enabled)
                        .checked(*checked)
                        .build(app)?,
                )?;
            } else {
                copies.push(
                    MenuItemBuilder::with_id(id.as_str(), label.as_str())
                        .enabled(*enabled)
                        .build(app)?,
                );
            }
        }
        builder = builder.item(&PredefinedMenuItem::separator(app)?);
        for item in &copies {
            builder = builder.item(item);
        }
        builder = builder.item(&modes);
    }

    builder
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::with_id("open_window", "Open Main Window").build(app)?)
        .item(&MenuItemBuilder::with_id("settings", "Settings").build(app)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::with_id("quit", "Quit").build(app)?)
        .build()
}

/// The Dictate tray rows (id, label, enabled, checked): the two copy items, then one mode per mode, in order.
/// Empty while Dictate is off. The mode rows are the ones with the `dictate_mode:` prefix.
fn dictate_menu_spec(s: &DictateSettings, has_last: bool) -> Vec<(String, String, bool, bool)> {
    if !s.enabled {
        return Vec::new();
    }
    let mut rows = vec![
        (DICTATE_COPY_LAST.to_string(), "Copy last dictation".to_string(), has_last, false),
        (
            DICTATE_COPY_LAST_RAW.to_string(),
            "Copy last dictation (as spoken)".to_string(),
            has_last,
            false,
        ),
    ];
    let ids = [AS_SPOKEN_ID, CLEANED_UP_ID]
        .into_iter()
        .map(str::to_string)
        .chain(s.modes.iter().map(|m| m.id.clone()));
    for id in ids {
        let label = mode_name(&resolve_mode(s, &id));
        let checked = id == s.default_mode;
        rows.push((format!("{DICTATE_MODE_PREFIX}{id}"), label, true, checked));
    }
    rows
}

/// Copies the last dictation to the clipboard. The exclusion formats keep it out of clipboard history.
/// The text is never logged.
fn copy_last_dictation(as_spoken: bool) {
    let Some(last) = session::last_dictation() else {
        return;
    };
    let text = if as_spoken { last.as_spoken } else { last.final_text };
    // OpenClipboard can retry for a while, so the write stays off the tray's event thread.
    tauri::async_runtime::spawn_blocking(move || {
        if dictate_input::clipboard::set_private_text(&text, None).is_err() {
            log::warn!("Tray: could not copy the last dictation");
        }
    });
}

fn set_dictate_mode<R: Runtime>(app: &AppHandle<R>, mode: &str) {
    let app = app.clone();
    let mode = mode.to_string();
    tauri::async_runtime::spawn(async move {
        if session::set_default_mode(&app, &mode).await.is_err() {
            log::warn!("Tray: could not switch the Dictate mode to {mode}");
            // The native check mark already moved on click; rebuild to show the real default.
            update_tray_menu(&app);
        }
    });
}

pub(crate) fn focus_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.unminimize() {
            log::error!("Failed to unminimize main window: {}", e);
        }

        if let Err(e) = window.show() {
            log::error!("Failed to show main window: {}", e);
        }

        if let Err(e) = window.set_focus() {
            log::error!("Failed to focus main window: {}", e);
        }

        if let Err(e) = window.eval("window.focus()") {
            log::error!("Failed to focus main webview: {}", e);
        }
    } else {
        log::warn!("Could not find main window");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictate::settings::{CustomMode, DictateSettings};

    fn row(id: &str, label: &str, enabled: bool, checked: bool) -> (String, String, bool, bool) {
        (id.to_string(), label.to_string(), enabled, checked)
    }

    fn on() -> DictateSettings {
        DictateSettings {
            enabled: true,
            ..DictateSettings::default()
        }
    }

    #[test]
    fn dictate_off_adds_nothing() {
        assert!(dictate_menu_spec(&DictateSettings::default(), true).is_empty());
        assert!(dictate_menu_spec(&DictateSettings::default(), false).is_empty());
    }

    #[test]
    fn copy_items_disabled_without_a_last_dictation() {
        let spec = dictate_menu_spec(&on(), false);
        assert_eq!(spec[0], row("dictate_copy_last", "Copy last dictation", false, false));
        assert_eq!(
            spec[1],
            row("dictate_copy_last_raw", "Copy last dictation (as spoken)", false, false)
        );
    }

    #[test]
    fn copy_items_enabled_with_a_last_dictation() {
        let spec = dictate_menu_spec(&on(), true);
        assert!(spec[0].2);
        assert!(spec[1].2);
    }

    #[test]
    fn mode_items_are_checked_only_on_the_default_mode() {
        let mut s = on();
        s.default_mode = "cleaned-up".to_string();
        let modes: Vec<(String, bool)> = dictate_menu_spec(&s, true)
            .into_iter()
            .filter(|r| r.0.starts_with("dictate_mode:"))
            .map(|r| (r.0, r.3))
            .collect();
        assert_eq!(
            modes,
            vec![
                ("dictate_mode:as-spoken".to_string(), false),
                ("dictate_mode:cleaned-up".to_string(), true),
                ("dictate_mode:prompt-for-claude".to_string(), false),
            ]
        );
    }

    #[test]
    fn custom_modes_follow_the_built_ins_in_order_with_their_names() {
        let mut s = on();
        s.default_mode = "as-spoken".to_string();
        s.modes = vec![
            CustomMode { id: "b-mode".into(), name: "Second".into(), instructions: String::new(), examples: vec![], hotkey: None },
            CustomMode { id: "a-mode".into(), name: "First".into(), instructions: String::new(), examples: vec![], hotkey: None },
        ];
        let labels: Vec<(String, String, bool)> = dictate_menu_spec(&s, true)
            .into_iter()
            .filter(|r| r.0.starts_with("dictate_mode:"))
            .map(|r| (r.0, r.1, r.3))
            .collect();
        assert_eq!(
            labels,
            vec![
                ("dictate_mode:as-spoken".to_string(), "As spoken".to_string(), true),
                ("dictate_mode:cleaned-up".to_string(), "Cleaned up".to_string(), false),
                ("dictate_mode:b-mode".to_string(), "Second".to_string(), false),
                ("dictate_mode:a-mode".to_string(), "First".to_string(), false),
            ]
        );
    }
}
