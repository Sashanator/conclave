pub mod busy;
pub mod github;
pub mod settings;

// Updater driven from Rust only (no webview permission): the token goes as a header to the Conclave asset URLs.

use busy::BusyReason;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

const POLL: Duration = Duration::from_secs(5);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// A stalled connection fails after this long without bytes; the download itself has no total limit.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
const STARTUP_DELAY: Duration = Duration::from_secs(30);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesStatus {
    pub token_last4: Option<String>,
    pub check_on_start: bool,
    pub last_check: Option<settings::LastCheck>,
    pub current_version: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub status: &'static str,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub message: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAvailable {
    pub version: String,
    pub notes: Option<String>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

impl UpdateCheckResult {
    fn plain(status: &'static str) -> Self {
        Self { status, version: None, notes: None, message: None }
    }
    fn error(message: String) -> Self {
        Self { message: Some(message), ..Self::plain("error") }
    }
}

fn token() -> Result<Option<String>, String> {
    crate::secrets::store()
        .get(crate::secrets::GITHUB_UPDATE_TOKEN)
        .map_err(|e| format!("Couldn't read the saved GitHub token: {e}"))
}

async fn find_update<R: Runtime>(
    app: &AppHandle<R>,
    token: &str,
    timeout: Option<Duration>,
) -> Result<Option<Update>, String> {
    let url = github::find_latest_json(token).await?;
    if !github::is_conclave_asset(&url) {
        return Err("The update file points outside the Conclave repository.".into());
    }
    let fail = |e: String| format!("Couldn't read the update file: {}", github::scrub(&e, token));
    let mut b = app
        .updater_builder()
        .configure_client(|c| c.connect_timeout(CONNECT_TIMEOUT).read_timeout(STALL_TIMEOUT))
        // Replaces the plugin's default hook, and Windows install ends the process without RunEvent::Exit:
        // stop the sidecar, flush the DB (bounded), then do Tauri's own cleanup (tray, windows).
        .on_before_exit({
            let app = app.clone();
            move || {
                HOOK_RAN.store(true, Ordering::SeqCst);
                let a = app.clone();
                let h = std::thread::spawn(move || {
                    tauri::async_runtime::block_on(async {
                        if let Err(e) = crate::summary::summary_engine::force_shutdown_sidecar().await {
                            log::error!("updates: sidecar shutdown before install failed: {e}");
                        }
                        if let Some(st) = a.try_state::<crate::state::AppState>() {
                            match tokio::time::timeout(Duration::from_secs(5), st.db_manager.cleanup()).await {
                                Ok(Ok(())) => {}
                                Ok(Err(e)) => log::error!("updates: database cleanup before install failed: {e}"),
                                Err(_) => log::error!("updates: database cleanup before install timed out"),
                            }
                        }
                    })
                });
                if h.join().is_err() {
                    log::error!("updates: cleanup before install panicked");
                }
                app.cleanup_before_exit();
            }
        })
        .endpoints(vec![url])
        .map_err(|e| fail(e.to_string()))?
        .header("Authorization", format!("Bearer {token}"))
        .map_err(|e| fail(e.to_string()))?
        .header("Accept", "application/octet-stream")
        .map_err(|e| fail(e.to_string()))?;
    if let Some(t) = timeout {
        b = b.timeout(t);
    }
    let u = b
        .build()
        .map_err(|e| fail(e.to_string()))?
        .check()
        .await
        .map_err(|e| fail(e.to_string()))?;
    // The token rides on the download too, so its URL gets the same check.
    if u.as_ref().is_some_and(|u| !github::is_conclave_asset(&u.download_url)) {
        return Err("The update file points outside the Conclave repository.".into());
    }
    Ok(u)
}

pub(crate) fn result_text(r: &UpdateCheckResult) -> String {
    match r.status {
        "up_to_date" => "Up to date".into(),
        "available" => format!("Version {} is available", r.version.as_deref().unwrap_or("")),
        _ => r.message.clone().unwrap_or_else(|| "Check failed".into()),
    }
}

pub(crate) fn should_emit_progress(downloaded: u64, last: u64, total: Option<u64>) -> bool {
    const MIB: u64 = 1024 * 1024;
    downloaded / MIB > last / MIB || total == Some(downloaded)
}

/// One waiting task at a time: a repeated check while busy doesn't stack prompts.
pub(crate) async fn announce_once(
    flag: &AtomicBool,
    busy: impl Fn() -> Option<BusyReason>,
    poll: Duration,
    emit: impl FnOnce(),
) {
    if flag.swap(true, Ordering::SeqCst) {
        return;
    }
    busy::wait_until_idle(busy, poll).await;
    emit();
    flag.store(false, Ordering::SeqCst);
}

static ANNOUNCING: AtomicBool = AtomicBool::new(false);
/// The newest announcement; read at emit time so a waiting task never shows an older version.
static LATEST: std::sync::Mutex<Option<UpdateAvailable>> = std::sync::Mutex::new(None);

fn announce<R: Runtime>(app: &AppHandle<R>, version: String, notes: Option<String>) {
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(UpdateAvailable { version, notes });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        announce_once(&ANNOUNCING, busy::current, POLL, || {
            let latest = LATEST.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(u) = latest {
                if let Err(e) = app.emit("update-available", u) {
                    log::warn!("updates: couldn't announce the update: {e}");
                }
            }
        })
        .await;
    });
}

