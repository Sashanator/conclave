// Pure calendar reconcile: calendar events + meeting rows -> actions. No I/O, never decides on
// content it cannot see: the caller's `untouched` flag is the only delete gate (spec 6.6, 4).

use super::{strip_marker, ExpandResult, Occurrence};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashSet;

pub struct CalRow {
    pub id: String,
    pub title: String,
    pub title_source: String,
    pub calendar_uid: String,
    pub calendar_start: Option<String>,
    pub calendar_original_start: Option<String>,
    pub calendar_marker: Option<String>,
    pub calendar_cancelled_at: Option<String>,
    pub started_at: Option<String>,
    pub untouched: bool,
}

pub struct Snapshot {
    pub meetings: Vec<CalRow>,
    /// (calendar_uid, original_start as stored: raw text, either `Z` or `+00:00` form).
    pub dismissed: Vec<(String, String)>,
}

pub struct Window {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

#[derive(Debug, PartialEq)]
pub enum DeleteReason {
    Gone,
    Past,
}

#[derive(Debug, PartialEq)]
pub enum Action {
    Adopt { id: String, original_start: String },
    Insert { uid: String, title: String, start: String, end: String, original_start: String },
    Move { id: String, start: String, end: String },
    Rename { id: String, title: String },
    Cancel { id: String },
    Uncancel { id: String },
    Delete { id: String, reason: DeleteReason },
}

/// Instants, never raw strings: the same moment is stored as `Z` or `+00:00`.
fn instant(s: &Option<String>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.as_deref()?).ok().map(|d| d.with_timezone(&Utc))
}

