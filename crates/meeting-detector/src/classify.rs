//! Pure meeting-detection rules (spec §5.1, §5.2). No Win32 calls: the runner feeds in snapshots, so every
//! decision is unit-tested on any OS.
use crate::consent::{is_browser, MicUser};
use std::collections::HashSet;

/// A detected call app that releases the mic for this long ends its recording (covers headset switches).
pub const STOP_GRACE_MS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Ask,
    Auto,
}

impl Mode {
    /// Stored values are "off", "ask" and "auto"; anything else is the default, Ask.
    pub fn parse(s: &str) -> Mode {
        match s {
            "off" => Mode::Off,
            "auto" => Mode::Auto,
            _ => Mode::Ask,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallApp {
    Zoom,
    Teams,
    Meet,
    Browser,
}

impl CallApp {
    pub fn label(self) -> &'static str {
        match self {
            CallApp::Zoom => "Zoom",
            CallApp::Teams => "Teams",
            CallApp::Meet => "Google Meet",
            CallApp::Browser => "Browser",
        }
    }
}

/// An app using the mic that is, or may be, a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// Identity: full exe path, or the package family name for packaged apps.
    pub path: String,
    /// Lowercase exe name of the process that owns the app's audio sessions.
    pub exe: String,
    pub app: CallApp,
    /// False for a browser whose window titles name no known call service.
    pub certain: bool,
}

/// Google Meet tabs read "Meet - <code>" or name Google Meet; a bare "Meet" also matches "Meetup" and "Meeting notes".
fn is_meet_title(t: &str) -> bool {
    t.starts_with("Meet - ")
        || t.starts_with("Meet – ")
        || t.starts_with("Meet — ")
        || t.contains("Google Meet")
        || t.contains("meet.google.com")
}

/// Classifies one consent-store entry. `titles` are the browser's window titles (ignored for other apps).
/// Anything that is not a call app (Conclave itself, OBS, Notion…) gives `None`.
pub fn classify(user: &MicUser, titles: &[String]) -> Option<Detected> {
    if !user.in_use {
        return None;
    }
    let (app, certain, exe) = if user.packaged {
        if !user.app.starts_with("msteams_") {
            return None;
        }
        (CallApp::Teams, true, "ms-teams.exe")
    } else if user.app == "zoom.exe" {
        (CallApp::Zoom, true, "zoom.exe")
    } else if user.app == "ms-teams.exe" {
        (CallApp::Teams, true, "ms-teams.exe")
    } else if is_browser(&user.app) {
        // Teams first: Teams meeting titles ("Meeting with … | Microsoft Teams") also contain "Meet".
        if titles.iter().any(|t| t.contains("Microsoft Teams")) {
            (CallApp::Teams, true, user.app.as_str())
        } else if titles.iter().any(|t| is_meet_title(t)) {
            (CallApp::Meet, true, user.app.as_str())
        } else {
            (CallApp::Browser, false, user.app.as_str())
        }
    } else {
        return None;
    };
    Some(Detected { path: user.path.clone(), exe: exe.to_string(), app, certain })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Show the banner (Ask mode, or any uncertain detection).
    Prompt { app: CallApp, certain: bool },
    /// Start recording at once (Auto mode, certain detection).
    Start { app: CallApp },
    /// The attached app released the mic for `STOP_GRACE_MS`: stop, then show "meeting ended".
    Stop { app: CallApp },
    /// Record from this capture device: the attached app's.
    FollowMic { app: CallApp, device: String },
    /// The attached app's capture device can't be determined: keep the current mic and show a warning.
    MicUnknown { app: CallApp },
}

/// One active capture session: the owning process's lowercase exe name and the device's friendly name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureUse {
    pub exe: String,
    pub device: String,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Monotonic milliseconds.
    pub now_ms: u64,
    pub mode: Mode,
    pub recording: bool,
    /// Increments each time a recording starts; lets the detector see a stop and a new start that both happen between two steps.
    pub recording_id: u64,
    /// Call apps using the mic now (the output of `classify`).
    pub in_use: Vec<Detected>,
    /// Active capture sessions, read only while recording; `None` when not read.
    pub sessions: Option<Vec<CaptureUse>>,
    /// The recording's current microphone name, if known.
    pub current_mic: Option<String>,
    /// The previous meeting is still being saved: offer and start nothing until it finishes.
    pub busy: bool,
}