async fn run_check<R: Runtime>(app: &AppHandle<R>) -> UpdateCheckResult {
    let token = match token() {
        Ok(Some(t)) => t,
        Ok(None) => return UpdateCheckResult::plain("not_set_up"),
        Err(m) => {
            settings::record_check(app, &m);
            return UpdateCheckResult::error(m);
        }
    };
    let r = match find_update(app, &token, Some(CHECK_TIMEOUT)).await {
        Ok(Some(u)) => {
            announce(app, u.version.clone(), u.body.clone());
            UpdateCheckResult { version: Some(u.version), notes: u.body, ..UpdateCheckResult::plain("available") }
        }
        Ok(None) => UpdateCheckResult::plain("up_to_date"),
        Err(m) => UpdateCheckResult::error(m),
    };
    settings::record_check(app, &github::scrub(&result_text(&r), &token));
    r
}

#[tauri::command]
pub fn updates_get_status<R: Runtime>(app: AppHandle<R>) -> Result<UpdatesStatus, String> {
    let token_last4 = token()?.map(|t| {
        let n = t.chars().count();
        t.chars().skip(n.saturating_sub(4)).collect()
    });
    Ok(UpdatesStatus {
        token_last4,
        check_on_start: settings::check_on_start(&app)?,
        last_check: settings::last_check(&app)?,
        current_version: app.package_info().version.to_string(),
    })
}

#[tauri::command]
pub fn updates_set_token(token: String) -> Result<(), String> {
    let t = github::validate_token(&token)?;
    crate::secrets::store()
        .set(crate::secrets::GITHUB_UPDATE_TOKEN, &t)
        .map_err(|e| format!("Couldn't save the GitHub token: {e}"))
}

#[tauri::command]
pub fn updates_remove_token() -> Result<(), String> {
    crate::secrets::store()
        .delete(crate::secrets::GITHUB_UPDATE_TOKEN)
        .map_err(|e| format!("Couldn't remove the GitHub token: {e}"))
}

#[tauri::command]
pub fn updates_set_check_on_start<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), String> {
    settings::set_check_on_start(&app, enabled)
}

#[tauri::command]
pub async fn updates_check<R: Runtime>(app: AppHandle<R>) -> Result<UpdateCheckResult, String> {
    Ok(run_check(&app).await)
}

/// Set once the before-exit hook has run: from then on the app is half shut down.
static HOOK_RAN: AtomicBool = AtomicBool::new(false);
static INSTALLING: AtomicBool = AtomicBool::new(false);

struct InstallGuard;
impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::SeqCst);
    }
}

fn busy_err() -> Result<(), String> {
    busy::current().map_or(Ok(()), |r| Err(format!("busy: {r}")))
}

