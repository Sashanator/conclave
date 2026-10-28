//! The detection loop (spec §5.3): wakes on consent-store changes, or every 5 s as a safety net, and feeds
//! snapshots to `Detector`. The app supplies a `Host`.
use crate::classify::{Action, Mode};

/// What the app provides to the loop. Every method is called from the detector thread.
/// Methods should not panic; a panic skips that pass and is reported via set_error.
pub trait Host: Send + 'static {
    fn mode(&self) -> Mode;
    fn is_recording(&self) -> bool;
    /// Number of recordings started so far (any counter that increments per start).
    fn recording_id(&self) -> u64;
    /// The previous meeting is still being saved (see `Snapshot::busy`).
    fn is_busy(&self) -> bool;
    /// The recording's current microphone name, if known. Only asked while recording.
    fn current_mic(&self) -> Option<String>;
    fn apply(&self, action: Action);
    /// `Some(reason)` while detection cannot run; `None` once it runs.
    fn set_error(&self, reason: Option<String>);
}

/// Starts the detector thread. On non-Windows targets it reports "Windows only" and starts nothing.
pub fn spawn<H: Host>(host: H) -> std::io::Result<()> {
    spawn_at(crate::consent::CONSENT_PATH.to_string(), host)
}

#[cfg(not(windows))]
fn spawn_at<H: Host>(_path: String, host: H) -> std::io::Result<()> {
    host.set_error(Some("Meeting detection works on Windows only.".to_string()));
    Ok(())
}

#[cfg(windows)]
fn spawn_at<H: Host>(path: String, host: H) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("meeting-detector".into())
        .spawn(move || run(&path, host))
        .map(|_| ())
}

#[cfg(windows)]
use crate::classify::{classify, CaptureUse, Detected, Detector, Snapshot};
#[cfg(windows)]
use crate::consent::{is_browser, merge_mic_users};
#[cfg(windows)]
use crate::windows::{capture_sessions, init_com, read_mic_users, window_titles_for, ChangeWatcher};
#[cfg(windows)]
use std::collections::{BTreeMap, HashMap};
#[cfg(windows)]
use std::panic::{catch_unwind, AssertUnwindSafe};
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
const SAFETY_REREAD_MS: u64 = 5_000;

/// Never returns: every failure is reported through `set_error` and retried.
#[cfg(windows)]
fn run<H: Host>(path: &str, host: H) {
    init_com();
    let t0 = Instant::now();
    let mut detector = Detector::new();
    // Browser titles are read once, when the browser starts using the mic (spec §5.3).
    let mut titles: HashMap<String, Vec<String>> = HashMap::new();
    let mut last_detected = BTreeMap::new();
    let mut last_error: Option<Option<String>> = None;
    let mut report = |err: Option<String>| {
        if last_error.as_ref() != Some(&err) {
            // A panicking host must not end the detector thread; the next change retries.
            if catch_unwind(AssertUnwindSafe(|| host.set_error(err.clone()))).is_ok() {
                last_error = Some(err);
            }
        }
    };
    // A failing watcher stays reported even while reads succeed, so the status doesn't flip every pass.
    let mut watch_error: Option<String> = None;

    let mut watcher = loop {
        match ChangeWatcher::open(path) {
            Ok(w) => break w,
            Err(e) => {
                report(Some(format!("Cannot read Windows' microphone usage record: {e}")));
                std::thread::sleep(Duration::from_millis(SAFETY_REREAD_MS));
            }
        }
    };

    loop {
        let stepped = match catch_unwind(AssertUnwindSafe(|| pass(path, &host, t0, &mut detector, &mut titles, &mut last_detected))) {
            Ok(Ok(())) => {
                report(watch_error.clone());
                true
            }
            // Skip the step: an unreadable store or session scan must not look like "every app released the mic" (no false stop).
            Ok(Err(e)) => {
                report(Some(e));
                false
            }
            Err(_) => {
                report(Some("Meeting detection hit an internal error and keeps retrying.".to_string()));
                false
            }
        };
        let now = t0.elapsed().as_millis() as u64;
        let wait = match detector.next_deadline_ms() {
            Some(d) if stepped => d.saturating_sub(now).clamp(1, SAFETY_REREAD_MS),
            _ => SAFETY_REREAD_MS,
        };
        let waited = Instant::now();
        match watcher.wait(wait as u32) {
            Ok(_) => watch_error = None,
            Err(e) => {
                watch_error = Some(format!("Cannot watch Windows' microphone usage record: {e}"));
                report(watch_error.clone());
                // Sleep only what is left of this wait, so a late failure doesn't double it.
                std::thread::sleep(Duration::from_millis(wait).saturating_sub(waited.elapsed()));
                // Keep the old watcher if the key cannot be re-opened; the next wait fails and retries.
                if let Ok(w) = ChangeWatcher::open(path) {
                    watcher = w;
                }
            }
        }
    }
}

