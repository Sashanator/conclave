# Conclave Phase 2 (meeting auto-detection and dark theme) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship v0.3.0 with two features. First, Conclave notices Zoom, Meet and Teams calls. It then prompts with its own top-centre banner (Ask) or records at once (Auto). It stops about 5 s after the call app releases the mic and follows the call app's microphone. Second, a System/Light/Dark theme that covers every screen.

**Architecture:**
- The detection rules are a pure state machine, `crates/meeting-detector/src/classify.rs`, tested on any OS.
- The detection loop, `crates/meeting-detector/src/runner.rs`, feeds that machine snapshots from the existing Win32 layer and calls back into the app through a small `Host` trait.
- The app side, `frontend/src-tauri/src/meeting_detector.rs`, holds the setting and turns each action into the existing start/stop paths, which are the tray's paths moved into shared functions.
- The banner, `frontend/src-tauri/src/banner.rs`, is a new borderless, non-activating window that loads the static `/banner` page.
- The theme uses the shadcn tokens already in `globals.css` (`:root` and `.dark`), a `dark` class set before first paint, and a token sweep over about 980 hard-coded colour classes. A bun guard test enforces the sweep.

**Tech Stack:**
- Rust: the `meeting-detector` crate with `windows` 0.61 and `winreg` 0.55; Tauri 2.11.1, using `WebviewWindowBuilder::focusable(false)`, which sets `WS_EX_NOACTIVATE` in tao 0.35.2.
- Frontend: Next.js 14 static export, React 18, Tailwind (`darkMode: ['class']`), sonner 2.0.7, BlockNote 0.36 and bun tests.

**Spec:** `docs/superpowers/specs/2026-10-10-conclave-v1-design.md`, §5.1–§5.6, with field results from the 2026-10-13 Zoom probe:
- notifications arrive instantly;
- Zoom holds the mic while muted;
- capture sessions follow device switches;
- OBS and Notion hold the mic permanently;
- clicking a Windows notification does nothing.

Meet and Teams have not been probed yet; the user tests them at the checkpoint.

## Execution setup (as in v0.2.1; see CLAUDE.md)

| Role | Agent | Model / effort |
|---|---|---|
| Orchestration | main session | Opus 5.5, high |
| Implementation | `conclave-implementer` | Haiku 5.5, high |
| Review | `reviewer` | Opus 5.5, medium |
| Codex pass (Tasks 1, 2, 5, 6 and the phase review) | inside the reviewer brief | `codex review -c model="gpt-6.1-sol" -c model_reasoning_effort="medium" -c sandbox_mode='"danger-full-access"' -` |
| Third reviewer, whole branch only (Task 10) | Antigravity CLI, run by the controller | `agy -p "<prompt>" --model gemini-3.8-flash-high --mode plan --dangerously-skip-permissions --print-timeout 1800s` |

Create branch `phase-2` from `main` (`c9ca1a3` or later), then two worktrees branched from it:
- **Stream A (detection):** Tasks 1, 2, 4, 5, 6, in `.worktrees/p2-detect` on branch `p2-detect`.
- **Stream B (theme):** Tasks 3, 7, 8, 9, in `.worktrees/p2-theme` on branch `p2-theme`.

Order:
1. Tasks 1 and 3 run in parallel.
2. After Task 3 is approved, the controller merges `p2-theme` into `p2-detect`. Task 4's banner page needs Task 3's split root layout.
3. Stream A continues with Tasks 2, 4, 5, 6 while Stream B runs Tasks 7, 8, 9.
4. Task 10 merges both streams into `phase-2`.

Expected merge conflicts in Task 10 are small:
- Task 5 edits `RecordingSettings.tsx` and `useRecordingStart.ts`; Task 6 edits `app/page.tsx`. Tasks 7 and 8 recolour the same files.
- Keep both sides: the feature code from Stream A, the token classes from Stream B. Then re-run the guard test.

## Global Constraints

- **No new external dependencies**, Rust or npm. The app gains a path dependency on the workspace crate `meeting-detector`, which is not a new download. `serde_json`, `tokio` and `log` are already dependencies of the app.
- **Setting values:** `meeting_detection` is stored in `RecordingPreferences` (`recording_preferences.json`) as `"off"`, `"ask"` or `"auto"`. The default is `"ask"`, and any unknown value reads as `"ask"`.
- **Timings:**
  - stop grace: `STOP_GRACE_MS = 5_000`;
  - safety re-read: 5 000 ms;
  - banner auto-hide: 15 s.
- **Names:**
  - banner window labels: `meeting-banner-<id>`;
  - banner route: `/banner`;
  - commands: `get_meeting_detection_status`, `get_meeting_banner`, `meeting_banner_action`;
  - events: `mic-following` `{ device, app }`, `mic-following-warning` `{ message }`, `meeting-banner-update` (unused; do not add it).
- **Theme:** stored in `localStorage` key `theme`, with the values `system` (default), `light` and `dark`. The resolved theme is the `dark` class plus `color-scheme` on `<html>`.
- **Detection never stops or blocks a recording**, except through the 5-second release rule. Its errors are logged and shown in Settings. They are never thrown into the recording path.
- **Muting is not leaving:** Zoom keeps the mic while muted (probe 2026-10-13), so only the consent store's release counts.
- **Allowlist, not "any mic user":** only Zoom, Teams (packaged `MSTeams_*` or `ms-teams.exe`) and browsers (`chrome.exe`, `msedge.exe`, `firefox.exe`, `brave.exe`) are ever classified. OBS, Notion and Conclave itself never are.
- **App identity, no telemetry:** unchanged. `scripts/check-no-phone-home.sh` must stay clean.
- **CI minutes:** verify locally. There are two push points: the Phase 2 PR (Task 10), and the `v0.3.0` tag after the user's explicit yes.
- Every commit ends with the trailer of the model that made it.

## Review Focus

1. **A long muted Zoom call.** Zoom keeps the mic, so the recording must never stop. Pinned by Task 1's `held_mic_never_stops` test.
2. **A headset switch: Zoom drops the mic for under 5 s.** No stop, and the grace restarts on the next release. Pinned by Task 1's `short_release_does_not_stop`.
3. **The consent store becomes unreadable mid-recording.** This must not look like "Zoom released the mic", so there is no stop. Pinned by Task 2's rule that a read error skips the step. The reviewer checks it in `runner.rs`.
4. **The banner appears while the user types in Zoom's chat.** Keyboard focus stays in Zoom. Set up by Task 4's `focused(false)` and `focusable(false)` on a fresh window per banner. Only the Windows checklist can confirm it.
5. **Upgrading from v0.2.1, whose saved `recording_preferences.json` has no `meeting_detection` field.** The value loads as `"ask"`, and the saved mic and system devices survive. Pinned by Task 5's serde test.

---

## File Structure