#[derive(Debug, Default)]
pub struct Detector {
    /// Apps already offered, or seen during a recording, while they hold the mic. Cleared on release.
    seen: HashSet<String>,
    /// The app most recently prompted or auto-started. A recording that starts while it holds the mic attaches to it.
    offered: Option<String>,
    attached: Option<Detected>,
    released_at: Option<u64>,
    was_recording: bool,
    last_recording_id: u64,
    /// The device the attached app's audio should be on: set once a follow was sent or the recording reached it.
    follow_target: Option<String>,
    /// FollowMic actions sent for `follow_target` while the recording was not on it.
    follow_attempts: u8,
    /// When the last FollowMic was sent. A retry waits `STOP_GRACE_MS` so a switch still running is not raced.
    follow_last_ms: u64,
    /// The previous step was Off: turning detection back on mid-recording attaches again.
    was_off: bool,
    warned: bool,
    /// Set by a Stop until the recording ends: a call rejoined while the save runs is offered afterwards.
    stopping: bool,
}

impl Detector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attached(&self) -> Option<&Detected> {
        self.attached.as_ref()
    }

    /// When a pending stop falls due, so the runner wakes for it instead of waiting the full 5 s.
    pub fn next_deadline_ms(&self) -> Option<u64> {
        self.released_at.map(|t| t + STOP_GRACE_MS)
    }

    pub fn step(&mut self, s: &Snapshot) -> Vec<Action> {
        let held = |path: &str| s.in_use.iter().any(|d| d.path == path);
        self.seen.retain(|p| held(p));
        if self.offered.as_deref().is_some_and(|p| !held(p)) {
            self.offered = None;
        }

        let started = s.recording && (!self.was_recording || s.recording_id != self.last_recording_id);
        let back_on = self.was_off && s.mode != Mode::Off;
        self.was_off = s.mode == Mode::Off;
        self.was_recording = s.recording;
        self.last_recording_id = s.recording_id;
        if started || !s.recording {
            self.stopping = false;
        }
        if started || s.mode == Mode::Off || !s.recording {
            self.detach();
        }
        if s.mode == Mode::Off {
            return Vec::new();
        }

        if s.recording {
            // The save after an auto-stop still counts as recording: nothing is seen until it ends.
            if self.stopping {
                return Vec::new();
            }
            // A manual start during a detected call attaches too (spec §5.1, Stop), and so does Off -> on mid-recording.
            if started || back_on {
                // An uncertain browser is kept only when no certain call is in use.
                self.attached = self
                    .offered
                    .as_ref()
                    .and_then(|p| s.in_use.iter().find(|d| &d.path == p))
                    .filter(|d| d.certain || !s.in_use.iter().any(|x| x.certain))
                    .or_else(|| s.in_use.iter().find(|d| d.certain))
                    .cloned();
            }
            // New detections are ignored while recording, and not offered once it ends.
            self.seen.extend(s.in_use.iter().map(|d| d.path.clone()));
            return self.while_recording(s);
        }

        if s.busy {
            return Vec::new();
        }

        // Offer at most one app per step, certain ones first; the rest count as seen.
        let mut fresh: Vec<&Detected> = s.in_use.iter().filter(|d| !self.seen.contains(&d.path)).collect();
        fresh.sort_by_key(|d| !d.certain);
        self.seen.extend(fresh.iter().map(|d| d.path.clone()));
        let Some(d) = fresh.first() else { return Vec::new() };
        self.offered = Some(d.path.clone());
        vec![if s.mode == Mode::Auto && d.certain {
            Action::Start { app: d.app }
        } else {
            Action::Prompt { app: d.app, certain: d.certain }
        }]
    }

    fn detach(&mut self) {
        self.attached = None;
        self.released_at = None;
        self.follow_target = None;
        self.follow_attempts = 0;
        self.warned = false;
    }

    fn while_recording(&mut self, s: &Snapshot) -> Vec<Action> {
        let Some(att) = self.attached.clone() else { return Vec::new() };
        if !s.in_use.iter().any(|d| d.path == att.path) {
            let since = *self.released_at.get_or_insert(s.now_ms);
            if s.now_ms.saturating_sub(since) >= STOP_GRACE_MS {
                self.detach();
                self.stopping = true;
                return vec![Action::Stop { app: att.app }];
            }
            return Vec::new();
        }
        self.released_at = None;

        let Some(sessions) = &s.sessions else { return Vec::new() };
        let mut devices: Vec<&str> =
            sessions.iter().filter(|c| c.exe == att.exe).map(|c| c.device.as_str()).collect();
        devices.sort_unstable();
        devices.dedup();
        match devices.as_slice() {
            [one] => {
                let device = one.to_string();
                if s.current_mic.as_deref() == Some(*one) {
                    self.follow_target = Some(device);
                    self.follow_attempts = 0;
                    Vec::new()
                } else if self.follow_target.as_deref() != Some(*one) {
                    // A new target. The device may be listed only after it is plugged in, so retries follow.
                    self.follow_target = Some(device.clone());
                    self.follow_attempts = 1;
                    self.follow_last_ms = s.now_ms;
                    vec![Action::FollowMic { app: att.app, device }]
                } else if self.follow_attempts < 3 && s.now_ms.saturating_sub(self.follow_last_ms) >= STOP_GRACE_MS {
                    self.follow_attempts += 1;
                    self.follow_last_ms = s.now_ms;
                    vec![Action::FollowMic { app: att.app, device }]
                } else {
                    Vec::new()
                }
            }
            // No session or several: keep the current mic, and warn once if no device was ever found.
            _ if self.follow_target.is_none() && !self.warned => {
                self.warned = true;
                vec![Action::MicUnknown { app: att.app }]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(app: &str, path: &str, packaged: bool) -> MicUser {
        MicUser { app: app.into(), path: path.into(), packaged, in_use: true }
    }

    fn zoom() -> Detected {
        Detected { path: r"C:\Zoom\bin\Zoom.exe".into(), exe: "zoom.exe".into(), app: CallApp::Zoom, certain: true }
    }

    fn browser() -> Detected {
        Detected { path: r"C:\Chrome\chrome.exe".into(), exe: "chrome.exe".into(), app: CallApp::Browser, certain: false }
    }

    fn snap(now_ms: u64, mode: Mode, recording: bool, in_use: Vec<Detected>) -> Snapshot {
        Snapshot { now_ms, mode, recording, recording_id: 0, in_use, sessions: None, current_mic: None, busy: false }
    }

    fn with_id(mut s: Snapshot, recording_id: u64) -> Snapshot {
        s.recording_id = recording_id;
        s
    }

    fn with_busy(mut s: Snapshot, busy: bool) -> Snapshot {
        s.busy = busy;
        s
    }

    fn with_current_mic(mut s: Snapshot, mic: &str) -> Snapshot {
        s.current_mic = Some(mic.to_string());
        s
    }

    fn with_sessions(mut s: Snapshot, sessions: &[(&str, &str)]) -> Snapshot {
        s.sessions = Some(sessions.iter().map(|(e, d)| CaptureUse { exe: e.to_string(), device: d.to_string() }).collect());
        s
    }

    /// Prompts for Zoom, then starts a recording that attaches to it (at t=1000).
    fn recording_attached_to_zoom() -> Detector {
        let mut d = Detector::new();
        d.step(&snap(0, Mode::Ask, false, vec![zoom()]));
        d.step(&snap(1000, Mode::Ask, true, vec![zoom()]));
        assert_eq!(d.attached(), Some(&zoom()));
        d
    }

    #[test]
    fn session_only_zoom_prompts_and_stops_after_release_grace() {
        let detected = |active| crate::consent::merge_mic_users(
            vec![], [(r"C:\Zoom\bin\Zoom.exe", active)],
        ).iter().filter_map(|u| classify(u, &[])).collect();
        let mut d = Detector::new();
        assert_eq!(d.step(&snap(0, Mode::Ask, false, detected(true))),
            vec![Action::Prompt { app: CallApp::Zoom, certain: true }]);
        assert!(d.step(&with_id(snap(1000, Mode::Ask, true, detected(true)), 1)).is_empty());
        assert_eq!(d.attached().unwrap().app, CallApp::Zoom);
        assert!(d.step(&with_id(snap(2000, Mode::Ask, true, detected(false)), 1)).is_empty());
        assert_eq!(d.step(&with_id(snap(7000, Mode::Ask, true, detected(false)), 1)),
            vec![Action::Stop { app: CallApp::Zoom }]);
    }

    #[test]
    fn registry_casing_change_does_not_stop_session_only_zoom() {
        let detected = |registry| crate::consent::merge_mic_users(
            registry, [(r"C:\Zoom\bin\Zoom.exe", true)],
        ).iter().filter_map(|u| classify(u, &[])).collect::<Vec<_>>();
        let mut d = Detector::new();
        assert_eq!(d.step(&snap(0, Mode::Ask, false, detected(vec![]))),
            vec![Action::Prompt { app: CallApp::Zoom, certain: true }]);
        assert!(d.step(&with_id(snap(1000, Mode::Ask, true, detected(vec![])), 1)).is_empty());
        let users = detected(vec![MicUser {
            app: "zoom.exe".into(), path: r"c:\zoom\bin\zoom.exe".into(), packaged: false, in_use: true,
        }]);
        for now in [2000, 7000, 12000] {
            let actions = d.step(&with_id(snap(now, Mode::Ask, true, users.clone()), 1));
            assert!(!actions.iter().any(|a| matches!(a, Action::Stop { .. })),
                "path casing caused a false Stop at {now}: {actions:?}");
            assert_eq!(d.attached().unwrap().app, CallApp::Zoom);
        }
        let gone = crate::consent::merge_mic_users(vec![], []);
        let gone = gone.iter().filter_map(|u| classify(u, &[])).collect::<Vec<_>>();
        assert!(d.step(&with_id(snap(13000, Mode::Ask, true, gone.clone()), 1)).is_empty());
        assert_eq!(d.step(&with_id(snap(18000, Mode::Ask, true, gone), 1)),
            vec![Action::Stop { app: CallApp::Zoom }]);
    }

    #[test]
    fn opera_is_classified_as_a_browser() {
        let opera = user("opera.exe", r"C:\Opera GX\opera.exe", false);
        let d = classify(&opera, &[]).unwrap();
        assert_eq!((d.app, d.certain, d.exe.as_str()), (CallApp::Browser, false, "opera.exe"));
        assert_eq!(classify(&opera, &["Meet - abc-defg-hij - Opera".into()]).unwrap().app, CallApp::Meet);
    }

    #[test]
    fn zoom_is_a_certain_zoom_call() {
        let d = classify(&user("zoom.exe", r"C:\Zoom\bin\Zoom.exe", false), &[]).unwrap();
        assert_eq!((d.app, d.certain, d.exe.as_str()), (CallApp::Zoom, true, "zoom.exe"));
    }

    #[test]
    fn packaged_and_desktop_teams_are_teams() {
        let d = classify(&user("msteams_8wekyb3d8bbwe", "MSTeams_8wekyb3d8bbwe", true), &[]).unwrap();
        assert_eq!((d.app, d.certain, d.exe.as_str()), (CallApp::Teams, true, "ms-teams.exe"));
        let d = classify(&user("ms-teams.exe", r"C:\Teams\ms-teams.exe", false), &[]).unwrap();
        assert_eq!(d.app, CallApp::Teams);
    }

    #[test]
    fn browser_titles_decide_meet_or_teams() {
        let chrome = user("chrome.exe", r"C:\Chrome\chrome.exe", false);
        let meet = classify(&chrome, &["Meet - abc-defg-hij - Google Chrome".into()]).unwrap();
        assert_eq!((meet.app, meet.certain, meet.exe.as_str()), (CallApp::Meet, true, "chrome.exe"));
        let teams = classify(&chrome, &["Meeting with Anna | Microsoft Teams".into()]).unwrap();
        assert_eq!(teams.app, CallApp::Teams, "Teams wins over the 'Meet' in 'Meeting'");
        for title in [
            "Meet – abc-defg-hij - Google Chrome",
            "Meet — abc-defg-hij - Google Chrome",
            "Google Meet - Google Chrome",
            "meet.google.com/abc-defg-hij - Google Chrome",
        ] {
            assert_eq!(classify(&chrome, &[title.into()]).unwrap().app, CallApp::Meet, "{title}");
        }
    }

    #[test]
    fn browser_titles_that_only_mention_meet_are_uncertain() {
        let chrome = user("chrome.exe", r"C:\Chrome\chrome.exe", false);
        for title in ["Meeting notes - Google Docs - Google Chrome", "Meetup - Google Chrome"] {
            let d = classify(&chrome, &[title.into()]).unwrap();
            assert_eq!((d.app, d.certain), (CallApp::Browser, false), "{title}");
        }
    }

    #[test]
    fn browser_without_a_call_title_is_uncertain() {
        let d = classify(&user("msedge.exe", r"C:\Edge\msedge.exe", false), &["Inbox - Outlook".into()]).unwrap();
        assert_eq!((d.app, d.certain), (CallApp::Browser, false));
    }

    #[test]
    fn conclave_and_other_mic_users_are_ignored() {
        for (app, path, packaged) in [
            ("conclave.exe", r"C:\Program Files\Conclave\Conclave.exe", false),
            ("obs64.exe", r"C:\OBS\obs64.exe", false),
            ("notion.exe", r"C:\Notion\Notion.exe", false),
            ("microsoft.windowscommunicationsapps_8wekyb3d8bbwe", "microsoft.windowscommunicationsapps_8wekyb3d8bbwe", true),
        ] {
            assert_eq!(classify(&user(app, path, packaged), &[]), None, "{app}");
        }
    }

    #[test]
    fn released_app_is_not_detected() {
        let mut u = user("zoom.exe", r"C:\Zoom\bin\Zoom.exe", false);
        u.in_use = false;
        assert_eq!(classify(&u, &[]), None);
    }

    #[test]
    fn mode_parse_defaults_to_ask() {
        assert_eq!(Mode::parse("off"), Mode::Off);
        assert_eq!(Mode::parse("auto"), Mode::Auto);
        assert_eq!(Mode::parse("ask"), Mode::Ask);
        assert_eq!(Mode::parse("bogus"), Mode::Ask);
    }

    #[test]
    fn off_mode_does_nothing() {
        let mut d = Detector::new();
        assert!(d.step(&snap(0, Mode::Off, false, vec![zoom()])).is_empty());
        assert!(d.step(&snap(1000, Mode::Off, true, vec![zoom()])).is_empty());
        assert_eq!(d.attached(), None);
    }

    #[test]
    fn ask_prompts_once_until_the_app_releases_the_mic() {
        let mut d = Detector::new();
        assert_eq!(d.step(&snap(0, Mode::Ask, false, vec![zoom()])), vec![Action::Prompt { app: CallApp::Zoom, certain: true }]);
        assert!(d.step(&snap(1000, Mode::Ask, false, vec![zoom()])).is_empty(), "dismissed or ignored: no repeat");
        assert!(d.step(&snap(2000, Mode::Ask, false, vec![])).is_empty());
        assert_eq!(d.step(&snap(3000, Mode::Ask, false, vec![zoom()])), vec![Action::Prompt { app: CallApp::Zoom, certain: true }]);
    }

    #[test]
    fn auto_starts_certain_calls_and_prompts_uncertain_ones() {
        let mut d = Detector::new();
        assert_eq!(d.step(&snap(0, Mode::Auto, false, vec![zoom()])), vec![Action::Start { app: CallApp::Zoom }]);
        let mut d = Detector::new();
        assert_eq!(d.step(&snap(0, Mode::Auto, false, vec![browser()])), vec![Action::Prompt { app: CallApp::Browser, certain: false }]);
    }

    #[test]
    fn one_offer_per_step_certain_first() {
        let mut d = Detector::new();
        assert_eq!(d.step(&snap(0, Mode::Ask, false, vec![browser(), zoom()])), vec![Action::Prompt { app: CallApp::Zoom, certain: true }]);
        assert!(d.step(&snap(1000, Mode::Ask, false, vec![browser(), zoom()])).is_empty());
    }

    #[test]
    fn stops_exactly_after_the_grace_period() {
        let mut d = recording_attached_to_zoom();
        assert!(d.step(&snap(2000, Mode::Ask, true, vec![])).is_empty(), "release starts the grace");
        assert_eq!(d.next_deadline_ms(), Some(7000));
        assert!(d.step(&snap(6999, Mode::Ask, true, vec![])).is_empty());
        assert_eq!(d.step(&snap(7000, Mode::Ask, true, vec![])), vec![Action::Stop { app: CallApp::Zoom }]);
        assert!(d.step(&snap(8000, Mode::Ask, true, vec![])).is_empty(), "stop is sent once");
        assert_eq!(d.next_deadline_ms(), None);
    }

    #[test]
    fn short_release_does_not_stop() {
        let mut d = recording_attached_to_zoom();
        assert!(d.step(&snap(2000, Mode::Ask, true, vec![])).is_empty());
        assert!(d.step(&snap(4000, Mode::Ask, true, vec![zoom()])).is_empty(), "back within 5 s");
        assert!(d.step(&snap(5000, Mode::Ask, true, vec![])).is_empty(), "grace restarts");
        assert!(d.step(&snap(9999, Mode::Ask, true, vec![])).is_empty());
        assert_eq!(d.step(&snap(10000, Mode::Ask, true, vec![])), vec![Action::Stop { app: CallApp::Zoom }]);
    }

    #[test]
    fn held_mic_never_stops() {
        let mut d = recording_attached_to_zoom();
        for t in (2000..3_600_000).step_by(5000) {
            assert!(d.step(&snap(t, Mode::Ask, true, vec![zoom()])).is_empty(), "muted Zoom keeps the mic");
        }
    }

    #[test]
    fn off_and_back_on_mid_recording_reattaches() {
        let mut d = recording_attached_to_zoom();
        assert!(d.step(&snap(2000, Mode::Off, true, vec![zoom()])).is_empty());
        assert_eq!(d.attached(), None, "Off detaches");
        assert!(d.step(&snap(3000, Mode::Ask, true, vec![zoom()])).is_empty());
        assert_eq!(d.attached(), Some(&zoom()), "back on while recording: attached as a fresh start would be");
        assert!(d.step(&snap(4000, Mode::Ask, true, vec![])).is_empty(), "release starts the grace");
        assert_eq!(d.step(&snap(9000, Mode::Ask, true, vec![])), vec![Action::Stop { app: CallApp::Zoom }]);
    }

    #[test]
    fn manual_recording_during_a_call_attaches() {
        let mut d = Detector::new();
        assert!(d.step(&snap(0, Mode::Ask, true, vec![zoom()])).is_empty());
        assert_eq!(d.attached(), Some(&zoom()));
    }

    #[test]
    fn uncertain_browser_attaches_only_when_it_was_offered() {
        let mut d = Detector::new();
        d.step(&snap(0, Mode::Ask, true, vec![browser()]));
        assert_eq!(d.attached(), None, "manual start: browser not attached");
        let mut d = Detector::new();
        d.step(&snap(0, Mode::Ask, false, vec![browser()]));
        d.step(&snap(1000, Mode::Ask, true, vec![browser()]));
        assert_eq!(d.attached(), Some(&browser()), "Record on its banner: attached");
    }

    #[test]
    fn manual_start_prefers_a_certain_call_over_an_offered_browser() {
        let mut d = Detector::new();
        d.step(&snap(0, Mode::Ask, false, vec![browser()]));
        d.step(&snap(1000, Mode::Ask, true, vec![browser(), zoom()]));
        assert_eq!(d.attached(), Some(&zoom()), "the browser would stop the recording when it releases the mic");
    }

    #[test]
    fn rejoin_after_auto_stop_is_offered_once_the_save_finishes() {
        let mut d = recording_attached_to_zoom();
        d.step(&snap(2000, Mode::Ask, true, vec![]));
        assert_eq!(d.step(&snap(7000, Mode::Ask, true, vec![])), vec![Action::Stop { app: CallApp::Zoom }]);
        assert!(d.step(&snap(20_000, Mode::Ask, true, vec![zoom()])).is_empty(), "rejoined while the save runs");
        assert!(d.step(&snap(40_000, Mode::Ask, true, vec![zoom()])).is_empty(), "still saving");
        assert_eq!(
            d.step(&snap(41_000, Mode::Ask, false, vec![zoom()])),
            vec![Action::Prompt { app: CallApp::Zoom, certain: true }]
        );
    }

    #[test]
    fn manual_stop_during_a_call_is_not_offered_again() {
        let mut d = recording_attached_to_zoom();
        assert!(d.step(&snap(2000, Mode::Ask, true, vec![zoom()])).is_empty());
        assert!(d.step(&snap(3000, Mode::Ask, false, vec![zoom()])).is_empty(), "manual stop: no prompt");
    }

    #[test]
    fn recording_without_a_call_app_never_auto_stops() {
        let mut d = Detector::new();
        d.step(&snap(0, Mode::Auto, true, vec![]));
        assert!(d.step(&snap(600_000, Mode::Auto, true, vec![])).is_empty());
    }

    #[test]
    fn detections_are_ignored_while_recording_and_after_it() {
        let mut d = Detector::new();
        d.step(&snap(0, Mode::Ask, true, vec![]));
        assert!(d.step(&snap(1000, Mode::Ask, true, vec![zoom()])).is_empty(), "no prompt while recording");
        assert_eq!(d.attached(), None, "joined after the start: not attached");
        assert!(d.step(&snap(2000, Mode::Ask, false, vec![zoom()])).is_empty(), "not offered after the recording");
        d.step(&snap(3000, Mode::Ask, false, vec![]));
        assert_eq!(d.step(&snap(4000, Mode::Ask, false, vec![zoom()])), vec![Action::Prompt { app: CallApp::Zoom, certain: true }]);
    }

    #[test]
    fn follows_the_attached_apps_device() {
        let mut d = recording_attached_to_zoom();
        let s = with_sessions(snap(2000, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic A"), ("chrome.exe", "Mic B")]);
        assert_eq!(d.step(&s), vec![Action::FollowMic { app: CallApp::Zoom, device: "Mic A".into() }]);
        let s = with_current_mic(with_sessions(snap(3000, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic A")]), "Mic A");
        assert!(d.step(&s).is_empty(), "already on that mic: no repeat");
        let s = with_current_mic(with_sessions(snap(4000, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic C")]), "Mic A");
        assert_eq!(d.step(&s), vec![Action::FollowMic { app: CallApp::Zoom, device: "Mic C".into() }]);
        let s = with_current_mic(
            with_sessions(snap(5000, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic C"), ("zoom.exe", "Mic D")]),
            "Mic A",
        );
        assert!(d.step(&s).is_empty(), "ambiguous after a follow: keep quietly");
    }

    #[test]
    fn no_follow_when_already_on_that_mic() {
        let mut d = recording_attached_to_zoom();
        let s = with_current_mic(with_sessions(snap(2000, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic A")]), "Mic A");
        assert!(d.step(&s).is_empty(), "the recording already records from Mic A");
        let s = with_sessions(snap(3000, Mode::Ask, true, vec![zoom()]), &[]);
        assert!(d.step(&s).is_empty(), "the target was reached: a device missing later is not a never-found warning");
    }

    #[test]
    fn follow_retries_wait_between_attempts() {
        let mut d = recording_attached_to_zoom();
        let on_b = |now_ms| {
            with_current_mic(with_sessions(snap(now_ms, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic A")]), "Mic B")
        };
        assert_eq!(d.step(&on_b(2000)), vec![Action::FollowMic { app: CallApp::Zoom, device: "Mic A".into() }]);
        assert!(d.step(&on_b(3000)).is_empty(), "the first switch may still be running: no retry after 1 s");
        assert_eq!(
            d.step(&on_b(7000)),
            vec![Action::FollowMic { app: CallApp::Zoom, device: "Mic A".into() }],
            "retry 5 s after the first"
        );
    }

    #[test]
    fn follow_retries_until_the_mic_matches_then_stops_at_three() {
        let mut d = recording_attached_to_zoom();
        let on = |now_ms, current: &str| {
            with_current_mic(with_sessions(snap(now_ms, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic A")]), current)
        };
        let follow_a = || Action::FollowMic { app: CallApp::Zoom, device: "Mic A".into() };
        assert_eq!(d.step(&on(2000, "Mic B")), vec![follow_a()], "first attempt");
        assert_eq!(d.step(&on(7000, "Mic B")), vec![follow_a()], "retry 2: the device may have just appeared");
        assert_eq!(d.step(&on(12000, "Mic B")), vec![follow_a()], "retry 3");
        assert!(d.step(&on(17000, "Mic B")).is_empty(), "gives up after three attempts");
        assert!(d.step(&on(22000, "Mic B")).is_empty());
        assert!(d.step(&on(27000, "Mic A")).is_empty(), "the recording reaches Mic A on a later pass");
        let c = with_current_mic(with_sessions(snap(32000, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic C")]), "Mic A");
        assert_eq!(
            d.step(&c),
            vec![Action::FollowMic { app: CallApp::Zoom, device: "Mic C".into() }],
            "a new target starts over"
        );
    }

    #[test]
    fn unknown_device_warns_once() {
        let mut d = recording_attached_to_zoom();
        let s = with_sessions(snap(2000, Mode::Ask, true, vec![zoom()]), &[]);
        assert_eq!(d.step(&s), vec![Action::MicUnknown { app: CallApp::Zoom }]);
        let s = with_sessions(snap(3000, Mode::Ask, true, vec![zoom()]), &[]);
        assert!(d.step(&s).is_empty());
    }

    #[test]
    fn a_new_recording_starts_clean_after_a_stop() {
        let mut d = recording_attached_to_zoom();
        d.step(&snap(2000, Mode::Ask, true, vec![]));
        d.step(&snap(7000, Mode::Ask, true, vec![]));
        d.step(&snap(8000, Mode::Ask, false, vec![]));
        d.step(&snap(9000, Mode::Ask, true, vec![]));
        assert_eq!(d.attached(), None);
        assert!(d.step(&snap(20_000, Mode::Ask, true, vec![])).is_empty());
    }

    #[test]
    fn busy_holds_offers_until_the_save_finishes() {
        let mut d = Detector::new();
        d.step(&snap(0, Mode::Ask, false, vec![]));
        assert!(d.step(&with_busy(snap(1000, Mode::Ask, false, vec![zoom()]), true)).is_empty());
        assert_eq!(
            d.step(&snap(2000, Mode::Ask, false, vec![zoom()])),
            vec![Action::Prompt { app: CallApp::Zoom, certain: true }],
            "held back during the save, offered once it finishes"
        );
    }

    #[test]
    fn busy_after_auto_stop_then_offer() {
        let mut d = recording_attached_to_zoom();
        d.step(&snap(2000, Mode::Ask, true, vec![]));
        assert_eq!(d.step(&snap(7000, Mode::Ask, true, vec![])), vec![Action::Stop { app: CallApp::Zoom }]);
        assert!(d.step(&with_busy(snap(8000, Mode::Ask, false, vec![zoom()]), true)).is_empty());
        assert_eq!(
            d.step(&snap(9000, Mode::Ask, false, vec![zoom()])),
            vec![Action::Prompt { app: CallApp::Zoom, certain: true }]
        );
    }

    #[test]
    fn new_recording_between_steps_attaches_and_clears_stopping() {
        let mut d = Detector::new();
        d.step(&snap(0, Mode::Ask, false, vec![zoom()]));
        d.step(&with_id(snap(1000, Mode::Ask, true, vec![zoom()]), 1));
        assert_eq!(d.attached(), Some(&zoom()));

        d.step(&with_id(snap(2000, Mode::Ask, true, vec![]), 1));
        assert_eq!(d.step(&with_id(snap(7000, Mode::Ask, true, vec![]), 1)), vec![Action::Stop { app: CallApp::Zoom }]);
        assert!(d.stopping, "the save after the stop is running");

        // A new recording began since the last step: still recording, new id, Zoom in use again.
        assert!(d.step(&with_id(snap(8000, Mode::Ask, true, vec![zoom()]), 2)).is_empty());
        assert!(!d.stopping, "the new recording starts clean");
        assert_eq!(d.attached(), Some(&zoom()), "the new recording attaches to Zoom");

        assert!(d.step(&with_id(snap(9000, Mode::Ask, true, vec![]), 2)).is_empty());
        assert_eq!(d.step(&with_id(snap(14_000, Mode::Ask, true, vec![]), 2)), vec![Action::Stop { app: CallApp::Zoom }]);
    }
}
