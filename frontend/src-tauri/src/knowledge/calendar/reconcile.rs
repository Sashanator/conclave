// Pure calendar reconcile: calendar events + meeting rows -> actions. No I/O, never decides on
// content it cannot see: the caller's `untouched` flag is the only delete gate (spec 6.6, 4).

use super::{strip_marker, ExpandResult, Occurrence};
use crate::database::repositories::meeting::UNTOUCHED_SQL;
use chrono::{DateTime, Duration, Utc};
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;

pub struct CalRow {
    pub id: String,
    pub title: String,
    pub title_source: String,
    pub calendar_uid: String,
    pub calendar_start: Option<String>,
    pub calendar_end: Option<String>,
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

/// Canonical stored form (`Z`, whole seconds) for every instant the plan writes.
fn iso(d: DateTime<Utc>) -> String {
    d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
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
        let cands: Vec<_> = rows.meetings.iter().enumerate()
            .filter(|(i, r)| free(*i) && r.calendar_marker.as_deref() == Some(marker) && key_ok(r)).collect();
        // One-off keys are uid alone: prefer the row already at this start.
        let hit = cands.iter().find(|(_, r)| instant(&r.calendar_start) == Some(ev.start)).or(cands.first()).copied();
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
                    start: iso(ev.start),
                    end: iso(ev.end),
                    original_start: iso(ev.original_start),
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
            out.push(Action::Adopt { id: id.clone(), original_start: iso(ev.original_start) });
        }
        if instant(&r.calendar_start) != Some(ev.start) || instant(&r.calendar_end) != Some(ev.end) {
            out.push(Action::Move { id: id.clone(), start: iso(ev.start), end: iso(ev.end) });
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

/// Rows that carry a calendar uid, with the shared "untouched" test evaluated by the database.
pub async fn snapshot(pool: &SqlitePool) -> Result<Snapshot, String> {
    let sql = format!(
        "SELECT m.id, m.title, m.title_source, m.calendar_uid, m.calendar_start, m.calendar_end, m.calendar_original_start, \
         m.calendar_marker, m.calendar_cancelled_at, \
         CASE WHEN EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id) THEN COALESCE(m.started_at, 'recorded') ELSE m.started_at END AS started_at, \
         ({UNTOUCHED_SQL}) AS untouched \
         FROM meetings m WHERE m.calendar_uid IS NOT NULL"
    );
    let rows = sqlx::query(&sql).fetch_all(pool).await.map_err(|e| format!("calendar snapshot: {e}"))?;
    let meetings = rows
        .iter()
        .map(|r| {
            Ok(CalRow {
                id: r.try_get("id")?,
                title: r.try_get("title")?,
                title_source: r.try_get("title_source")?,
                calendar_uid: r.try_get("calendar_uid")?,
                calendar_start: r.try_get("calendar_start")?,
                calendar_end: r.try_get("calendar_end")?,
                calendar_original_start: r.try_get("calendar_original_start")?,
                calendar_marker: r.try_get("calendar_marker")?,
                calendar_cancelled_at: r.try_get("calendar_cancelled_at")?,
                started_at: r.try_get("started_at")?,
                untouched: r.try_get("untouched")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(|e| format!("calendar snapshot: {e}"))?;
    let dismissed = sqlx::query("SELECT calendar_uid, original_start FROM dismissed_occurrences")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("calendar snapshot (dismissed): {e}"))?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    Ok(Snapshot { meetings, dismissed })
}

// Every write re-checks its precondition inside its own statement: the snapshot may be stale by now.
// A row with a transcript was recorded (fallback saves leave started_at NULL): never edit it.
const NOT_RECORDED: &str = "started_at IS NULL AND NOT EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = meetings.id)";
const GUARD: &str = "started_at IS NULL AND calendar_marker = ? AND NOT EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = meetings.id)";
const CANCEL_SQL: &str =
    "UPDATE meetings SET calendar_cancelled_at = ? WHERE id = ? AND started_at IS NULL AND calendar_marker = ? AND calendar_cancelled_at IS NULL \
     AND NOT EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = meetings.id)";

/// Applies the plan; returns how many actions changed a row. The first failure aborts with its cause.
pub async fn apply(pool: &SqlitePool, actions: &[Action], marker: &str, now: DateTime<Utc>) -> Result<usize, String> {
    let now = now.to_rfc3339();
    let mut changed = 0;
    for a in actions {
        let ctx = |e: sqlx::Error| format!("calendar reconcile {a:?}: {e}");
        let n = match a {
            Action::Adopt { id, original_start } => sqlx::query(&format!(
                "UPDATE meetings SET calendar_marker = ?, calendar_original_start = ? WHERE id = ? AND calendar_marker IS NULL AND {NOT_RECORDED}"
            ))
            .bind(marker).bind(original_start).bind(id)
            .execute(pool).await.map_err(ctx)?.rows_affected(),
            Action::Insert { uid, title, start, end, original_start } => sqlx::query(
                "INSERT INTO meetings (id,title,title_source,created_at,updated_at,calendar_uid,calendar_start,calendar_end,calendar_original_start,calendar_marker) \
                 SELECT ?,?,'calendar',?,?,?,?,?,?,? \
                 WHERE NOT EXISTS (SELECT 1 FROM dismissed_occurrences d WHERE d.calendar_uid = ? AND julianday(d.original_start) = julianday(?)) \
                 AND NOT EXISTS (SELECT 1 FROM meetings WHERE calendar_uid = ? AND julianday(calendar_original_start) = julianday(?))",
            )
            .bind(format!("meeting-{}", uuid::Uuid::new_v4())).bind(title).bind(&now).bind(&now)
            .bind(uid).bind(start).bind(end).bind(original_start).bind(marker)
            .bind(uid).bind(original_start).bind(uid).bind(original_start)
            .execute(pool).await.map_err(ctx)?.rows_affected(),
            Action::Move { id, start, end } => {
                let mut tx = pool.begin().await.map_err(ctx)?;
                let n = sqlx::query(&format!("UPDATE meetings SET calendar_start = ?, calendar_end = ? WHERE id = ? AND {GUARD}"))
                    .bind(start).bind(end).bind(id).bind(marker)
                    .execute(&mut *tx).await.map_err(ctx)?.rows_affected();
                if n > 0 {
                    // A prep that cannot take the slot keeps its meeting_id link.
                    sqlx::query(
                        "UPDATE meeting_preps SET event_start = ? WHERE meeting_id = ? AND NOT EXISTS \
                         (SELECT 1 FROM meeting_preps q WHERE q.event_uid = meeting_preps.event_uid AND julianday(q.event_start) = julianday(?) AND q.meeting_id IS NOT meeting_preps.meeting_id)",
                    )
                    .bind(start).bind(id).bind(start)
                    .execute(&mut *tx).await.map_err(ctx)?;
                }
                tx.commit().await.map_err(ctx)?;
                n
            }
            Action::Rename { id, title } => sqlx::query(&format!("UPDATE meetings SET title = ? WHERE id = ? AND {GUARD} AND title_source <> 'user'"))
                .bind(title).bind(id).bind(marker)
                .execute(pool).await.map_err(ctx)?.rows_affected(),
            Action::Cancel { id } => sqlx::query(CANCEL_SQL).bind(&now).bind(id).bind(marker).execute(pool).await.map_err(ctx)?.rows_affected(),
            Action::Uncancel { id } => sqlx::query(&format!("UPDATE meetings SET calendar_cancelled_at = NULL WHERE id = ? AND {GUARD}"))
                .bind(id).bind(marker)
                .execute(pool).await.map_err(ctx)?.rows_affected(),
            Action::Delete { id, reason } => {
                let mut tx = pool.begin().await.map_err(ctx)?;
                let sql = format!("DELETE FROM meetings AS m WHERE m.id = ? AND m.calendar_marker = ? AND {UNTOUCHED_SQL}");
                let mut n = sqlx::query(&sql).bind(id).bind(marker).execute(&mut *tx).await.map_err(ctx)?.rows_affected();
                if n > 0 {
                    // Empty by the guard above, same transaction.
                    sqlx::query("DELETE FROM meeting_preps WHERE meeting_id = ?").bind(id).execute(&mut *tx).await.map_err(ctx)?;
                    sqlx::query("DELETE FROM meeting_notes WHERE meeting_id = ?").bind(id).execute(&mut *tx).await.map_err(ctx)?;
                } else if *reason == DeleteReason::Gone {
                    n = sqlx::query(CANCEL_SQL).bind(&now).bind(id).bind(marker).execute(&mut *tx).await.map_err(ctx)?.rows_affected();
                }
                tx.commit().await.map_err(ctx)?;
                n
            }
        };
        changed += usize::from(n > 0);
    }
    Ok(changed)
}

pub async fn run(
    pool: &SqlitePool,
    events: &ExpandResult,
    live_target: Option<&str>,
    marker: &str,
    window: &Window,
    now: DateTime<Utc>,
) -> Result<usize, String> {
    let snap = snapshot(pool).await?;
    apply(pool, &plan(events, &snap, live_target, marker, window, now), marker, now).await
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
            calendar_end: Some(off(start + Duration::hours(1))),
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
        Action::Move { id: id.into(), start: z(s), end: z(s + Duration::hours(1)) }
    }
    fn del(id: &str, reason: DeleteReason) -> Action {
        Action::Delete { id: id.into(), reason }
    }
    fn insert(uid: &str, title: &str, s: DateTime<Utc>, orig: DateTime<Utc>) -> Action {
        Action::Insert { uid: uid.into(), title: title.into(), start: z(s), end: z(s + Duration::hours(1)), original_start: z(orig) }
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
            Case {
                events: vec![ev("u", "📞 Sync", t(10), t(10), false)],
                rows: vec![{ let mut r = row("a", "u", t(10), t(10)); r.calendar_end = Some(off(t(10) + Duration::minutes(30))); r }],
                want: vec![mv("a", t(10))],
                ..case("end_only_change_moves")
            },
            Case {
                events: vec![ev("u", "📞 Sync", t(12), t(12), false)],
                rows: vec![row("a", "u", t(10), t(10)), row("b", "u", t(12), t(12))],
                want: vec![del("a", DeleteReason::Gone)], // "b" already sits at 12:00: it is the match, not first-by-order "a"
                ..case("one_off_prefers_exact_start_row")
            },
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
                want: vec![Action::Adopt { id: "a".into(), original_start: z(t(9)) }],
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
    mod exec {
        use super::super::*;
        use super::{now, t, z};
        use crate::database::repositories::test_pool;
        use sqlx::{Row, SqlitePool};

        const M: &str = "📞";
        fn ts(h: u32) -> String { t(h).to_rfc3339() } // `+00:00` form, as older rows store it
        fn zs(h: u32) -> String { z(t(h)) }

        async fn put(pool: &SqlitePool, id: &str, start: u32) {
            sqlx::query("INSERT INTO meetings (id,title,title_source,created_at,updated_at,calendar_uid,calendar_start,calendar_end,calendar_original_start,calendar_marker) \
                         VALUES (?,'Sync','calendar','c','c','u',?,?,?,?)")
                .bind(id).bind(ts(start)).bind(ts(start + 1)).bind(ts(start)).bind(M).execute(pool).await.unwrap();
        }
        async fn prep(pool: &SqlitePool, uid: &str, start: u32, meeting: Option<&str>, material: &str) {
            sqlx::query("INSERT INTO meeting_preps (event_uid,event_start,event_title,material,created_at,updated_at,meeting_id) VALUES (?,?,'T',?,'c','c',?)")
                .bind(uid).bind(ts(start)).bind(material).bind(meeting).execute(pool).await.unwrap();
        }
        async fn count(pool: &SqlitePool, sql: &str) -> i64 {
            sqlx::query(sql).fetch_one(pool).await.unwrap().get(0)
        }
        fn gone(id: &str) -> Action { Action::Delete { id: id.into(), reason: DeleteReason::Gone } }
        async fn cancelled(pool: &SqlitePool, id: &str) -> bool {
            let c: Option<String> = sqlx::query("SELECT calendar_cancelled_at FROM meetings WHERE id=?").bind(id).fetch_one(pool).await.unwrap().get(0);
            c.is_some()
        }

        #[tokio::test]
        async fn concurrent_prep_save_wins_over_delete() {
            let pool = test_pool().await;
            put(&pool, "a", 10).await;
            prep(&pool, "u", 10, Some("a"), "").await;
            let actions = vec![gone("a")]; // planned while untouched
            sqlx::query("UPDATE meeting_preps SET material='agenda' WHERE meeting_id='a'").execute(&pool).await.unwrap();
            assert_eq!(apply(&pool, &actions, M, now()).await.unwrap(), 1);
            assert_eq!(count(&pool, "SELECT COUNT(*) FROM meetings").await, 1);
            assert!(cancelled(&pool, "a").await);
            let m: String = sqlx::query("SELECT material FROM meeting_preps").fetch_one(&pool).await.unwrap().get(0);
            assert_eq!(m, "agenda");
        }

        #[tokio::test]
        async fn notes_json_only_note_survives_cancel() {
            let pool = test_pool().await;
            put(&pool, "a", 10).await;
            sqlx::query("INSERT INTO meeting_notes (meeting_id,notes_markdown,notes_json,created_at,updated_at) VALUES ('a','','[{\"t\":1}]','c','c')")
                .execute(&pool).await.unwrap();
            apply(&pool, &[gone("a")], M, now()).await.unwrap();
            assert_eq!(count(&pool, "SELECT COUNT(*) FROM meetings").await, 1);
            assert_eq!(count(&pool, "SELECT COUNT(*) FROM meeting_notes").await, 1);
            assert!(cancelled(&pool, "a").await);
        }

        #[tokio::test]
        async fn delete_removes_untouched_row_and_empty_prep() {
            let pool = test_pool().await;
            put(&pool, "a", 10).await;
            prep(&pool, "u", 10, Some("a"), "").await;
            sqlx::query("INSERT INTO meeting_notes (meeting_id,notes_markdown,notes_json,created_at,updated_at) VALUES ('a','','[]','c','c')")
                .execute(&pool).await.unwrap();
            assert_eq!(apply(&pool, &[gone("a")], M, now()).await.unwrap(), 1);
            assert_eq!(count(&pool, "SELECT COUNT(*) FROM meetings").await, 0);
            assert_eq!(count(&pool, "SELECT COUNT(*) FROM meeting_preps").await, 0);
            assert_eq!(count(&pool, "SELECT COUNT(*) FROM meeting_notes").await, 0);
        }

        #[tokio::test]
        async fn rename_after_user_rename_is_skipped() {
            let pool = test_pool().await;
            put(&pool, "a", 10).await;
            sqlx::query("UPDATE meetings SET title='Mine', title_source='user'").execute(&pool).await.unwrap();
            let n = apply(&pool, &[Action::Rename { id: "a".into(), title: "New".into() }], M, now()).await.unwrap();
            assert_eq!(n, 0);
            let title: String = sqlx::query("SELECT title FROM meetings").fetch_one(&pool).await.unwrap().get(0);
            assert_eq!(title, "Mine");
        }

        #[tokio::test]
        async fn move_carries_prep_event_start() {
            let pool = test_pool().await;
            put(&pool, "a", 10).await;
            prep(&pool, "u", 10, Some("a"), "x").await;
            let mv = Action::Move { id: "a".into(), start: ts(12), end: ts(13) };
            assert_eq!(apply(&pool, &[mv], M, now()).await.unwrap(), 1);
            let r = sqlx::query("SELECT calendar_start, calendar_end FROM meetings").fetch_one(&pool).await.unwrap();
            assert_eq!((r.get::<String, _>(0), r.get::<String, _>(1)), (ts(12), ts(13)));
            let s: String = sqlx::query("SELECT event_start FROM meeting_preps").fetch_one(&pool).await.unwrap().get(0);
            assert_eq!(s, ts(12));
        }

        #[tokio::test]
        async fn move_with_slot_taken_keeps_prep_link() {
            let pool = test_pool().await;
            put(&pool, "a", 10).await;
            prep(&pool, "u", 10, Some("a"), "x").await;
            prep(&pool, "u", 12, None, "other").await; // another prep already owns the target slot
            let mv = Action::Move { id: "a".into(), start: zs(12), end: zs(13) }; // Z-form target vs +00:00 stored slot
            apply(&pool, &[mv], M, now()).await.unwrap();
            let r = sqlx::query("SELECT event_start, meeting_id FROM meeting_preps WHERE material='x'").fetch_one(&pool).await.unwrap();
            assert_eq!((r.get::<String, _>(0), r.get::<Option<String>, _>(1)), (ts(10), Some("a".into())));
        }

        #[tokio::test]
        async fn started_at_written_after_snapshot_blocks_all() {
            let pool = test_pool().await;
            put(&pool, "a", 10).await;
            let actions = vec![
                Action::Move { id: "a".into(), start: ts(12), end: ts(13) },
                Action::Rename { id: "a".into(), title: "New".into() },
                Action::Cancel { id: "a".into() },
                Action::Uncancel { id: "a".into() },
                gone("a"),
                Action::Delete { id: "a".into(), reason: DeleteReason::Past },
            ];
            sqlx::query("UPDATE meetings SET started_at='s'").execute(&pool).await.unwrap();
            assert_eq!(apply(&pool, &actions, M, now()).await.unwrap(), 0);
            let r = sqlx::query("SELECT title, calendar_start, calendar_cancelled_at FROM meetings").fetch_one(&pool).await.unwrap();
            assert_eq!((r.get::<String, _>(0), r.get::<String, _>(1), r.get::<Option<String>, _>(2)), ("Sync".into(), ts(10), None));
        }

        #[tokio::test]
        async fn insert_skips_occurrence_dismissed_after_snapshot() {
            let pool = test_pool().await;
            let ins = Action::Insert { uid: "u".into(), title: "Sync".into(), start: ts(10), end: ts(11), original_start: ts(10) };
            sqlx::query("INSERT INTO dismissed_occurrences (calendar_uid, original_start, dismissed_at) VALUES ('u', ?, 'x')")
                .bind(t(10).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).execute(&pool).await.unwrap(); // `Z` form
            assert_eq!(apply(&pool, &[ins], M, now()).await.unwrap(), 0);
            assert_eq!(count(&pool, "SELECT COUNT(*) FROM meetings").await, 0);
        }

        #[tokio::test]
        async fn adopt_then_rerun_inserts_nothing() {
            let pool = test_pool().await;
            put(&pool, "a", 12).await;
            sqlx::query("UPDATE meetings SET calendar_marker=NULL, calendar_original_start=NULL, calendar_end=NULL").execute(&pool).await.unwrap();
            let ex = ExpandResult { occurrences: vec![
                Occurrence { uid: "u".into(), title: "📞 Sync".into(), start: t(12), end: t(13), original_start: t(9), recurring: true }], complete: true };
            let win = Window { from: now(), to: now() + Duration::days(7) };
            assert_eq!(run(&pool, &ex, None, M, &win, now()).await.unwrap(), 2); // adopt + fill calendar_end
            assert_eq!(run(&pool, &ex, None, M, &win, now()).await.unwrap(), 0);
            assert_eq!(count(&pool, "SELECT COUNT(*) FROM meetings").await, 1);
            let r = sqlx::query("SELECT calendar_marker, calendar_original_start, calendar_end FROM meetings").fetch_one(&pool).await.unwrap();
            assert_eq!((r.get::<String, _>(0), r.get::<String, _>(1), r.get::<String, _>(2)), (M.to_string(), zs(9), zs(13)));
        }

        #[tokio::test]
        async fn recorded_row_without_started_at_is_never_touched() {
            let pool = test_pool().await;
            put(&pool, "a", 12).await;
            sqlx::query("UPDATE meetings SET calendar_marker=NULL, calendar_original_start=NULL").execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('t','a','hi','x')").execute(&pool).await.unwrap();
            let ex = ExpandResult { occurrences: vec![
                Occurrence { uid: "u".into(), title: "📞 Other".into(), start: t(12), end: t(14), original_start: t(12), recurring: false }], complete: true };
            let win = Window { from: now(), to: now() + Duration::days(7) };
            assert!(plan(&ex, &snapshot(&pool).await.unwrap(), None, M, &win, now()).iter().all(|a| !matches!(a, Action::Adopt { .. } | Action::Move { .. } | Action::Rename { .. })));
            // and the executor refuses even a stale plan
            let stale = [Action::Adopt { id: "a".into(), original_start: zs(12) }, Action::Rename { id: "a".into(), title: "X".into() }, Action::Cancel { id: "a".into() }];
            sqlx::query("UPDATE meetings SET calendar_marker='📞'").execute(&pool).await.unwrap();
            assert_eq!(apply(&pool, &stale, M, now()).await.unwrap(), 0);
            let r = sqlx::query("SELECT title, calendar_cancelled_at FROM meetings").fetch_one(&pool).await.unwrap();
            assert_eq!((r.get::<String, _>(0), r.get::<Option<String>, _>(1)), ("Sync".into(), None));
        }

        #[tokio::test]
        async fn insert_dedupes_against_plus_zero_stored_value() {
            let pool = test_pool().await;
            put(&pool, "a", 10).await; // stored `+00:00`
            let ins = Action::Insert { uid: "u".into(), title: "Sync".into(), start: zs(10), end: zs(11), original_start: zs(10) };
            assert_eq!(apply(&pool, &[ins], M, now()).await.unwrap(), 0);
            assert_eq!(count(&pool, "SELECT COUNT(*) FROM meetings").await, 1);
        }

        #[tokio::test]
        async fn end_only_change_is_written() {
            let pool = test_pool().await;
            put(&pool, "a", 10).await;
            let ex = ExpandResult { occurrences: vec![
                Occurrence { uid: "u".into(), title: "📞 Sync".into(), start: t(10), end: t(12), original_start: t(10), recurring: false }], complete: true };
            let win = Window { from: now(), to: now() + Duration::days(7) };
            assert_eq!(run(&pool, &ex, None, M, &win, now()).await.unwrap(), 1);
            let e: String = sqlx::query("SELECT calendar_end FROM meetings").fetch_one(&pool).await.unwrap().get(0);
            assert_eq!(e, zs(12));
        }
    }
}