| Path | Change | Task |
|---|---|---|
| `crates/meeting-detector/src/classify.rs` (new) | Pure detection rules: classification, Ask/Auto/Off, attach, the 5 s stop, mic following | 1 |
| `crates/meeting-detector/src/lib.rs` | `pub mod classify; pub mod runner;` | 1, 2 |
| `crates/meeting-detector/src/runner.rs` (new) | `Host` trait, `spawn`, and the Windows wait/read/step loop | 2 |
| `docs/superpowers/specs/2026-10-10-conclave-v1-design.md`, `AGENTS.md` | Name `runner.rs`; record the manual-pick ruling | 2 |
| `frontend/src/lib/theme.ts` (new), `frontend/tests/lib/theme.test.ts` (new) | Theme preference parsing and resolution, and the no-flash init script | 3 |
| `frontend/src/contexts/ThemeContext.tsx` (new) | `ThemeProvider` and `useTheme` | 3 |
| `frontend/src/app/layout.tsx` | Root `html`/`head`/`body` + `ThemeProvider`; the existing tree moves into `AppShell`; `/banner` renders without the app shell | 3 |
| `frontend/src/components/PreferenceSettings.tsx` | Theme select | 3 |
| `frontend/src/components/BlockNoteEditor/Editor.tsx`, `frontend/src/components/AISummary/BlockNoteSummaryView.tsx` | BlockNote follows the theme | 3 |
| `frontend/tailwind.config.js`, `frontend/src/app/globals.css` | Content globs; scrollbar colours from tokens | 3 |
| `frontend/src-tauri/tauri.conf.json` | Drop `"theme": "Light"`; `core:window:allow-set-theme`; banner capability | 3, 4 |
| `frontend/tests/theme-guard.test.ts` (new) | Fails on hard-coded neutral colours, or on light tints without a `dark:` pair | 3, 7, 8, 9 |
| `frontend/src-tauri/src/banner.rs` (new) | Banner window: create, position, auto-hide, actions | 4 |
| `frontend/src/app/banner/page.tsx` (new) | Banner UI | 4 |
| `frontend/src-tauri/Cargo.toml` | `meeting-detector = { path = "../../crates/meeting-detector" }` | 4 |
| `frontend/src-tauri/src/tray.rs` | `request_recording_start` and `stop_recording_and_save` extracted from the tray handlers | 4, 5 |
| `frontend/src-tauri/src/meeting_detector.rs` (new) | The setting, the status, `AppHost`, action handling | 5, 6 |
| `frontend/src-tauri/src/audio/recording_preferences.rs` | `meeting_detection` field; the setter updates the live mode | 5 |
| `frontend/src-tauri/src/lib.rs` | `mod banner; mod meeting_detector;`, commands, `meeting_detector::start` in setup | 4, 5 |
| `frontend/src/hooks/useRecordingStart.ts` | Auto-start reads the `autoStartTitlePrefix` key | 5 |
| `frontend/src/components/RecordingSettings.tsx` | Auto-detect select and status | 5 |
| `frontend/src-tauri/src/audio/recording_commands.rs` | `follow_mic` | 6 |
| `frontend/src/lib/mic-following.ts` (new), `frontend/tests/lib/mic-following.test.ts` (new), `frontend/src/hooks/useMicFollowing.ts` (new), `frontend/src/components/MicFollowingLine.tsx` (new), `frontend/src/app/page.tsx` | The "🎙 Mic (following Zoom)" line and warning | 6 |
| About 70 files under `frontend/src` | Hard-coded colours become tokens | 7, 8, 9 |
| `.github/workflows/release.yml` | Release notes name the two features | 10 |

---

### Task 1: Detection rules (`classify.rs`)

**Files:**
- Create: `crates/meeting-detector/src/classify.rs`
- Modify: `crates/meeting-detector/src/lib.rs`

**Interfaces:**
- Consumes: `consent::{MicUser, is_browser}` (existing).
- Produces (used by Tasks 2, 4, 5 and 6):
  - `Mode { Off, Ask, Auto }`, with `Mode::parse(&str) -> Mode`;
  - `CallApp { Zoom, Teams, Meet, Browser }`, with `label() -> &'static str`;
  - `Detected { path, exe, app, certain }`;
  - `classify(&MicUser, &[String]) -> Option<Detected>`;
  - `Action { Prompt { app, certain }, Start { app }, Stop { app }, FollowMic { app, device }, MicUnknown { app } }`;
  - `CaptureUse { exe, device }`;
  - `Snapshot { now_ms, mode, recording, in_use, sessions }`;
  - `Detector::{new, step, attached, next_deadline_ms}`;
  - `STOP_GRACE_MS`.

Rulings already made (do not re-open them):
- **Prompts:** an app is offered at most once while it holds the mic, whether it was dismissed, ignored or recorded. There is therefore no dismiss channel back from the banner.
- **Attach on recording start:** a recording that starts attaches to the last offered app if that app is still in use; otherwise to the first certain app in use. An uncertain browser is attached only if it was the one offered.
- **Manual mic pick:** the mic picker is disabled during recording (`SettingsModal.tsx:184`, `disabled={isRecording}`), so spec §5.5's "following turns off after a manual pick" has no trigger and is not built.
- **One offer per step:** certain apps come first. The other new apps count as seen, so there is no banner storm.

- [ ] **Step 1: Write `classify.rs` with its tests**

```rust
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
        } else if titles.iter().any(|t| t.contains("Meet")) {
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
    /// Call apps using the mic now (the output of `classify`).
    pub in_use: Vec<Detected>,
    /// Active capture sessions, read only while recording; `None` when not read.
    pub sessions: Option<Vec<CaptureUse>>,
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
    followed: Option<String>,
    warned: bool,
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

        let started = s.recording && !self.was_recording;
        self.was_recording = s.recording;
        if s.mode == Mode::Off || !s.recording {
            self.detach();
        }
        if s.mode == Mode::Off {
            return Vec::new();
        }

        if s.recording {
            // A manual start during a detected call attaches too (spec §5.1, Stop).
            if started {
                self.attached = self
                    .offered
                    .as_ref()
                    .and_then(|p| s.in_use.iter().find(|d| &d.path == p))
                    .or_else(|| s.in_use.iter().find(|d| d.certain))
                    .cloned();
            }
            // New detections are ignored while recording, and not offered once it ends.
            self.seen.extend(s.in_use.iter().map(|d| d.path.clone()));
            return self.while_recording(s);
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
        self.followed = None;
        self.warned = false;
    }

    fn while_recording(&mut self, s: &Snapshot) -> Vec<Action> {
        let Some(att) = self.attached.clone() else { return Vec::new() };
        if !s.in_use.iter().any(|d| d.path == att.path) {
            let since = *self.released_at.get_or_insert(s.now_ms);
            if s.now_ms.saturating_sub(since) >= STOP_GRACE_MS {
                self.detach();
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
            [one] if self.followed.as_deref() != Some(*one) => {
                self.followed = Some(one.to_string());
                self.warned = false;
                vec![Action::FollowMic { app: att.app, device: one.to_string() }]
            }
            [_] => Vec::new(),
            // No session or several: keep the current mic, and warn once if the device was never found.
            _ if self.followed.is_none() && !self.warned => {
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
        Snapshot { now_ms, mode, recording, in_use, sessions: None }
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
        let s = with_sessions(snap(3000, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic A")]);
        assert!(d.step(&s).is_empty(), "same device: no repeat");
        let s = with_sessions(snap(4000, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic C")]);
        assert_eq!(d.step(&s), vec![Action::FollowMic { app: CallApp::Zoom, device: "Mic C".into() }]);
        let s = with_sessions(snap(5000, Mode::Ask, true, vec![zoom()]), &[("zoom.exe", "Mic C"), ("zoom.exe", "Mic D")]);
        assert!(d.step(&s).is_empty(), "ambiguous after a follow: keep quietly");
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
}
```

- [ ] **Step 2: Export it.** In `crates/meeting-detector/src/lib.rs`, add `pub mod classify;` after `pub mod consent;`.

- [ ] **Step 3: Run the tests.** `cargo test -p meeting-detector` → all pass, including the existing `consent` tests. Then `cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins` → clean.

- [ ] **Step 4: Commit.** `git commit -m "feat(detector): pure meeting-detection rules (classify, ask/auto, 5 s stop, mic following)"`

---

### Task 2: Detection loop (`runner.rs`)

**Files:**
- Create: `crates/meeting-detector/src/runner.rs`
- Modify: `crates/meeting-detector/src/lib.rs`, `docs/superpowers/specs/2026-10-10-conclave-v1-design.md` (§5.3 "Units", §5.5), `AGENTS.md` (the `crates/meeting-detector/` layout row)

**Interfaces:**
- Consumes: Task 1's `classify`, `Detector`, `Snapshot`, `CaptureUse`, `Mode` and `Action`. From `windows.rs`: `read_mic_users`, `ChangeWatcher`, `Wake`, `window_titles_for`, `capture_sessions` and `init_com`.
- Produces (used by Task 5): the `pub trait Host: Send + 'static { fn mode(&self) -> Mode; fn is_recording(&self) -> bool; fn apply(&self, action: Action); fn set_error(&self, reason: Option<String>); }` and `pub fn spawn<H: Host>(host: H) -> std::io::Result<()>`. Both compile on every OS. On non-Windows targets, `spawn` reports "Meeting detection works on Windows only." through `set_error` and starts nothing.

Ruling: the loop lives in the crate, not in the app's `meeting_detector.rs` as spec §5.3 has it. That way all Windows code type-checks locally with `cargo check -p meeting-detector --target x86_64-pc-windows-msvc`. The app crate cannot be cross-checked here because of whisper.cpp. Record this in the spec.

