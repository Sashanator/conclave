//! "Keeps coming back": pattern keys that recur across the last few sessions, and the fillers-per-minute trend.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::report::Report;
use super::RECURRING_SESSIONS;

/// Trend points need at least this much speech, so a short session cannot skew the rate.
const TREND_MIN_SPEECH_SECS: f64 = 30.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recurring {
    pub patterns: Vec<RecurringPattern>,
    /// Fillers per minute, oldest first.
    pub filler_trend: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecurringPattern {
    pub key: String,
    pub sessions: u32,
    pub same_setup: bool,
}

#[derive(Debug, Clone)]
pub struct PastReport {
    pub session_id: String,
    pub setup_id: Option<String>,
    pub started_at: String,
    pub report: Report,
}

/// `recent` holds the earlier ready reports, newest first.
pub fn compute(current: &PastReport, recent: &[PastReport]) -> Recurring {
    // A session without a setup (its setup was deleted) is never the same setup as another.
    let same = |past: &PastReport| current.setup_id.is_some() && past.setup_id == current.setup_id;
    let window = std::iter::once(current).chain(recent.iter().take(RECURRING_SESSIONS - 1));
    // Per key: how many window sessions contain it, and whether one of them is in the current setup.
    let mut seen: BTreeMap<&str, (u32, bool)> = BTreeMap::new();
    for past in window {
        let keys: BTreeSet<&str> = past.report.items.iter().filter_map(|item| item.pattern.as_deref()).collect();
        for key in keys {
            let entry = seen.entry(key).or_insert((0, false));
            entry.0 += 1;
            entry.1 |= same(past);
        }
    }
    let mut patterns: Vec<RecurringPattern> = seen
        .into_iter()
        .filter(|(_, (sessions, _))| *sessions >= 2)
        .map(|(key, (sessions, same_setup))| RecurringPattern { key: key.to_string(), sessions, same_setup })
        .collect();
    patterns.sort_by(|a, b| {
        b.same_setup
            .cmp(&a.same_setup)
            .then(b.sessions.cmp(&a.sessions))
            .then(a.key.cmp(&b.key))
    });

    // The newest RECURRING_SESSIONS same-setup sessions, short ones dropped, then oldest first.
    let mut filler_trend: Vec<f64> = std::iter::once(current)
        .chain(recent.iter().filter(|past| same(past)))
        .take(RECURRING_SESSIONS)
        .filter(|past| past.report.fluency.speech_secs >= TREND_MIN_SPEECH_SECS)
        .map(|past| {
            let fluency = &past.report.fluency;
            let per_minute = f64::from(fluency.filler_total) / (fluency.speech_secs / 60.0);
            (per_minute * 10.0).round() / 10.0
        })
        .collect();
    filler_trend.reverse();

    Recurring { patterns, filler_trend }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::report::{Fluency, ItemKind, ReportItem};

    /// A report with one grammar item per pattern key (`patterns` empty for a v1-style report).
    fn past(id: &str, setup: &str, started: &str, patterns: &[&str], fillers: u32, speech_secs: f64) -> PastReport {
        let items = patterns
            .iter()
            .map(|p| ReportItem {
                kind: ItemKind::Grammar,
                turn: 1,
                quote: "q".into(),
                better: "b".into(),
                note: String::new(),
                echoed: false,
                pattern: Some(p.to_string()),
            })
            .collect();
        PastReport {
            session_id: id.into(),
            setup_id: Some(setup.into()),
            started_at: started.into(),
            report: Report {
                version: 2,
                fluency: Fluency { filler_total: fillers, speech_secs, ..Default::default() },
                items,
                dropped_items: 0,
                pronunciation: vec![],
                pronunciation_note: None,
            },
        }
    }

    fn pattern(key: &str, sessions: u32, same_setup: bool) -> RecurringPattern {
        RecurringPattern { key: key.into(), sessions, same_setup }
    }

    #[test]
    fn patterns_seen_in_two_sessions_are_listed() {
        let current = past("c", "s1", "2026-10-20T10:00", &["a b", "c d"], 0, 100.0);
        let recent = [
            past("r1", "s2", "2026-10-19T10:00", &["a b"], 0, 100.0),
            past("r2", "s3", "2026-10-18T10:00", &["x y"], 0, 100.0),
        ];
        let got = compute(&current, &recent);
        assert_eq!(got.patterns, [pattern("a b", 2, true)].to_vec());
    }

    #[test]
    fn same_setup_patterns_come_first() {
        // "e f" is in three sessions, but only in other setups; "c d" is in two, one of them the same setup
        let current = past("c", "s1", "2026-10-20T10:00", &["c d"], 0, 100.0);
        let recent = [
            past("r1", "s1", "2026-10-19T10:00", &["c d"], 0, 100.0),
            past("r2", "s2", "2026-10-18T10:00", &["e f"], 0, 100.0),
            past("r3", "s2", "2026-10-17T10:00", &["e f"], 0, 100.0),
            past("r4", "s2", "2026-10-16T10:00", &["e f"], 0, 100.0),
        ];
        let got = compute(&current, &recent);
        assert_eq!(got.patterns, [pattern("c d", 2, true), pattern("e f", 3, false)].to_vec());
    }

    #[test]
    fn window_is_five_sessions() {
        // The window is the current session and the four newest earlier ones: "q" reaches r3, "z" only r4 and r5
        let current = past("c", "s1", "2026-10-20T10:00", &["q"], 0, 100.0);
        let recent = [
            past("r0", "s2", "2026-10-19T10:00", &[], 0, 100.0),
            past("r1", "s2", "2026-10-18T10:00", &[], 0, 100.0),
            past("r2", "s2", "2026-10-17T10:00", &[], 0, 100.0),
            past("r3", "s2", "2026-10-16T10:00", &["q"], 0, 100.0),
            past("r4", "s2", "2026-10-15T10:00", &["z"], 0, 100.0),
            past("r5", "s2", "2026-10-14T10:00", &["z"], 0, 100.0),
        ];
        let got = compute(&current, &recent);
        assert_eq!(got.patterns, [pattern("q", 2, true)].to_vec());
    }

    #[test]
    fn filler_trend_is_per_minute_oldest_first_and_skips_short_sessions() {
        let current = past("c", "s1", "2026-10-20T10:00", &[], 6, 120.0); // 3.0
        let recent = [
            past("r1", "s1", "2026-10-19T10:00", &[], 10, 25.0), // under 30 s: skipped
            past("r2", "s2", "2026-10-18T10:00", &[], 50, 60.0), // another setup
            past("r3", "s1", "2026-10-17T10:00", &[], 4, 90.0),  // 2.7
            past("r4", "s1", "2026-10-16T10:00", &[], 1, 45.0),  // 1.3
            past("r5", "s1", "2026-10-15T10:00", &[], 3, 30.0),  // 6.0, exactly 30 s is kept
            past("r6", "s1", "2026-10-14T10:00", &[], 2, 60.0),  // 6th same-setup session: outside the window
        ];
        let got = compute(&current, &recent);
        assert_eq!(got.filler_trend, [6.0, 1.3, 2.7, 3.0].to_vec());
    }

    #[test]
    fn a_session_without_a_setup_is_never_the_same_setup() {
        // A deleted setup leaves setup_id None: it matches nothing, so the trend is just the current point
        let mut current = past("c", "s1", "2026-10-20T10:00", &["a b"], 6, 120.0);
        current.setup_id = None;
        let mut without_setup = past("r2", "s1", "2026-10-18T10:00", &["a b"], 8, 80.0);
        without_setup.setup_id = None;
        let recent = [past("r1", "s1", "2026-10-19T10:00", &["a b"], 4, 90.0), without_setup];
        let got = compute(&current, &recent);
        assert_eq!(got.patterns, [pattern("a b", 3, false)].to_vec());
        assert_eq!(got.filler_trend, [3.0].to_vec());
    }

    #[test]
    fn v1_reports_count_only_for_fillers() {
        // A v1 report has no pattern keys, but its fillers still make a trend point
        let current = past("c", "s1", "2026-10-20T10:00", &["a b"], 4, 80.0);
        let mut v1 = past("r1", "s1", "2026-10-19T10:00", &[], 8, 80.0);
        v1.report.version = 1;
        let got = compute(&current, &[v1]);
        assert_eq!(got.patterns, Vec::new());
        assert_eq!(got.filler_trend, [6.0, 3.0].to_vec());
    }
}
