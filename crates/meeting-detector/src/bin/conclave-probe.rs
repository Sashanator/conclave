//! Pre-flight diagnostic for spec §5.4. Read-only: prints mic usage, wake reasons and capture sessions.
#[cfg(not(windows))]
fn main() {
    eprintln!("conclave-probe only runs on Windows");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    use meeting_detector::consent::{is_browser, CONSENT_PATH};
    use meeting_detector::windows::*;
    use std::collections::{BTreeSet, HashMap};
    use std::time::Instant;

    let t0 = Instant::now();
    let log = |msg: String| println!("[{:>8.3}s] {msg}", t0.elapsed().as_secs_f64());

    init_com();
    let watcher = match ChangeWatcher::open(CONSENT_PATH) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Cannot open the microphone consent store (HKCU\\{CONSENT_PATH}): {e}");
            std::process::exit(2);
        }
    };
    println!("conclave-probe {} — read-only. Join/mute/leave calls and switch mics; press Ctrl+C to quit.", env!("CARGO_PKG_VERSION"));
    println!("Legend: via=notify means Windows sent a change notification; via=timeout means it was only caught by the 5 s re-read.\n");

    let mut prev: HashMap<String, bool> = HashMap::new();
    let mut prev_sessions: BTreeSet<String> = BTreeSet::new();
    let mut first = true;
    loop {
        let via = match watcher.wait(5000) {
            Ok(Wake::Changed) => "notify",
            Ok(Wake::Timeout) => "timeout",
            Err(e) => {
                log(format!("ERROR waiting for change: {e}"));
                std::thread::sleep(std::time::Duration::from_secs(5));
                "error-retry"
            }
        };
        match read_mic_users(CONSENT_PATH) {
            Ok(users) => {
                for u in users {
                    let was = prev.insert(u.app.clone(), u.in_use).unwrap_or(false);
                    if u.in_use != was && !(first && !u.in_use) {
                        let kind = if u.packaged { "packaged" } else { "desktop" };
                        log(format!("MIC {} {} ({kind}) via={via}", if u.in_use { "START" } else { "STOP " }, u.app));
                        if u.in_use && is_browser(&u.app) {
                            for t in window_titles_for(&u.app) {
                                log(format!("    window title: {t}"));
                            }
                        }
                    }
                }
            }
            Err(e) => log(format!("ERROR reading consent store: {e}")),
        }
        match capture_sessions() {
            Ok(sessions) => {
                let now: BTreeSet<String> = sessions
                    .iter()
                    .filter(|s| s.active)
                    .map(|s| format!("{} [{}] ← {} (pid {})", s.device_name, s.device_id, s.process_name.as_deref().unwrap_or("?"), s.process_id))
                    .collect();
                if now != prev_sessions {
                    log("ACTIVE CAPTURE SESSIONS:".to_string());
                    if now.is_empty() {
                        log("    (none)".to_string());
                    }
                    for s in &now {
                        log(format!("    {s}"));
                    }
                    prev_sessions = now;
                }
            }
            Err(e) => log(format!("ERROR listing capture sessions: {e}")),
        }
        first = false;
    }
}