- [ ] **Step 1: Write `runner.rs`**

```rust
//! The detection loop (spec §5.3): wakes on consent-store changes, or every 5 s as a safety net, and feeds
//! snapshots to `Detector`. The app supplies a `Host`.
use crate::classify::{Action, Mode};

/// What the app provides to the loop. Every method is called from the detector thread.
pub trait Host: Send + 'static {
    fn mode(&self) -> Mode;
    fn is_recording(&self) -> bool;
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
```

- [ ] **Step 2: Export it.** `crates/meeting-detector/src/lib.rs` becomes:

```rust
//! Detects online meetings from Windows' microphone-usage record (spec §5.3).
pub mod classify;
pub mod consent;
pub mod runner;
#[cfg(windows)]
pub mod windows;
```

- [ ] **Step 3: Update the docs.**
  - **Spec §5.3 "Units":** add a bullet after the `windows.rs` bullet: "`crates/meeting-detector/src/runner.rs`: the watch loop (wait, read, classify, step) behind a small `Host` trait, so all Windows code type-checks without the app (ruling 2026-10-13)." Change the `meeting_detector.rs` bullet to: "the app side: holds the setting, implements `Host`, and wires actions to the existing recording paths, the banner and the hot-swap path."
  - **Spec §5.5:** append to the `classify.rs` cases: "(The manual-pick case has no trigger: the mic picker is disabled while recording.)"
  - **`AGENTS.md`:** change the layout row for `crates/meeting-detector/` to: "Windows meeting detection: pure rules (`classify.rs`), the watch loop (`runner.rs`), the Win32 layer (`windows.rs`), and the `conclave-probe` diagnostic binary."

- [ ] **Step 4: Verify.**
  - `cargo test -p meeting-detector` → pass. The runner test is Windows-only, so it is compiled out here.
  - `cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins` → clean. This type-checks `run` and the Windows test.

- [ ] **Step 5: Commit.** `git commit -m "feat(detector): event-driven detection loop behind a Host trait"`

---

### Task 3: Theme foundation (setting, no-flash start, layout split, guard)

**Files:**
- Create: `frontend/src/lib/theme.ts`, `frontend/tests/lib/theme.test.ts`, `frontend/src/contexts/ThemeContext.tsx`, `frontend/tests/theme-guard.test.ts`
- Modify:
  - `frontend/src/app/layout.tsx`
  - `frontend/src/components/PreferenceSettings.tsx`
  - `frontend/src/components/BlockNoteEditor/Editor.tsx:50`
  - `frontend/src/components/AISummary/BlockNoteSummaryView.tsx:268`
  - `frontend/tailwind.config.js:4-8`
  - `frontend/src/app/globals.css:169-180` (scrollbar)
  - `frontend/src-tauri/tauri.conf.json` (window `"theme": "Light"`, line 21; main capability)

**Interfaces:**
- Produces:
  - `ThemePreference = 'system' | 'light' | 'dark'`;
  - `parseThemePreference(v: string | null)`, `resolveTheme(pref, systemDark)`, `applyResolvedTheme(el, resolved)`, `THEME_STORAGE_KEY`, `THEME_INIT_SCRIPT`;
  - `ThemeProvider` and `useTheme(): { preference, resolved, setPreference }`;
  - in `layout.tsx`, the `/banner` route renders `children` inside `ThemeProvider` only, with no app providers. Task 4 relies on this.

- [ ] **Step 1: Write the failing test** `frontend/tests/lib/theme.test.ts`:

```ts
import { describe, expect, test } from 'bun:test';
import { parseThemePreference, resolveTheme, THEME_INIT_SCRIPT, type ThemePreference } from '../../src/lib/theme';

/** Runs the inline <head> script against fakes and returns the theme it applied. */
function runInitScript(stored: string | null, systemDark: boolean) {
  const classes = new Set<string>();
  const style: Record<string, string> = {};
  const document = {
    documentElement: {
      classList: { toggle: (c: string, on: boolean) => (on ? classes.add(c) : classes.delete(c)) },
      style,
    },
  };
  const localStorage = { getItem: () => stored };
  const matchMedia = () => ({ matches: systemDark });
  new Function('document', 'localStorage', 'matchMedia', THEME_INIT_SCRIPT)(document, localStorage, matchMedia);
  return { dark: classes.has('dark'), colorScheme: style.colorScheme };
}

describe('theme', () => {
  test('parse falls back to system', () => {
    expect(parseThemePreference('light')).toBe('light');
    expect(parseThemePreference('dark')).toBe('dark');
    expect(parseThemePreference('system')).toBe('system');
    expect(parseThemePreference(null)).toBe('system');
    expect(parseThemePreference('purple')).toBe('system');
  });

  test('resolve: explicit choice wins, system follows Windows', () => {
    const cases: [ThemePreference, boolean, 'light' | 'dark'][] = [
      ['system', false, 'light'], ['system', true, 'dark'],
      ['light', true, 'light'], ['light', false, 'light'],
      ['dark', false, 'dark'], ['dark', true, 'dark'],
    ];
    for (const [pref, sys, want] of cases) expect(resolveTheme(pref, sys)).toBe(want);
  });

  test('the no-flash script applies exactly what resolveTheme says', () => {
    for (const stored of [null, 'system', 'light', 'dark', 'garbage']) {
      for (const systemDark of [false, true]) {
        const want = resolveTheme(parseThemePreference(stored), systemDark);
        const got = runInitScript(stored, systemDark);
        expect(got.dark).toBe(want === 'dark');
        expect(got.colorScheme).toBe(want);
      }
    }
  });

  test('the script never throws when storage is blocked', () => {
    const document = { documentElement: { classList: { toggle() {} }, style: {} } };
    const localStorage = { getItem: () => { throw new Error('blocked'); } };
    expect(() => new Function('document', 'localStorage', 'matchMedia', THEME_INIT_SCRIPT)(document, localStorage, () => ({ matches: false }))).not.toThrow();
  });
});
```

Run `cd frontend && bun test tests/lib/theme.test.ts`. It must FAIL: the module is not found.

- [ ] **Step 2: Implement** `frontend/src/lib/theme.ts`:

```ts
export type ThemePreference = 'system' | 'light' | 'dark';
export type ResolvedTheme = 'light' | 'dark';

export const THEME_STORAGE_KEY = 'theme';

export function parseThemePreference(value: string | null): ThemePreference {
  return value === 'light' || value === 'dark' ? value : 'system';
}

export function resolveTheme(pref: ThemePreference, systemDark: boolean): ResolvedTheme {
  return pref === 'system' ? (systemDark ? 'dark' : 'light') : pref;
}

export function applyResolvedTheme(el: HTMLElement, resolved: ResolvedTheme) {
  el.classList.toggle('dark', resolved === 'dark');
  el.style.colorScheme = resolved;
}

/** Inline <head> script: applies the saved theme before first paint, so dark mode never flashes light.
 *  Must agree with parseThemePreference + resolveTheme (pinned by tests/lib/theme.test.ts). */
export const THEME_INIT_SCRIPT =
  "try{var p=localStorage.getItem('theme');" +
  "var d=p==='dark'||(p!=='light'&&matchMedia('(prefers-color-scheme: dark)').matches);" +
  "document.documentElement.classList.toggle('dark',d);" +
  "document.documentElement.style.colorScheme=d?'dark':'light'}catch(e){}";
```

Re-run the test: PASS.

- [ ] **Step 3: Implement** `frontend/src/contexts/ThemeContext.tsx`:

