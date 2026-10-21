//! Name alert gate (spec §7). Matching and cooldown are pure; `on_transcript` wires them to the banner.
use super::{settings, ALERT_COOLDOWN_SECS, MENTION_LINE_CHARS};
use crate::audio::transcription::TranscriptUpdate;
use crate::banner::{self, BannerKind};
use log::info;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime};

/// One cooldown for the whole recording. `reset_for_recording` gives each recording a fresh one.
static COOLDOWN: Mutex<Cooldown> = Mutex::new(Cooldown::new());

/// Called right before each transcript listener is registered, so a new recording starts with no cooldown.
pub fn reset_for_recording() {
    *COOLDOWN.lock().unwrap_or_else(PoisonError::into_inner) = Cooldown::new();
}

/// Runs for every transcript line. Returns at once when no alert words are set.
pub fn on_transcript<R: Runtime>(app: &AppHandle<R>, update: &TranscriptUpdate) {
    if !settings::has_alert_words() {
        return;
    }
    let words = settings::current().alert_words;
    let main_focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    let fire = {
        let mut cooldown = COOLDOWN.lock().unwrap_or_else(PoisonError::into_inner);
        should_alert(
            &words,
            &update.text,
            update.is_partial,
            update.speaker_idx,
            main_focused,
            &mut cooldown,
            Instant::now(),
        )
    };
    if !fire {
        return;
    }
    let line = banner_line(&update.text);
    // Creating a window inside the event callback is unsafe, so the banner is shown from a task.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        banner::show(&app, BannerKind::Mention { line });
    });
    info!("copilot: name alert shown");
}

/// True when a word of `text` starts with an alert word (case-insensitive, Unicode).
pub fn mentions(words: &[String], text: &str) -> bool {
    let words: Vec<String> = words
        .iter()
        .map(|w| w.to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return false;
    }
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .any(|t| words.iter().any(|w| t.starts_with(w.as_str())))
}

pub struct Cooldown {
    last: Option<Instant>,
}

impl Cooldown {
    pub const fn new() -> Self {
        Cooldown { last: None }
    }

    /// True and records `now` when the last alert was ≥ ALERT_COOLDOWN_SECS ago (or never).
    pub fn try_fire(&mut self, now: Instant) -> bool {
        let ready = match self.last {
            None => true,
            Some(last) => now.duration_since(last) >= Duration::from_secs(ALERT_COOLDOWN_SECS),
        };
        if ready {
            self.last = Some(now);
        }
        ready
    }
}

/// The whole gate, pure. The cooldown is consumed only when every other condition holds.
pub fn should_alert(
    words: &[String],
    text: &str,
    is_partial: bool,
    speaker_idx: Option<i64>,
    main_focused: bool,
    cooldown: &mut Cooldown,
    now: Instant,
) -> bool {
    if is_partial || speaker_idx == Some(0) || main_focused {
        return false;
    }
    mentions(words, text) && cooldown.try_fire(now)
}

/// The line for the banner, at most MENTION_LINE_CHARS chars, "…" appended when cut.
pub fn banner_line(text: &str) -> String {
    if text.chars().count() <= MENTION_LINE_CHARS {
        return text.to_string();
    }
    let mut line: String = text.chars().take(MENTION_LINE_CHARS - 1).collect();
    line.push('…');
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(ws: &[&str]) -> Vec<String> {
        ws.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn prefix_matches_declensions() {
        let w = words(&["Саш"]);
        assert!(mentions(&w, "Спасибо, Саше передай"));
        assert!(mentions(&w, "САШУ"));
        assert!(!mentions(&w, "Наташа"));
    }

    #[test]
    fn matches_through_punctuation_and_case() {
        let w = words(&["Sasha", "Саша"]);
        assert!(mentions(&w, "SASHA, what do you think?"));
        assert!(mentions(&w, "That's Sasha's call"));
        assert!(mentions(&w, "—Саша!"));
    }

    #[test]
    fn no_match_inside_a_word() {
        let w = words(&["Alex"]);
        assert!(!mentions(&w, "Malexa"));
        assert!(mentions(&w, "Alexander"));
    }

    #[test]
    fn cooldown_and_focus_gate() {
        let w = words(&["Sasha"]);
        let line = "Sasha, are you there?";
        let t0 = Instant::now();
        let mut cd = Cooldown::new();

        assert!(should_alert(&w, line, false, None, false, &mut cd, t0));
        assert!(!should_alert(
            &w,
            line,
            false,
            None,
            false,
            &mut cd,
            t0 + Duration::from_secs(10)
        ));
        assert!(should_alert(
            &w,
            line,
            false,
            None,
            false,
            &mut cd,
            t0 + Duration::from_secs(31)
        ));

        // Focus gate: no alert, and the cooldown is not consumed.
        let mut cd = Cooldown::new();
        assert!(!should_alert(&w, line, false, None, true, &mut cd, t0));
        assert!(should_alert(&w, line, false, None, false, &mut cd, t0));

        // Partial lines never alert and don't consume the cooldown.
        let mut cd = Cooldown::new();
        assert!(!should_alert(&w, line, true, None, false, &mut cd, t0));
        assert!(should_alert(&w, line, false, None, false, &mut cd, t0));

        // "You" lines (speaker 0) never alert; other speakers and unknown speakers can.
        let mut cd = Cooldown::new();
        assert!(!should_alert(&w, line, false, Some(0), false, &mut cd, t0));
        assert!(should_alert(&w, line, false, Some(1), false, &mut cd, t0));
        let mut cd = Cooldown::new();
        assert!(should_alert(&w, line, false, None, false, &mut cd, t0));
    }

    #[test]
    fn empty_words_never_alert() {
        let mut cd = Cooldown::new();
        let t0 = Instant::now();
        assert!(!mentions(&[], "Sasha"));
        assert!(!mentions(&words(&[""]), "Sasha"));
        assert!(!should_alert(&[], "Sasha", false, None, false, &mut cd, t0));
    }

    #[test]
    fn banner_line_trims() {
        let long: String = "а".repeat(300);
        let out = banner_line(&long);
        assert_eq!(out.chars().count(), MENTION_LINE_CHARS);
        assert!(out.ends_with('…'));
        assert_eq!(banner_line("короткая"), "короткая");
        let exact: String = "a".repeat(MENTION_LINE_CHARS);
        assert_eq!(banner_line(&exact), exact);
    }
}