pub fn plan(
    events: &ExpandResult,
    rows: &Snapshot,
    live_target: Option<&str>,
    marker: &str,
    window: &Window,
    now: DateTime<Utc>,
) -> Vec<Action> {
    let day = Duration::days(1);
    let mut out = Vec::new();
    let mut used: HashSet<usize> = HashSet::new();
    let dismissed: Vec<(&str, Option<DateTime<Utc>>)> =
        rows.dismissed.iter().map(|(u, o)| (u.as_str(), instant(&Some(o.clone())))).collect();
    let frozen = |r: &CalRow| r.started_at.is_some() || live_target == Some(r.id.as_str());

    for ev in &events.occurrences {
        let Some(title) = strip_marker(&ev.title, marker) else { continue };
        let key_ok = |r: &CalRow| {
            r.calendar_uid == ev.uid && (!ev.recurring || instant(&r.calendar_original_start) == Some(ev.original_start))
        };
        let free = |i: usize| !used.contains(&i);
        // A current-marker row wins; else a migrated (NULL marker) row with the same uid and start.
        let hit = rows.meetings.iter().enumerate().find(|(i, r)| {
            free(*i) && r.calendar_marker.as_deref() == Some(marker) && key_ok(r)
        });
        let adopt = || {
            rows.meetings.iter().enumerate().find(|(i, r)| {
                free(*i) && r.calendar_marker.is_none() && r.calendar_uid == ev.uid && instant(&r.calendar_start) == Some(ev.start)
            })
        };
        let adopting = hit.is_none();
        let Some((i, r)) = hit.or_else(adopt) else {
            let gone = dismissed.iter().any(|(u, o)| *u == ev.uid && (!ev.recurring || *o == Some(ev.original_start)));
            if !gone && ev.end > now && ev.start < window.to && ev.start + day > now {
                out.push(Action::Insert {
                    uid: ev.uid.clone(),
                    title,
                    start: ev.start.to_rfc3339(),
                    end: ev.end.to_rfc3339(),
                    original_start: ev.original_start.to_rfc3339(),
                });
            }
            continue;
        };
        used.insert(i);
        if frozen(r) {
            continue;
        }
        let id = r.id.clone();
        if adopting {
            out.push(Action::Adopt { id: id.clone(), original_start: ev.original_start.to_rfc3339() });
        }
        // shortcut: CalRow has no calendar_end, so an end-only change is not detected; upgrade by adding the column to CalRow.
        if instant(&r.calendar_start) != Some(ev.start) {
            out.push(Action::Move { id: id.clone(), start: ev.start.to_rfc3339(), end: ev.end.to_rfc3339() });
        }
        if r.title != title && r.title_source != "user" {
            out.push(Action::Rename { id: id.clone(), title });
        }
        if r.calendar_cancelled_at.is_some() {
            out.push(Action::Uncancel { id });
        }
    }

    if events.complete {
        for (i, r) in rows.meetings.iter().enumerate() {
            if used.contains(&i) || frozen(r) || r.calendar_marker.as_deref() != Some(marker) {
                continue;
            }
            let Some(start) = instant(&r.calendar_start) else { continue };
            let id = r.id.clone();
            if start + day <= now {
                if r.untouched {
                    out.push(Action::Delete { id, reason: DeleteReason::Past });
                }
            } else if window.from <= start && start < window.to {
                if r.untouched {
                    out.push(Action::Delete { id, reason: DeleteReason::Gone });
                } else if r.calendar_cancelled_at.is_none() {
                    out.push(Action::Cancel { id });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, h, 0, 0).unwrap()
    }
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap()
    }
    fn z(d: DateTime<Utc>) -> String {
        d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
    fn off(d: DateTime<Utc>) -> String {
        d.to_rfc3339()
    }

    fn ev(uid: &str, title: &str, start: DateTime<Utc>, orig: DateTime<Utc>, recurring: bool) -> Occurrence {
        Occurrence { uid: uid.into(), title: title.into(), start, end: start + Duration::hours(1), original_start: orig, recurring }
    }
    fn row(id: &str, uid: &str, start: DateTime<Utc>, orig: DateTime<Utc>) -> CalRow {
        CalRow {
            id: id.into(),
            title: "Sync".into(),
            title_source: "calendar".into(),
            calendar_uid: uid.into(),
            calendar_start: Some(off(start)),
            calendar_original_start: Some(off(orig)),
            calendar_marker: Some("📞".into()),
            calendar_cancelled_at: None,
            started_at: None,
            untouched: true,
        }
    }
    fn content(mut r: CalRow) -> CalRow {
        r.untouched = false;
        r
    }
    fn mv(id: &str, s: DateTime<Utc>) -> Action {
        Action::Move { id: id.into(), start: off(s), end: off(s + Duration::hours(1)) }
    }
    fn del(id: &str, reason: DeleteReason) -> Action {
        Action::Delete { id: id.into(), reason }
    }
    fn insert(uid: &str, title: &str, s: DateTime<Utc>, orig: DateTime<Utc>) -> Action {
        Action::Insert { uid: uid.into(), title: title.into(), start: off(s), end: off(s + Duration::hours(1)), original_start: off(orig) }
    }

    struct Case {
        name: &'static str,
        events: Vec<Occurrence>,
        complete: bool,
        rows: Vec<CalRow>,
        dismissed: Vec<(String, String)>,
        live: Option<&'static str>,
        marker: &'static str,
        want: Vec<Action>,
    }
    fn case(name: &'static str) -> Case {
        Case { name, events: vec![], complete: true, rows: vec![], dismissed: vec![], live: None, marker: "📞", want: vec![] }
    }

    #[test]
    fn plan_rules() {
        let day = Duration::days(1);
        let cases = vec![
            Case { events: vec![ev("u", "📞 Sync", t(10), t(10), false)], want: vec![insert("u", "Sync", t(10), t(10))], ..case("new_event_inserts") },
            Case { events: vec![ev("u", "Lunch", t(10), t(10), false)], ..case("non_marker_event_ignored") },
            Case {
                events: vec![ev("u", "📞 Sync", t(12), t(10), true)],
                rows: vec![row("a", "u", t(10), t(10))],
                want: vec![mv("a", t(12))],
                ..case("moved_recurring_by_original_start_moves")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(12) + day, t(10), true)],
                rows: vec![row("a", "u", t(10) + day, t(10) + day), row("b", "u", t(10), t(10))],
                // row "b" (original 10:00) is the match; "a" has another original start: gone -> deleted
                want: vec![mv("b", t(12) + day), del("a", DeleteReason::Gone)],
                ..case("recurring_other_occurrence_not_matched")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(12), t(12), false)],
                rows: vec![row("a", "u", t(10), t(10))],
                want: vec![mv("a", t(12))],
                ..case("one_off_moved_matches_on_uid")
            },
            Case {
                events: vec![ev("u", "📞 Planning", t(10), t(10), false)],
                rows: vec![row("a", "u", t(10), t(10))],
                want: vec![Action::Rename { id: "a".into(), title: "Planning".into() }],
                ..case("renamed_updates_title")
            },
            Case {
                events: vec![ev("u", "📞 Planning", t(10), t(10), false)],
                rows: vec![{ let mut r = row("a", "u", t(10), t(10)); r.title_source = "user".into(); r }],
                ..case("user_title_not_renamed")
            },
            Case { rows: vec![row("a", "u", t(10), t(10))], want: vec![del("a", DeleteReason::Gone)], ..case("gone_untouched_deletes") },
            Case { rows: vec![content(row("a", "u", t(10), t(10)))], want: vec![Action::Cancel { id: "a".into() }], ..case("gone_with_content_cancels") },
            Case {
                rows: vec![{ let mut r = content(row("a", "u", t(10), t(10))); r.calendar_cancelled_at = Some("x".into()); r }],
                ..case("already_cancelled_not_recancelled")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(10), t(10), false)],
                rows: vec![{ let mut r = content(row("a", "u", t(10), t(10))); r.calendar_cancelled_at = Some("x".into()); r }],
                want: vec![Action::Uncancel { id: "a".into() }],
                ..case("reappear_uncancels")
            },
            Case { rows: vec![row("a", "u", t(10) - day * 2, t(10) - day * 2)], want: vec![del("a", DeleteReason::Past)], ..case("past_untouched_deletes") },
            Case { rows: vec![content(row("a", "u", t(10) - day * 2, t(10) - day * 2))], ..case("past_with_content_stays") },
            Case {
                events: vec![ev("u", "📞 Planning", t(12), t(10), false)],
                rows: vec![row("a", "u", t(10), t(10))],
                live: Some("a"),
                ..case("live_target_untouched")
            },
            Case { rows: vec![row("a", "u", t(10), t(10))], live: Some("a"), ..case("live_target_gone_untouched") },
            Case {
                events: vec![ev("u", "📞 Planning", t(12), t(10), false)],
                rows: vec![{ let mut r = row("a", "u", t(10), t(10)); r.started_at = Some("s".into()); r }],
                ..case("started_at_row_untouched_and_not_reinserted")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(10), t(10), false)],
                rows: vec![row("gone", "g", t(11), t(11)), row("old", "o", t(10) - day * 2, t(10) - day * 2)],
                complete: false,
                want: vec![insert("u", "Sync", t(10), t(10))],
                ..case("incomplete_never_cancels_or_deletes")
            },
            Case { rows: vec![row("a", "u", t(10) + day * 9, t(10) + day * 9)], ..case("outside_window_never_cancelled") },
            Case {
                events: vec![ev("u", "🎙 Sync", t(10), t(10), false)],
                rows: vec![row("a", "u", t(10), t(10))],
                marker: "🎙",
                want: vec![insert("u", "Sync", t(10), t(10))],
                ..case("marker_change_leaves_old_rows")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(12), t(9), true)],
                rows: vec![{ let mut r = row("a", "u", t(12), t(12)); r.calendar_marker = None; r }],
                want: vec![Action::Adopt { id: "a".into(), original_start: off(t(9)) }],
                ..case("migrated_prep_row_adopted_no_duplicate")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(12), t(12), false)],
                rows: vec![{ let mut r = row("a", "u", t(10), t(10)); r.calendar_marker = None; r }],
                want: vec![insert("u", "Sync", t(12), t(12))],
                ..case("migrated_row_not_matching_left_alone")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(10), t(10), true)],
                dismissed: vec![("u".into(), z(t(10)))],
                ..case("dismissed_recurring_not_reinserted")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(10), t(10), true)],
                dismissed: vec![("u".into(), off(t(10)))],
                ..case("dismissed_recurring_plus_zero_form")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(11), t(11), true)],
                dismissed: vec![("u".into(), z(t(10)))],
                want: vec![insert("u", "Sync", t(11), t(11))],
                ..case("dismissed_other_occurrence_inserted")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(12), t(12), false)],
                dismissed: vec![("u".into(), "whatever".into())],
                ..case("dismissed_one_off_matches_uid_alone")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(10) - day * 2, t(10) - day * 2, false), ev("v", "📞 Over", t(5), t(5), false)],
                ..case("finished_events_not_inserted")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(10) + day * 8, t(10) + day * 8, false)],
                ..case("beyond_window_not_inserted")
            },
        ];
        for c in cases {
            let ex = ExpandResult { occurrences: c.events, complete: c.complete };
            let snap = Snapshot { meetings: c.rows, dismissed: c.dismissed };
            let win = Window { from: now(), to: now() + Duration::days(7) };
            assert_eq!(plan(&ex, &snap, c.live, c.marker, &win, now()), c.want, "{}", c.name);
        }
    }
}