```tsx
'use client';

import { createContext, useCallback, useContext, useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  applyResolvedTheme, parseThemePreference, resolveTheme, THEME_STORAGE_KEY,
  type ResolvedTheme, type ThemePreference,
} from '@/lib/theme';

interface ThemeContextValue {
  preference: ThemePreference;
  resolved: ResolvedTheme;
  setPreference: (pref: ThemePreference) => void;
}

const ThemeContext = createContext<ThemeContextValue>({
  preference: 'system',
  resolved: 'light',
  setPreference: () => {},
});

function readStored(): ThemePreference {
  try {
    return parseThemePreference(localStorage.getItem(THEME_STORAGE_KEY));
  } catch {
    return 'system';
  }
}

const systemQuery = () => window.matchMedia('(prefers-color-scheme: dark)');

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  // Lazy initialisers read the same values the <head> script used, so the first client render matches the page.
  const [preference, setPref] = useState<ThemePreference>(() => (typeof window === 'undefined' ? 'system' : readStored()));
  const [systemDark, setSystemDark] = useState(() => typeof window !== 'undefined' && systemQuery().matches);
  const resolved = resolveTheme(preference, systemDark);

  useEffect(() => {
    const mq = systemQuery();
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener('change', onChange);
    // Other Conclave windows (the meeting banner) share localStorage and pick up a change here.
    const onStorage = (e: StorageEvent) => {
      if (e.key === THEME_STORAGE_KEY) setPref(parseThemePreference(e.newValue));
    };
    window.addEventListener('storage', onStorage);
    return () => {
      mq.removeEventListener('change', onChange);
      window.removeEventListener('storage', onStorage);
    };
  }, []);

  useEffect(() => {
    applyResolvedTheme(document.documentElement, resolved);
  }, [resolved]);

  useEffect(() => {
    // The native title bar follows too; null = follow Windows.
    getCurrentWindow().setTheme(preference === 'system' ? null : preference).catch(() => {});
  }, [preference]);

  const setPreference = useCallback((pref: ThemePreference) => {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, pref);
    } catch {
      // Storage blocked: the choice still applies for this session.
    }
    setPref(pref);
  }, []);

  return <ThemeContext.Provider value={{ preference, resolved, setPreference }}>{children}</ThemeContext.Provider>;
}

export const useTheme = () => useContext(ThemeContext);
```

- [ ] **Step 4: Split the root layout.** In `frontend/src/app/layout.tsx`:
  1. Rename the current `export default function RootLayout` to `function AppShell({ children }: { children: React.ReactNode })`. Move into it, unchanged, all of the current component's state, effects and handlers, plus everything that is now inside `<body>`: the provider tree, `<Toaster>` and the import overlay. `AppShell` returns that tree without `<html>` or `<body>`.
  2. Replace `<Toaster position="bottom-center" richColors closeButton />` with `<ThemedToaster />`, and define it:

```tsx
function ThemedToaster() {
  const { resolved } = useTheme();
  return <Toaster theme={resolved} position="bottom-center" richColors closeButton />;
}
```

  3. Add the new default export:

```tsx
export default function RootLayout({ children }: { children: React.ReactNode }) {
  // The meeting banner window (spec §5.1) loads /banner. It must not mount the app's providers: a second
  // RecordingPostProcessingProvider would save every meeting twice.
  const isBanner = usePathname()?.startsWith('/banner') ?? false;
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_INIT_SCRIPT }} />
      </head>
      <body className={`${sourceSans3.variable} font-sans antialiased`}>
        <ThemeProvider>{isBanner ? children : <AppShell>{children}</AppShell>}</ThemeProvider>
      </body>
    </html>
  );
}
```

  4. Add the imports: `usePathname` from `next/navigation`, `ThemeProvider` and `useTheme` from `@/contexts/ThemeContext`, and `THEME_INIT_SCRIPT` from `@/lib/theme`.

- [ ] **Step 5: Theme setting.** In `PreferenceSettings.tsx`, add an "Appearance" section at the top, in the style of the file's existing sections. It holds a "Theme" select with three options: System (follows Windows), Light and Dark. The select is bound to `useTheme().preference` / `setPreference`. Use the `Select` components from `@/components/ui/select`. Use token classes only (`text-foreground`, `text-muted-foreground`, `bg-card`, `border-border`).

- [ ] **Step 6: Everything else follows the theme.**
  - `Editor.tsx:50` and `BlockNoteSummaryView.tsx:268`: `theme="light"` → `theme={resolved}`, with `const { resolved } = useTheme();`.
  - `tailwind.config.js` content: add `'./src/lib/**/*.{js,ts,jsx,tsx,mdx}'`, `'./src/hooks/**/*.{js,ts,jsx,tsx,mdx}'` and `'./src/contexts/**/*.{js,ts,jsx,tsx,mdx}'`. `src/lib/recordingNotification.tsx` uses classes that the current globs purge.
  - `globals.css` scrollbar: `#d1d5db` → `hsl(var(--border))`; `#9ca3af` → `hsl(var(--muted-foreground))`.
  - `tauri.conf.json`:
    - Delete `"theme": "Light"` from the main window. A forced Light theme also forces WebView2's `prefers-color-scheme` to light.
    - Add `"core:window:allow-set-theme"` to the `main` capability's permissions.

- [ ] **Step 7: Guard test.** Create `frontend/tests/theme-guard.test.ts`:

```ts
import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const SRC = join(import.meta.dir, '..', 'src');

// Files not yet moved to theme tokens. Tasks 7-9 of the Phase 2 plan remove their files; Task 9 leaves this empty.
const PENDING = new Set<string>([
]);

// Neutral colours must be tokens (bg-background, text-muted-foreground, border-border…), never hard-coded.
const NEUTRAL =
  /(?<!dark:)\b(?:bg-white|(?:bg|text|border|divide|ring|ring-offset|placeholder|from|via|to|fill|stroke|outline)-(?:gray|slate|zinc|neutral|stone)-\d{2,3})\b/;
// Light tints of a hue need a dark: partner on the same line (e.g. bg-blue-50 dark:bg-blue-950/40).
const HUES = 'red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose';
const TINT = new RegExp(`(?<!dark:)\\b(?:bg-(?:${HUES})-(?:50|100|200)|text-(?:${HUES})-(?:700|800|900)|border-(?:${HUES})-(?:100|200|300))\\b`);

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) return sourceFiles(p);
    return /\.(tsx?|css)$/.test(name) ? [p] : [];
  });
}

function offenders(file: string): string[] {
  return readFileSync(file, 'utf8')
    .split('\n')
    .flatMap((line, i) => (NEUTRAL.test(line) || (TINT.test(line) && !line.includes('dark:')) ? [`${i + 1}: ${line.trim()}`] : []));
}

describe('theme guard', () => {
  const files = sourceFiles(SRC).map((f) => [relative(SRC, f).split('\\').join('/'), f] as const);

  test('converted files use theme tokens', () => {
    const bad = files.filter(([rel]) => !PENDING.has(rel)).flatMap(([rel, f]) => offenders(f).map((o) => `${rel}:${o}`));
    expect(bad).toEqual([]);
  });

  test('every pending file still needs converting (keep the list honest)', () => {
    const stale = [...PENDING].filter((rel) => {
      const hit = files.find(([r]) => r === rel);
      return !hit || offenders(hit[1]).length === 0;
    });
    expect(stale).toEqual([]);
  });
});
```

Run it once with the empty `PENDING`. Copy every file it reports into `PENDING`, sorted, one per line, as paths relative to `src` (for example `'components/Sidebar/index.tsx'`). Re-run it: PASS. Files this task already fully converted, such as `PreferenceSettings.tsx` if no offenders remain, stay out of the list.

- [ ] **Step 8: Verify.** `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build` → all pass. In the built output, `out/banner` does not exist yet (Task 4 adds it). Then `bash scripts/check-no-phone-home.sh` → clean.

- [ ] **Step 9: Commit.** `git commit -m "feat(theme): System/Light/Dark setting with no-flash start, themed toasts and editor, colour guard"`

---

### Task 4: Meeting banner window

**Files:**
- Create: `frontend/src-tauri/src/banner.rs`, `frontend/src/app/banner/page.tsx`
- Modify:
  - `frontend/src-tauri/Cargo.toml`: add `meeting-detector = { path = "../../crates/meeting-detector" }` under `[dependencies]`;
  - `frontend/src-tauri/src/lib.rs`: `pub mod banner;`, and register the commands;
  - `frontend/src-tauri/src/tray.rs`: extract `request_recording_start`;
  - `frontend/src-tauri/tauri.conf.json`: the banner capability.

**Interfaces:**
- Consumes: `meeting_detector::classify::CallApp` (Task 1), and Task 3's `/banner` layout branch with `useTheme`.
- Produces (used by Task 5):
  - `banner::show<R: Runtime>(app: &AppHandle<R>, kind: BannerKind)`, with `BannerKind { Detected { call: CallApp, certain: bool }, Ended { call: CallApp } }`;
  - `tray::request_recording_start<R: Runtime>(app: &AppHandle<R>, title_prefix: Option<&str>)`;
  - the commands `get_meeting_banner` and `meeting_banner_action`.

