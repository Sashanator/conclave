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
use crate::consent::is_browser;
#[cfg(windows)]
use crate::windows::{capture_sessions, init_com, read_mic_users, window_titles_for, ChangeWatcher};
#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::io;
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
    let mut last_error: Option<Option<String>> = None;
    let mut report = |err: Option<String>| {
        if last_error.as_ref() != Some(&err) {
            host.set_error(err.clone());
            last_error = Some(err);
        }
    };

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
        let stepped = match catch_unwind(AssertUnwindSafe(|| pass(path, &host, t0, &mut detector, &mut titles))) {
            Ok(Ok(())) => {
                report(None);
                true
            }
            // Skip the step: an unreadable store must not look like "every app released the mic" (no false stop).
            Ok(Err(e)) => {
                report(Some(format!("Cannot read Windows' microphone usage record: {e}")));
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
        if let Err(e) = watcher.wait(wait as u32) {
            report(Some(format!("Cannot watch Windows' microphone usage record: {e}")));
            std::thread::sleep(Duration::from_millis(wait));
            // Keep the old watcher if the key cannot be re-opened; the next wait fails and retries.
            if let Ok(w) = ChangeWatcher::open(path) {
                watcher = w;
            }
        }
    }
}

/// One pass: read the store, classify it, and step the detector. A read error leaves the detector untouched.
#[cfg(windows)]
fn pass<H: Host>(
    path: &str,
    host: &H,
    t0: Instant,
    detector: &mut Detector,
    titles: &mut HashMap<String, Vec<String>>,
) -> io::Result<()> {
    let users = read_mic_users(path)?;
    titles.retain(|p, _| users.iter().any(|u| u.in_use && &u.path == p));
    for u in users.iter().filter(|u| u.in_use && is_browser(&u.app)) {
        titles.entry(u.path.clone()).or_insert_with(|| window_titles_for(&u.app));
    }
    let in_use: Vec<Detected> = users
        .iter()
        .filter_map(|u| classify(u, titles.get(&u.path).map(Vec::as_slice).unwrap_or(&[])))
        .collect();
    let recording = host.is_recording();
    let sessions = if recording {
        capture_sessions().ok().map(|list| {
            list.into_iter()
                .filter(|c| c.active)
                .filter_map(|c| Some(CaptureUse { exe: c.process_name?, device: c.device_name }))
                .collect()
        })
    } else {
        None
    };
    let snapshot = Snapshot {
        now_ms: t0.elapsed().as_millis() as u64,
        mode: host.mode(),
        recording,
        recording_id: host.recording_id(),
        in_use,
        sessions,
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
