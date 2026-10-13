//! The detection loop (spec §5.3): wakes on consent-store changes, or every 5 s as a safety net, and feeds
//! snapshots to `Detector`. The app supplies a `Host`.
use crate::classify::{Action, Mode};

/// What the app provides to the loop. Every method is called from the detector thread.
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
fn run<H: Host>(path: &str, host: H) {
    use crate::classify::{classify, CaptureUse, Detected, Detector, Snapshot};
    use crate::consent::is_browser;
    use crate::windows::{capture_sessions, init_com, read_mic_users, window_titles_for, ChangeWatcher};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    const SAFETY_REREAD_MS: u64 = 5_000;

    init_com();
    let watcher = match ChangeWatcher::open(path) {
        Ok(w) => w,
        Err(e) => return host.set_error(Some(format!("Cannot read Windows' microphone usage record: {e}"))),
    };
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

    loop {
        match read_mic_users(path) {
            Ok(users) => {
                report(None);
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
            }
            // Skip the step: an unreadable store must not look like "every app released the mic" (no false stop).
            Err(e) => report(Some(format!("Cannot read Windows' microphone usage record: {e}"))),
        }
        let now = t0.elapsed().as_millis() as u64;
        let wait = detector
            .next_deadline_ms()
            .map_or(SAFETY_REREAD_MS, |d| d.saturating_sub(now).clamp(1, SAFETY_REREAD_MS));
        if watcher.wait(wait as u32).is_err() {
            std::thread::sleep(Duration::from_millis(SAFETY_REREAD_MS));
        }
    }
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