Rulings (do not re-open them):
- **A fresh window per banner, with a unique label.** tao shows a window with `SW_SHOWNOACTIVATE` only at creation (`focused(false)`). A later `show()` uses `SW_SHOW`, which could take focus from the call. Unique labels also avoid closing and recreating windows under the same label.
- **Content comes from Rust state** (`get_meeting_banner`), not from the URL.
- **The 15 s timer and Dismiss both close the window.** The detector never re-offers an app while it holds the mic, so Dismiss needs no other effect.

- [ ] **Step 1: Write `banner.rs`, with the pure helpers tested first**

```rust
//! Conclave's own top-centre meeting banner (spec §5.1 Ask mode). It replaces Windows notifications, which do
//! nothing when clicked (probe check D, 2026-10-13).
use meeting_detector::classify::CallApp;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const LABEL_PREFIX: &str = "meeting-banner-";
const WIDTH: f64 = 460.0;
const HEIGHT: f64 = 72.0;
const TOP_MARGIN: f64 = 16.0;
const AUTO_HIDE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerKind {
    Detected { call: CallApp, certain: bool },
    Ended { call: CallApp },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BannerView {
    pub kind: &'static str,
    pub app: &'static str,
    pub certain: bool,
}

impl BannerKind {
    fn view(self) -> BannerView {
        match self {
            BannerKind::Detected { call, certain } => BannerView { kind: "detected", app: call.label(), certain },
            BannerKind::Ended { call } => BannerView { kind: "ended", app: call.label(), certain: true },
        }
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

/// Removes and returns the banner's content if `label` is still the current banner.
fn take_if_current(label: &str) -> Option<BannerKind> {
    let id = id_from_label(label)?;
    let mut current = CURRENT.lock().unwrap();
    match *current {
        Some((cur, kind)) if cur == id => {
            *current = None;
            Some(kind)
        }
        _ => None,
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
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    *CURRENT.lock().unwrap() = Some((id, kind));
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
        if take_if_current(&label).is_some() {
            if let Some(w) = app.get_webview_window(&label) {
                let _ = w.close();
            }
        }
    });
}

#[tauri::command]
pub fn get_meeting_banner<R: Runtime>(window: WebviewWindow<R>) -> Option<BannerView> {
    let id = id_from_label(window.label())?;
    match *CURRENT.lock().unwrap() {
        Some((cur, kind)) if cur == id => Some(kind.view()),
        _ => None,
    }
}

/// `action` is "record" or "dismiss". Async: a sync command that touches windows can deadlock on Windows.
#[tauri::command]
pub async fn meeting_banner_action<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    action: String,
) -> Result<(), String> {
    let kind = take_if_current(window.label());
    let _ = window.close();
    if action == "record" {
        if let Some(BannerKind::Detected { call, .. }) = kind {
            crate::tray::request_recording_start(&app, Some(call.label()));
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

    #[test]
    fn only_the_current_banner_can_act() {
        *CURRENT.lock().unwrap() = Some((7, BannerKind::Ended { call: CallApp::Zoom }));
        assert_eq!(take_if_current(&label_for(6)), None, "stale banner");
        assert_eq!(take_if_current(&label_for(7)), Some(BannerKind::Ended { call: CallApp::Zoom }));
        assert_eq!(take_if_current(&label_for(7)), None, "acts once");
    }

    #[test]
    fn view_texts() {
        let v = BannerKind::Detected { call: CallApp::Meet, certain: true }.view();
        assert_eq!((v.kind, v.app, v.certain), ("detected", "Google Meet", true));
        assert_eq!(BannerKind::Ended { call: CallApp::Teams }.view().kind, "ended");
    }
}
```

- [ ] **Step 2: Extract the tray start.** In `tray.rs`, add:

```rust
/// Starts recording through the main webview's normal start path (model check, recording notice), without
/// bringing the window forward. `title_prefix` (a call app name) goes in front of the generated meeting title.
pub fn request_recording_start<R: Runtime>(app: &AppHandle<R>, title_prefix: Option<&str>) {
    set_tray_state(app, RecordingState::Starting);
    if let Some(window) = app.get_webview_window("main") {
        // JSON-encode so the prefix is always a safe JS string literal.
        let prefix = serde_json::to_string(title_prefix.unwrap_or("")).unwrap_or_else(|_| "\"\"".to_string());
        let _ = window.eval(&format!(
            "sessionStorage.setItem('autoStartTitlePrefix', {prefix}); sessionStorage.setItem('autoStartRecording', 'true'); window.location.assign('/')"
        ));
    }
}
```

In `toggle_recording_handler`'s start branch, replace the `set_tray_state(..Starting)` call and the two `window.eval` calls with `request_recording_start(&app_clone, None);`. The handler's `focus_main_window` stays, because a tray click should show the window.

- [ ] **Step 3: Register.** In `lib.rs`:
  - add `pub mod banner;`;
  - add `banner::get_meeting_banner` and `banner::meeting_banner_action` to `generate_handler!`.

  In `tauri.conf.json`, add a second capability after `main`:

```json
{
    "identifier": "meeting-banner",
    "description": "Meeting banner windows: read their content, act, follow the theme",
    "windows": ["meeting-banner-*"],
    "permissions": ["core:default", "core:window:allow-set-theme"]
}
```

- [ ] **Step 4: Banner page.** Create `frontend/src/app/banner/page.tsx`. Use token classes only, because the guard test covers this file.

```tsx
'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Mic, X } from 'lucide-react';

interface BannerView {
  kind: 'detected' | 'ended';
  app: string;
  certain: boolean;
}

export default function MeetingBanner() {
  const [view, setView] = useState<BannerView | null>(null);

  useEffect(() => {
    invoke<BannerView | null>('get_meeting_banner').then(setView).catch(() => setView(null));
  }, []);

  const act = (action: 'record' | 'dismiss') => invoke('meeting_banner_action', { action }).catch(() => {});

  if (!view) return null;
  const text =
    view.kind === 'ended'
      ? `${view.app} meeting ended. Notes are being generated.`
      : view.certain
        ? `${view.app} meeting detected`
        : 'Browser is using the microphone';

  return (
    <div className="flex h-screen w-screen select-none items-center gap-3 overflow-hidden rounded-xl border border-border bg-card px-4 text-card-foreground">
      <Mic className="h-5 w-5 shrink-0 text-primary" aria-hidden />
      <span className="flex-1 truncate text-sm font-medium">{text}</span>
      {view.kind === 'detected' && (
        <button
          onClick={() => act('record')}
          className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
        >
          Record
        </button>
      )}
      <button
        onClick={() => act('dismiss')}
        aria-label="Dismiss"
        className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-accent-foreground"
      >
        {view.kind === 'detected' ? <span className="text-sm">Dismiss</span> : <X className="h-4 w-4" />}
      </button>
    </div>
  );
}
```

- [ ] **Step 5: Verify.**
  - `cargo test -p meetily banner` → the 4 tests pass.
  - `cargo test --workspace --exclude llama-helper` → pass (needs `frontend/out` and the stub sidecar; see AGENTS.md).
  - `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build` → pass, and `out/banner.html` exists.
  - `bash scripts/check-no-phone-home.sh` → clean.
  - `git diff main -- Cargo.lock` → only the `meeting-detector` entry is added to the `meetily` dependency list; no new external package.

- [ ] **Step 6: Commit.** `git commit -m "feat(banner): non-activating top-centre meeting banner window"`

---

### Task 5: Wire detection into the app

**Files:**
- Create: `frontend/src-tauri/src/meeting_detector.rs`
- Modify:
  - `frontend/src-tauri/src/lib.rs`: `pub mod meeting_detector;`, the command, and the call in setup;
  - `frontend/src-tauri/src/tray.rs`: extract `stop_recording_and_save`;
  - `frontend/src-tauri/src/audio/recording_preferences.rs`;
  - `frontend/src/hooks/useRecordingStart.ts` (the auto-start effect, around line 203);
  - `frontend/src/components/RecordingSettings.tsx`;
  - `frontend/src/services/configService.ts:39`.