/// One pass: read both mic-usage sources, classify, and step. Either read failing leaves the detector untouched.
#[cfg(windows)]
fn pass<H: Host>(
    path: &str,
    host: &H,
    t0: Instant,
    detector: &mut Detector,
    titles: &mut HashMap<String, Vec<String>>,
    last_detected: &mut BTreeMap<String, (bool, bool)>,
) -> Result<(), String> {
    let registry = read_mic_users(path)
        .map_err(|e| format!("Cannot read Windows' microphone usage record: {e}"))?;
    // Normalize source identities too, so attribution uses the same stable keys as the detector.
    let registry = merge_mic_users(registry, []);
    // Scan even before recording. A top-level scan failure skips the detector step.
    let capture = capture_sessions()
        .map_err(|e| format!("Cannot list apps using the microphone: {e}"))?;
    let session_users = merge_mic_users(
        vec![],
        capture.iter().filter(|c| c.active)
            .filter_map(|c| Some((c.process_path.as_deref()?, true))),
    );
    let users = merge_mic_users(
        registry.clone(),
        session_users.iter().map(|u| (u.path.as_str(), u.in_use)),
    );
    titles.retain(|p, _| users.iter().any(|u| u.in_use && &u.path == p));
    for u in users.iter().filter(|u| u.in_use && is_browser(&u.app)) {
        titles.entry(u.path.clone()).or_insert_with(|| window_titles_for(&u.app));
    }
    let in_use: Vec<Detected> = users
        .iter()
        .filter_map(|u| classify(u, titles.get(&u.path).map(Vec::as_slice).unwrap_or(&[])))
        .collect();
    // Aggregate by exe for stable, privacy-safe logs: no paths or window/meeting titles.
    let mut detected = BTreeMap::<String, (bool, bool)>::new();
    for call in &in_use {
        let sources = detected.entry(call.exe.to_lowercase()).or_default();
        sources.0 |= registry.iter().any(|u| u.in_use && u.path == call.path);
        sources.1 |= session_users.iter().any(|u| u.path == call.path
            || (u.app == "ms-teams.exe" && call.exe == "ms-teams.exe"));
    }
    if &detected != last_detected {
        let apps: Vec<String> = detected.iter().map(|(exe, sources)| {
            let source = match sources {
                (true, true) => "registry/session",
                (true, false) => "registry",
                _ => "session",
            };
            format!("{exe} ({source})")
        }).collect();
        log::info!("Meeting detector call apps: [{}]", apps.join(", "));
        *last_detected = detected;
    }
    let recording = host.is_recording();
    let sessions = if recording {
        Some(capture.into_iter()
            .filter(|c| c.active)
            .filter_map(|c| Some(CaptureUse { exe: c.process_name?, device: c.device_name }))
            .collect())
    } else {
        None
    };
    let current_mic = if recording { host.current_mic() } else { None };
    let snapshot = Snapshot {
        now_ms: t0.elapsed().as_millis() as u64,
        mode: host.mode(),
        recording,
        recording_id: host.recording_id(),
        in_use,
        sessions,
        current_mic,
        busy: host.is_busy(),
    };
    for action in detector.step(&snapshot) {
        host.apply(action);
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::classify::CallApp;
    use std::sync::mpsc;
    use std::time::Duration;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    struct FakeHost(mpsc::Sender<Action>);

    impl Host for FakeHost {
        fn mode(&self) -> Mode {
            Mode::Ask
        }
        fn is_recording(&self) -> bool {
            false
        }
        fn recording_id(&self) -> u64 {
            0
        }
        fn is_busy(&self) -> bool {
            false
        }
        fn current_mic(&self) -> Option<String> {
            None
        }
        fn apply(&self, action: Action) {
            let _ = self.0.send(action);
        }
        fn set_error(&self, _reason: Option<String>) {}
    }

    #[test]
    fn zoom_starting_to_use_the_mic_prompts_promptly() {
        let root = r"Software\ConclaveTest\runner";
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all(root);
        let path = format!(r"{root}\microphone");
        let (store, _) = hkcu.create_subkey(&path).unwrap();
        let (tx, rx) = mpsc::channel();
        spawn_at(path, FakeHost(tx)).unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let (zoom, _) = store.create_subkey(r"NonPackaged\C:#Zoom#bin#Zoom.exe").unwrap();
        zoom.set_value("LastUsedTimeStart", &100u64).unwrap();
        zoom.set_value("LastUsedTimeStop", &0u64).unwrap();

        let got = rx.recv_timeout(Duration::from_secs(3));
        drop(zoom);
        drop(store);
        let _ = hkcu.delete_subkey_all(root); // the detector thread then idles on read errors until the process exits
        assert_eq!(got.unwrap(), Action::Prompt { app: CallApp::Zoom, certain: true });
    }
}