#[tauri::command]
pub async fn updates_install<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("An update is already being installed.".into());
    }
    let _guard = InstallGuard;
    HOOK_RAN.store(false, Ordering::SeqCst);
    busy_err()?;
    let token = token()?.ok_or("Updates are not set up: add a GitHub token in Settings.")?;
    // No timeout: an 80 MB download must not be cut off.
    let update = find_update(&app, &token, None)
        .await?
        .ok_or("Conclave is already up to date.")?;
    let mut downloaded = 0u64;
    let mut last = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if should_emit_progress(downloaded, last, total) {
                    last = downloaded;
                    let _ = app.emit("updates-progress", Progress { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(|e| update_error(&e, &token, "Download failed: "))?;
    // A recording may have started during the download; the bytes are dropped.
    busy_err()?;
    if let Err(e) = update.install(bytes) {
        let m = update_error(&e, &token, "");
        if HOOK_RAN.swap(false, Ordering::SeqCst) {
            // The hook already stopped the sidecar and closed the DB: restart rather than run half shut down.
            let note = format!("Install failed: {}. Conclave restarted.", m.trim_end_matches('.'));
            log::error!("updates: {note}");
            settings::record_check(&app, &github::scrub(&note, &token));
            app.restart();
        }
        return Err(m);
    }
    Ok(())
}

const SIGNATURE_MESSAGE: &str = "The update's signature doesn't match. It was not installed.";

fn update_error(e: &tauri_plugin_updater::Error, token: &str, prefix: &str) -> String {
    use tauri_plugin_updater::Error;
    match e {
        Error::Minisign(_) | Error::SignatureUtf8(_) => SIGNATURE_MESSAGE.to_string(),
        _ => format!("{prefix}{}", github::scrub(&e.to_string(), token)),
    }
}

/// Checks ~30 s after launch, never with a modal. Errors reach the user through `lastCheck`.
pub fn start_check<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        match settings::check_on_start(&app) {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                // Skipped, not defaulted: Settings shows the same error when opened.
                log::warn!("updates: start-up check skipped: {e}");
                return;
            }
        }
        match token() {
            Ok(Some(_)) => {}
            Ok(None) => return,
            Err(e) => {
                log::warn!("updates: start-up check skipped: {e}");
                return;
            }
        }
        let r = run_check(&app).await;
        if r.status == "error" {
            log::warn!("updates: start-up check failed: {}", result_text(&r));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn result_text_cases() {
        assert_eq!(result_text(&UpdateCheckResult::plain("up_to_date")), "Up to date");
        let a = UpdateCheckResult { version: Some("1.2.0".into()), ..UpdateCheckResult::plain("available") };
        assert_eq!(result_text(&a), "Version 1.2.0 is available");
        assert_eq!(result_text(&UpdateCheckResult::error("Nope".into())), "Nope");
    }

    #[test]
    fn progress_is_throttled() {
        const M: u64 = 1024 * 1024;
        assert!(!should_emit_progress(500_000, 0, Some(80 * M)));
        assert!(should_emit_progress(M, 0, None));
        assert!(should_emit_progress(80 * M, 79 * M + M / 2, Some(80 * M)));
    }

    #[test]
    fn status_serializes_camel_case() {
        let s = UpdatesStatus {
            token_last4: None,
            check_on_start: true,
            last_check: None,
            current_version: "1.1.0".into(),
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"tokenLast4":null,"checkOnStart":true,"lastCheck":null,"currentVersion":"1.1.0"})
        );
        assert_eq!(
            serde_json::to_value(UpdateCheckResult::plain("not_set_up")).unwrap(),
            serde_json::json!({"status":"not_set_up","version":null,"notes":null,"message":null})
        );
        assert_eq!(
            serde_json::to_value(Progress { downloaded: 1, total: None }).unwrap(),
            serde_json::json!({"downloaded":1,"total":null})
        );
    }

    #[tokio::test]
    async fn startup_announce_waits_for_idle() {
        let log = RefCell::new(Vec::new());
        let n = Cell::new(0);
        let flag = AtomicBool::new(false);
        announce_once(
            &flag,
            || {
                n.set(n.get() + 1);
                let b = n.get() <= 2;
                log.borrow_mut().push(if b { "busy" } else { "idle" });
                b.then_some(BusyReason::Summary)
            },
            Duration::from_millis(1),
            || log.borrow_mut().push("emit"),
        )
        .await;
        assert_eq!(*log.borrow(), ["busy", "busy", "idle", "emit"]);
        assert!(!flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn announce_is_not_stacked() {
        let emits = Cell::new(0);
        let n = Cell::new(0);
        let flag = AtomicBool::new(false);
        let busy = || {
            n.set(n.get() + 1);
            (n.get() <= 3).then_some(BusyReason::Recording)
        };
        let p = Duration::from_millis(1);
        tokio::join!(
            announce_once(&flag, busy, p, || emits.set(emits.get() + 1)),
            announce_once(&flag, busy, p, || emits.set(emits.get() + 1)),
        );
        assert_eq!(emits.get(), 1);
    }

    #[tokio::test]
    async fn startup_announce_never_emits_while_busy() {
        let emitted = Cell::new(false);
        let flag = AtomicBool::new(false);
        let r = tokio::time::timeout(
            Duration::from_millis(50),
            announce_once(&flag, || Some(BusyReason::Recording), Duration::from_millis(1), || emitted.set(true)),
        )
        .await;
        assert!(r.is_err());
        assert!(!emitted.get());
    }
}