**Interfaces:**
- Consumes: Task 2's `Host` and `spawn`; Task 1's `Action`, `Mode` and `CallApp`; Task 4's `banner::show` and `tray::request_recording_start`.
- Produces:
  - `meeting_detector::{set_mode(&str), mode() -> Mode, start(&AppHandle)}`;
  - the command `get_meeting_detection_status() -> Option<String>`;
  - `tray::stop_recording_and_save<R: Runtime>(app: &AppHandle<R>)` (async);
  - the `apply` match. Task 6 fills in its `FollowMic` and `MicUnknown` arms.

- [ ] **Step 1: Preference field, with a failing serde test.** In `recording_preferences.rs`, add to the struct:

```rust
    /// Meeting auto-detection: "off", "ask" (default) or "auto" (spec §5.1).
    #[serde(default = "default_meeting_detection")]
    pub meeting_detection: String,
```

Add `fn default_meeting_detection() -> String { "ask".to_string() }` and `meeting_detection: default_meeting_detection(),` in `Default`. Grep for other `RecordingPreferences {` struct literals and add the field there too. Add a test module:

```rust
#[cfg(test)]
mod meeting_detection_pref_tests {
    use super::*;

    #[test]
    fn v021_file_without_the_field_loads_as_ask_and_keeps_devices() {
        let json = r#"{"save_folder":"C:\\rec","auto_save":true,"file_format":"mp4",
            "preferred_mic_device":"Shure MV7","preferred_system_device":"Speakers"}"#;
        let p: RecordingPreferences = serde_json::from_str(json).unwrap();
        assert_eq!(p.meeting_detection, "ask");
        assert_eq!(p.preferred_mic_device.as_deref(), Some("Shure MV7"));
        assert_eq!(p.preferred_system_device.as_deref(), Some("Speakers"));
    }
}
```

In `set_recording_preferences`, after a successful save, call `crate::meeting_detector::set_mode(&preferences.meeting_detection);`. The save and its error mapping stay as they are.

- [ ] **Step 2: Extract the tray stop.** In `tray.rs`, move the body of `stop_recording_handler`'s spawned task (save path, `stop_recording`, emitting `recording-stop-complete`, the error revert) into:

```rust
/// Stops the recording and hands it to the frontend's save flow via `recording-stop-complete` (works from any page).
pub async fn stop_recording_and_save<R: Runtime>(app: &AppHandle<R>) { /* moved body, unchanged */ }
```

`stop_recording_handler` keeps its `set_tray_state(Stopping)` and `focus_main_window`, then spawns `stop_recording_and_save(&app_clone).await`. `toggle_recording_handler`'s stop branch keeps `set_tray_state(Stopping)` and calls `stop_recording_and_save(&app_clone).await`. That removes the duplicated block. The log prefixes become "Stop:".

- [ ] **Step 3: Write `meeting_detector.rs`**

```rust
//! App side of meeting detection (spec §5): the Off/Ask/Auto setting, the status Settings shows, and what each
//! detector action does. The detection loop lives in `crates/meeting-detector`.
use crate::banner::{self, BannerKind};
use meeting_detector::classify::{Action, Mode};
use meeting_detector::runner::Host;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Runtime};

/// Ask until the saved preference loads.
static MODE: AtomicU8 = AtomicU8::new(1);
static ERROR: Mutex<Option<String>> = Mutex::new(None);

fn encode(m: Mode) -> u8 {
    match m {
        Mode::Off => 0,
        Mode::Ask => 1,
        Mode::Auto => 2,
    }
}

fn decode(v: u8) -> Mode {
    match v {
        0 => Mode::Off,
        2 => Mode::Auto,
        _ => Mode::Ask,
    }
}

/// Takes the stored value ("off", "ask" or "auto").
pub fn set_mode(stored: &str) {
    MODE.store(encode(Mode::parse(stored)), Ordering::Relaxed);
}

pub fn mode() -> Mode {
    decode(MODE.load(Ordering::Relaxed))
}

/// `None` while detection runs; otherwise the reason it doesn't, shown in Settings.
#[tauri::command]
pub fn get_meeting_detection_status() -> Option<String> {
    ERROR.lock().unwrap().clone()
}

struct AppHost<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> Host for AppHost<R> {
    fn mode(&self) -> Mode {
        mode()
    }

    fn is_recording(&self) -> bool {
        tauri::async_runtime::block_on(crate::audio::recording_commands::is_recording())
    }

    fn apply(&self, action: Action) {
        apply(&self.app, action)
    }

    fn set_error(&self, reason: Option<String>) {
        match &reason {
            Some(r) => log::warn!("Meeting detection unavailable: {r}"),
            None => log::info!("Meeting detection running"),
        }
        *ERROR.lock().unwrap() = reason;
    }
}

fn apply<R: Runtime>(app: &AppHandle<R>, action: Action) {
    log::info!("Meeting detection: {action:?}");
    match action {
        Action::Prompt { app: call, certain } => banner::show(app, BannerKind::Detected { call, certain }),
        Action::Start { app: call } => crate::tray::request_recording_start(app, Some(call.label())),
        Action::Stop { app: call } => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move { crate::tray::stop_recording_and_save(&handle).await });
            banner::show(app, BannerKind::Ended { call });
        }
        // Mic following arrives in Task 6.
        Action::FollowMic { .. } | Action::MicUnknown { .. } => {}
    }
}

/// Loads the saved mode, then starts the detector thread.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match crate::audio::recording_preferences::load_recording_preferences(&app).await {
            Ok(p) => set_mode(&p.meeting_detection),
            Err(e) => log::warn!("Meeting detection: preferences unreadable, using Ask: {e}"),
        }
        if let Err(e) = meeting_detector::runner::spawn(AppHost { app }) {
            *ERROR.lock().unwrap() = Some(format!("Cannot start meeting detection: {e}"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_mode_round_trips_and_unknown_is_ask() {
        for (s, m) in [("off", Mode::Off), ("ask", Mode::Ask), ("auto", Mode::Auto), ("??", Mode::Ask)] {
            set_mode(s);
            assert_eq!(mode(), m, "{s}");
        }
        set_mode("ask");
    }
}
```

If `load_recording_preferences` takes the handle differently, adapt only the call. Keep `apply` free of `.await`. The detector thread is not a tokio thread, which is why `is_recording` uses `block_on`.

- [ ] **Step 4: Start it.** In `lib.rs`:
  - add `pub mod meeting_detector;`;
  - add `meeting_detector::get_meeting_detection_status` to `generate_handler!`;
  - in `.setup`, right after `tray::create_tray(...)`, add `meeting_detector::start(_app.handle());`.

- [ ] **Step 5: Title prefix in the frontend.** In `useRecordingStart.ts`'s auto-start effect, right after `sessionStorage.removeItem('autoStartRecording');`, add:

```ts
          // Set by meeting detection (tray.rs request_recording_start): the call app's name, e.g. "Zoom".
          const titlePrefix = sessionStorage.getItem('autoStartTitlePrefix');
          sessionStorage.removeItem('autoStartTitlePrefix');
```

and change `const generatedMeetingTitle = generateMeetingTitle();` in the same effect to:

```ts
            const generatedMeetingTitle = titlePrefix ? `${titlePrefix} ${generateMeetingTitle()}` : generateMeetingTitle();
```

- [ ] **Step 6: Settings UI.**
  - **Types:** in `RecordingSettings.tsx`, add `meeting_detection: 'off' | 'ask' | 'auto';` to its `RecordingPreferences` interface, and `meeting_detection: 'ask'` to the initial state. Add `meeting_detection?: 'off' | 'ask' | 'auto';` to the interface in `configService.ts`.
  - **Status:** load `invoke<string | null>('get_meeting_detection_status')` on mount into `detectionStatus`.
  - **New section "Meeting detection"**, placed before the device section, in the file's existing section style and with token classes only:
    - a `Select` (`@/components/ui/select`) with the values `off` "Off", `ask` "Ask (show a banner)" and `auto` "Auto-record";
    - a muted help line: "Detects Zoom, Google Meet and Microsoft Teams calls. Recording stops 5 seconds after you leave the call.";
    - when `detectionStatus` is non-null, show it in `text-amber-700 dark:text-amber-400`.
  - **Handler:** `handleDetectionChange(v)` saves `{ ...preferences, meeting_detection: v }` with `invoke('set_recording_preferences', { preferences })` directly and toasts `Meeting detection: <label>`. Do not use `savePreferences`, whose toast describes devices. Revert the state on error.

- [ ] **Step 7: Verify.**
  - `cargo test --workspace --exclude llama-helper` → pass, including the new serde and mode tests.
  - `cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins` → clean.
  - `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build` → pass.
  - `bash scripts/check-no-phone-home.sh` → clean.
  - Grep: `toggle_recording_handler` and `stop_recording_handler` no longer contain their own copy of the stop block.

- [ ] **Step 8: Commit.** `git commit -m "feat(detector): Off/Ask/Auto setting; detection prompts, starts and stops recordings"`

---

### Task 6: Mic following

**Files:**
- Create: `frontend/src/lib/mic-following.ts`, `frontend/tests/lib/mic-following.test.ts`, `frontend/src/hooks/useMicFollowing.ts`, `frontend/src/components/MicFollowingLine.tsx`
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs`, `frontend/src-tauri/src/meeting_detector.rs` (the two arms), `frontend/src/app/page.tsx`

**Interfaces:**
- Consumes: the existing `RECORDING_MANAGER`, `session_live`, `MIC_SWAP_IN_PROGRESS`, `perform_mic_hot_swap_task`, and `RecordingState::get_microphone_device() -> Option<Arc<AudioDevice>>` (`AudioDevice.name`).
- Produces:
  - `pub async fn follow_mic<R: Runtime>(app: AppHandle<R>, device_name: String) -> Result<(), String>`;
  - the events `mic-following` `{ device, app }` and `mic-following-warning` `{ message }`.

Ruling: following goes through the existing hot-swap task. It retries once after 500 ms and emits `mic-device-switched` or `mic-swap-failed`. It does not use the disconnect-fallback budget (`MAX_MIC_FALLBACK_ATTEMPTS`). The comment at `recording_commands.rs:1321-1333` says device-list-driven swaps were removed because cpal hung on Bluetooth devices. Following swaps only when the call app itself moved to another device, which is far rarer. The user checks a Bluetooth headset at the checkpoint.

- [ ] **Step 1: `follow_mic`.** In `recording_commands.rs`, next to `trigger_mic_fallback_to_default`:

```rust
/// Meeting detection's mic following (spec §5.2): move the live recording to the call app's capture device.
/// Ok(()) when switched or already on that device.
pub async fn follow_mic<R: Runtime>(app: AppHandle<R>, device_name: String) -> Result<(), String> {
    let session = RECORDING_MANAGER
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| m.get_state().clone())
        .ok_or_else(|| "Not recording".to_string())?;
    // session_live locks RECORDING_MANAGER: call it only after the guard above is dropped.
    if !session_live(&session) {
        return Err("Not recording".to_string());
    }
    if session.get_microphone_device().is_some_and(|d| d.name == device_name) {
        return Ok(());
    }
    if MIC_SWAP_IN_PROGRESS.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err("Another microphone switch is in progress".to_string());
    }
    struct SwapGuard;
    impl Drop for SwapGuard {
        fn drop(&mut self) {
            MIC_SWAP_IN_PROGRESS.store(false, Ordering::SeqCst);
        }
    }
    let _guard = SwapGuard;
    perform_mic_hot_swap_task(device_name, &session, app).await
}
```

If `get_state()` returns something other than `&Arc<RecordingState>`, adapt the clone and nothing else.

- [ ] **Step 2: Fill in the arms** in `meeting_detector.rs` (add `use tauri::Emitter;`):

```rust
        Action::FollowMic { app: call, device } => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::audio::recording_commands::follow_mic(handle.clone(), device.clone()).await {
                    Ok(()) => {
                        let _ = handle.emit("mic-following", serde_json::json!({ "device": device, "app": call.label() }));
                    }
                    Err(e) => {
                        log::warn!("Mic following: cannot switch to {device}: {e}");
                        let _ = handle.emit("mic-following-warning", serde_json::json!({
                            "message": format!("Couldn't switch to {device}, the microphone {} is using. Recording continues on the current microphone.", call.label())
                        }));
                    }
                }
            });
        }
        Action::MicUnknown { app: call } => {
            let _ = app.emit("mic-following-warning", serde_json::json!({
                "message": format!("Couldn't tell which microphone {} is using. Recording continues on the current microphone.", call.label())
            }));
        }
```

- [ ] **Step 3: Failing frontend test** `frontend/tests/lib/mic-following.test.ts`:

```ts
import { describe, expect, test } from 'bun:test';
import { micLine, reduceMicFollowing, initialMicFollowing } from '../../src/lib/mic-following';

describe('mic following line', () => {
  test('shows the selected mic until following starts', () => {
    expect(micLine(initialMicFollowing, 'Shure MV7')).toBe('🎙 Shure MV7');
    expect(micLine(initialMicFollowing, null)).toBe('🎙 Default microphone');
  });

  test('following names the app; a later fallback switch drops it', () => {
    let s = reduceMicFollowing(initialMicFollowing, { type: 'following', device: 'Maono DM40', app: 'Zoom' });
    expect(micLine(s, 'Shure MV7')).toBe('🎙 Maono DM40 (following Zoom)');
    s = reduceMicFollowing(s, { type: 'switched', device: 'Realtek Mic' });
    expect(micLine(s, 'Shure MV7')).toBe('🎙 Realtek Mic');
  });

  test('warning is kept until a successful follow or a new recording', () => {
    let s = reduceMicFollowing(initialMicFollowing, { type: 'warning', message: 'Couldn’t tell' });
    expect(s.warning).toBe('Couldn’t tell');
    s = reduceMicFollowing(s, { type: 'following', device: 'A', app: 'Zoom' });
    expect(s.warning).toBeNull();
    s = reduceMicFollowing(reduceMicFollowing(s, { type: 'warning', message: 'x' }), { type: 'reset' });
    expect(s).toEqual(initialMicFollowing);
  });
});
```

Run `bun test tests/lib/mic-following.test.ts`. It must FAIL.

- [ ] **Step 4: Implement the helper, the hook, the line, and the placement.**

`frontend/src/lib/mic-following.ts`:

```ts
export interface MicFollowingState {
  device: string | null;
  app: string | null;
  warning: string | null;
}

export type MicFollowingEvent =
  | { type: 'following'; device: string; app: string }
  | { type: 'switched'; device: string }
  | { type: 'warning'; message: string }
  | { type: 'reset' };

export const initialMicFollowing: MicFollowingState = { device: null, app: null, warning: null };

export function reduceMicFollowing(s: MicFollowingState, e: MicFollowingEvent): MicFollowingState {
  switch (e.type) {
    case 'following':
      return { device: e.device, app: e.app, warning: null };
    case 'switched':
      // A switch not made by following (disconnect fallback) ends the "following" label.
      return s.app && s.device === e.device ? s : { ...s, device: e.device, app: null };
    case 'warning':
      return { ...s, warning: e.message };
    case 'reset':
      return initialMicFollowing;
  }
}

export function micLine(s: MicFollowingState, selectedMic: string | null): string {
  const device = s.device ?? selectedMic ?? 'Default microphone';
  return s.app ? `🎙 ${device} (following ${s.app})` : `🎙 ${device}`;
}
```

`frontend/src/hooks/useMicFollowing.ts`: a hook that keeps the state with `useReducer(reduceMicFollowing, initialMicFollowing)` and subscribes with `listen` from `@tauri-apps/api/event`:
- `mic-following` → `{type:'following', ...payload}`;
- `mic-device-switched` → `{type:'switched', device: payload.device_name}`;
- `mic-following-warning` → `{type:'warning', message: payload.message}`;
- `recording-started` → `{type:'reset'}`.

It unsubscribes on unmount and returns the state.

`frontend/src/components/MicFollowingLine.tsx`: takes `{ selectedMic: string | null }`, calls the hook, and renders two lines:
- `<p className="text-xs text-muted-foreground truncate">{micLine(state, selectedMic)}</p>`;
- when `state.warning` is set, `<p className="text-xs text-amber-700 dark:text-amber-400">{state.warning}</p>`.

`app/page.tsx`: in the fixed recording-controls container, directly under the `div` that holds the rounded pill, render `{recordingState.isRecording && <div className="mt-2 flex justify-center"><MicFollowingLine selectedMic={selectedDevices?.micDevice ?? null} /></div>}` inside the same `w-2/3 max-w-[750px]` column. Change the column to `flex flex-col items-center` if needed so the line sits under the pill.

- [ ] **Step 5: Verify.**
  - `cargo test --workspace --exclude llama-helper` → pass.
  - `cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins` → clean.
  - `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build` → pass.

- [ ] **Step 6: Commit.** `git commit -m "feat(detector): follow the call app's microphone and show it on the recording screen"`

---

### Tasks 7–9: Colour sweep (shared rules)

Each sweep task converts the files it owns, removes them from `PENDING` in `frontend/tests/theme-guard.test.ts`, and runs the guard test. **Only colour classes change.** No layout, copy or behaviour changes, and no file outside the task's list.

| Hard-coded | Becomes |
|---|---|
| `bg-white` | `bg-background` for pages and panels; `bg-card` for cards, pills and floating bars; `bg-popover` for menus and popovers |
| `bg-gray-50` | `bg-muted/50` |
| `bg-gray-100`, `bg-gray-200` | `bg-muted` |
| `hover:bg-gray-50/100/200` (also `hover:bg-white`) | `hover:bg-accent` (add `hover:text-accent-foreground` when the text colour changes on hover) |
| `bg-gray-300`…`bg-gray-500` (tracks, bars, dots) | `bg-muted-foreground/30`, or `bg-border` for 1 px lines |
| `bg-gray-700`…`bg-gray-900` (dark chips, tooltips) | `bg-foreground text-background` (inverts in dark) |
| `text-gray-900`, `-800`, `-700` | `text-foreground` |
| `text-gray-600`, `-500`, `-400` | `text-muted-foreground` |
| `text-gray-300`, `-200` | `text-muted-foreground/60` |
| `border-gray-*`, `divide-gray-*` | `border-border`, `divide-border`; form fields: `border-input` |
| `ring-gray-*`, `ring-offset-white` | `ring-ring`, `ring-offset-background` |
| `placeholder-gray-*` | `placeholder:text-muted-foreground` |
| `slate`, `zinc`, `neutral`, `stone` | same as gray |
| Light tints: `bg-<hue>-50/100/200` | keep, and add `dark:bg-<hue>-950/40` on the same line |
| Dark hue text: `text-<hue>-700/800/900` | keep, and add `dark:text-<hue>-300` on the same line |
| Tint borders: `border-<hue>-100/200/300` | keep, and add `dark:border-<hue>-800` on the same line |
| Solid accents (`bg-blue-600 text-white`, `bg-red-500`, `text-blue-600`, …) | keep |
| SVG `stroke="#e5e7eb"` / `fill` greys | `className="stroke-border"` / `fill-muted` (remove the attribute) |

A `cn(...)` or template literal split over several lines must keep each tint and its `dark:` partner on the same line.

Verify each sweep task with:
- `cd frontend && bun test tests/theme-guard.test.ts`, which passes with the task's files out of `PENDING`;
- `pnpm exec tsc --noEmit && bun test && pnpm run build`, which pass;
- `git diff --stat`, which lists only the task's files and the guard test.

Commit: `git commit -m "style(theme): theme tokens in <area>"`.

### Task 7: Sweep: app shell and recording

**Files:** every `PENDING` entry under these paths:
- `app/`, `components/Sidebar/`, `lib/`, `hooks/` and `contexts/`;
- `components/RecordingControls.tsx`, `AudioLevelMeter.tsx`, `Logo.tsx`, `ChunkProgressDisplay.tsx`, `TranscriptView.tsx`, `VirtualizedTranscriptView.tsx`, `MainContent.tsx`, `TranscriptRecovery.tsx`, `StatusOverlays.tsx` and `MicFollowingLine.tsx`. `MicFollowingLine.tsx` is already tokenised; skip it if absent.

Note: `app/page.tsx:245` and `app/_components/StatusOverlays.tsx:29` are floating pills, so they become `bg-card`. `components/Sidebar/index.tsx:654,665` are the sidebar and its toggle, so they become `bg-background` / `bg-card`.

### Task 8: Sweep: settings and models

**Files:** every `PENDING` entry among:
- `components/PreferenceSettings.tsx`, `RecordingSettings.tsx`, `DeviceSelection.tsx`, `AudioBackendSelector.tsx`, `ModelSettingsModal.tsx`, `WhisperModelManager.tsx`, `ParakeetModelManager.tsx`, `BuiltInModelManager.tsx`, `ModelDownloadProgress.tsx`, `LanguagePickerPopover.tsx`, `SummaryLanguageSettings.tsx` and `About.tsx`;
- `components/shared/`;
- any other `PENDING` file under `components/` whose name contains `Settings`, `Model` or `Notification`.

`ModelDownloadProgress.tsx:77,85`: the track stroke becomes `stroke-border`, and the blue progress stroke stays.

### Task 9: Sweep: everything else

**Files:** every remaining `PENDING` entry. This covers meeting details, AISummary, BlockNoteEditor, onboarding, ImportAudio, DatabaseImport and `components/ui/`. In `button.tsx`, the gray variant becomes `secondary`-style tokens, and the coloured variants keep their solid colours.

When done, `PENDING` is `new Set<string>([])`. Keep the empty set and its comment: it documents the mechanism for future files. Also run `grep -rn 'theme="light"' frontend/src` → no matches.

---

### Task 10: Integration, phase review, PR

**Files:** `.github/workflows/release.yml` (`releaseBody`), plus merge-conflict resolutions.

- [ ] **Step 1: Merge.**
  1. On `phase-2`, merge `p2-theme`, then `p2-detect`.
  2. Resolve the conflicts by keeping Stream A's feature code with Stream B's token classes.
  3. Re-run the guard test. Any Stream A line that brought back a hard-coded neutral or an unpaired tint is fixed in a `style(theme):` commit.

- [ ] **Step 2: Release notes.** Add to `releaseBody`, keeping it one single-quoted YAML string with `''` escapes:

  > New in 0.3.0: meeting auto-detection for Zoom, Google Meet and Teams (Settings → Recording; Ask shows a banner at the top of the screen, Auto-record starts at once; recording stops 5 seconds after you leave the call and follows the call's microphone), and a dark theme (Settings → General → Theme).

  Run `actionlint`.

- [ ] **Step 3: Full local verification.**
  - `cd frontend && pnpm install --frozen-lockfile && pnpm exec tsc --noEmit && bun test && pnpm run build`
  - `cargo test --workspace --exclude llama-helper`
  - `cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins`
  - `bash scripts/check-no-phone-home.sh`
  - `actionlint`

- [ ] **Step 4: Phase review.**
  1. Run the `reviewer` (Opus, medium) with a Codex pass over `main...phase-2`.
  2. Run Antigravity on the same range.
  3. Verify every finding, then do one fix wave and one scoped re-review.

  The review prompts carry this plan's Review Focus. They also ask about:
  - deadlocks between the detector thread, `RECORDING_MANAGER` and `block_on`;
  - the banner's focus behaviour;
  - double saves from the banner route;
  - whether a stop can fire on a read error.

- [ ] **Step 5: PR.** Push `phase-2` and open a PR to `main`. CI runs the Frontend checks (Linux) and the meeting-detector (Windows) job, which includes the runner's registry test. Wait for green.

- [ ] **Step 6: Ask the user.**
  - Ask "Merge and tag v0.3.0?" and present the rulings, test results and open findings.
  - After an explicit yes: merge, tag, watch the release, then give the Windows checklist:
    1. Ask mode with Zoom: the banner shows at the top centre, typing in Zoom keeps working, and Record starts a "Zoom Meeting …" recording without bringing Conclave forward.
    2. Leaving the call stops the recording within about 6 s and shows the "meeting ended" banner, and the notes and auto-summary appear.
    3. Muting for over a minute does not stop the recording.
    4. Switching the mic in Zoom mid-call updates "🎙 … (following Zoom)" within about 5 s. Repeat with the Bluetooth headset.
    5. Auto mode with Zoom.
    6. Meet in Chrome or Edge, including with the call tab in the background (expect the "Browser is using the microphone" banner).
    7. The Teams app and Teams in the browser.
    8. OBS or Notion open produce no banner.
    9. Settings → General → Theme: Light, Dark and System, on every screen, with no white flash at start and the title bar following.
