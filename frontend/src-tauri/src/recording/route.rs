//! The one function that decides which meeting row a recording start goes into.
use crate::database::repositories::meeting::MeetingsRepository;
use crate::state::AppState;
use anyhow::bail;
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartSource {
    Sidebar,
    Popup,
    Tray,
    AutoDetect,
    Page(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteTarget {
    Existing(String),
    New { title: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutedMeeting {
    pub meeting_id: String,
    pub title: String,
}

// shortcut: process-local; after a restart nothing marks a row as ours, so a release only clears its started_at and never deletes it
static FRESH_ROUTED: Mutex<Option<String>> = Mutex::new(None);

/// True once if `resolve_target` created this row.
pub(crate) fn take_fresh(meeting_id: &str) -> bool {
    let mut g = FRESH_ROUTED.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_deref() == Some(meeting_id) {
        *g = None;
        true
    } else {
        false
    }
}

pub(crate) struct CallRow {
    pub id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Has transcripts or a started_at.
    pub recorded: bool,
}

fn call_marker() -> String {
    crate::knowledge::settings::current().calendar_marker
}

/// §6.7: the 📞 meeting in progress or starting within 15 minutes. Fresh ones first, then closest to now; if every
/// candidate is already recorded, a new "<title> · part 2" meeting (no calendar link keeps the reconcile key unique).
pub(crate) fn pick_call(rows: &[CallRow], now: DateTime<Utc>) -> Option<RouteTarget> {
    let lead = chrono::Duration::minutes(15);
    rows.iter()
        .filter(|r| r.start - lead <= now && now < r.end)
        .min_by_key(|r| (r.recorded, (r.start - now).abs(), r.start, r.id.clone()))
        .map(|r| if r.recorded { RouteTarget::New { title: Some(format!("{} · part 2", r.title)) } } else { RouteTarget::Existing(r.id.clone()) })
}

async fn call_rows(pool: &SqlitePool) -> Result<Vec<CallRow>, sqlx::Error> {
    let rows: Vec<(String, String, Option<String>, Option<String>, bool)> = sqlx::query_as(
        "SELECT m.id, m.title, m.calendar_start, m.calendar_end, \
                (m.started_at IS NOT NULL OR EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id)) \
         FROM meetings m WHERE m.calendar_marker = ? AND m.calendar_cancelled_at IS NULL",
    )
    .bind(call_marker())
    .fetch_all(pool)
    .await?;
    let parse = |s: &Option<String>| s.as_deref().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc));
    Ok(rows
        .into_iter()
        .filter_map(|(id, title, s, e, recorded)| match (parse(&s), parse(&e)) {
            (Some(start), Some(end)) => Some(CallRow { id, title, start, end, recorded }),
            // a call row we cannot time falls back to a new meeting, never an error that blocks the recording
            _ => {
                log::warn!("route: 📞 meeting {id} has an unreadable calendar time; skipped");
                None
            }
        })
        .collect())
}

pub async fn pick_target(pool: &SqlitePool, now: DateTime<Utc>, source: StartSource) -> anyhow::Result<RouteTarget> {
    Ok(match source {
        StartSource::Page(id) => RouteTarget::Existing(id),
        StartSource::Sidebar => RouteTarget::New { title: None },
        StartSource::Popup | StartSource::Tray | StartSource::AutoDetect => {
            pick_call(&call_rows(pool).await?, now).unwrap_or(RouteTarget::New { title: None })
        }
    })
}

pub async fn resolve_target(pool: &SqlitePool, now: DateTime<Utc>, source: StartSource) -> anyhow::Result<RoutedMeeting> {
    let id = match pick_target(pool, now, source).await? {
        RouteTarget::Existing(id) => {
            crate::audio::recording_commands::ensure_not_live_now(&id).map_err(anyhow::Error::msg)?;
            id
        }
        RouteTarget::New { title } => {
            let id = MeetingsRepository::create_meeting(pool, title.as_deref(), Local::now().naive_local()).await?;
            *FRESH_ROUTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.clone());
            id
        }
    };
    match MeetingsRepository::get_meeting_metadata(pool, &id).await? {
        Some(m) => Ok(RoutedMeeting { meeting_id: m.id, title: m.title }),
        None => bail!("This meeting no longer exists"),
    }
}

pub(crate) async fn mark_started(pool: &SqlitePool, meeting_id: &str, at: DateTime<Utc>) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("UPDATE meetings SET started_at = ?, updated_at = ? WHERE id = ? AND ended_at IS NULL \
         AND NOT EXISTS (SELECT 1 FROM transcripts WHERE meeting_id = ?)")
        .bind(at)
        .bind(at)
        .bind(meeting_id)
        .bind(meeting_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() == 1)
}

#[tauri::command]
pub async fn api_route_recording_start(state: tauri::State<'_, AppState>, source: StartSource) -> Result<RoutedMeeting, String> {
    resolve_target(state.db_manager.pool(), Utc::now(), source).await.map_err(|e| e.to_string())
}

/// A start failed after routing: gives the row back (deleted when we created it and it is still untouched). True = deleted.
pub(crate) async fn release_failed_start(pool: &SqlitePool, meeting_id: &str) -> Result<bool, sqlx::Error> {
    MeetingsRepository::release_routed_row(pool, meeting_id, take_fresh(meeting_id)).await
}

/// `release_failed_start`, except for the row a live recording owns (a start that lost a race releases its own
/// row, which must never be the winner's). False = nothing deleted.
pub(crate) async fn release_unless_live(pool: &SqlitePool, meeting_id: &str, refuse: bool) -> Result<bool, sqlx::Error> {
    if refuse {
        return Ok(false);
    }
    release_failed_start(pool, meeting_id).await
}

#[tauri::command]
pub async fn api_release_routed_start(state: tauri::State<'_, AppState>, meeting_id: String) -> Result<bool, String> {
    // Refuses the live recording's row, one whose save is running, and a continue target while busy.
    let refuse = crate::audio::recording_commands::ensure_not_live_now(&meeting_id).is_err();
    release_unless_live(state.db_manager.pool(), &meeting_id, refuse).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;

    // FRESH_ROUTED is one process-wide slot: tests that use it take turns.
    static FRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn row_exists(pool: &SqlitePool, id: &str) -> bool {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings WHERE id = ?").bind(id).fetch_one(pool).await.unwrap() == 1
    }

    #[tokio::test]
    async fn failed_start_deletes_the_fresh_row_it_created() {
        let _g = FRESH_LOCK.lock().await;
        let pool = test_pool().await;
        let r = resolve_target(&pool, Utc::now(), StartSource::Sidebar).await.unwrap();
        assert!(release_failed_start(&pool, &r.meeting_id).await.unwrap());
        assert!(!row_exists(&pool, &r.meeting_id).await);
    }

    #[tokio::test]
    async fn a_lost_race_never_releases_the_winners_row() {
        let _g = FRESH_LOCK.lock().await;
        let pool = test_pool().await;
        let r = resolve_target(&pool, Utc::now(), StartSource::Sidebar).await.unwrap();
        assert!(!release_unless_live(&pool, &r.meeting_id, true).await.unwrap());
        assert!(row_exists(&pool, &r.meeting_id).await, "the live target stays");
        assert!(release_unless_live(&pool, &r.meeting_id, false).await.unwrap());
        assert!(!row_exists(&pool, &r.meeting_id).await);
    }

    #[tokio::test]
    async fn routing_to_a_meeting_that_is_still_saving_is_refused() {
        let pool = test_pool().await;
        let id = MeetingsRepository::create_meeting(&pool, None, Local::now().naive_local()).await.unwrap();
        crate::audio::recording_commands::add_saving_target(&id);
        let e = resolve_target(&pool, Utc::now(), StartSource::Page(id.clone())).await.unwrap_err();
        assert!(e.to_string().contains("being recorded"), "{e}");
        crate::audio::recording_commands::remove_saving_target(&id);
        assert!(resolve_target(&pool, Utc::now(), StartSource::Page(id)).await.is_ok());
    }

    #[tokio::test]
    async fn failed_start_keeps_an_existing_row_and_clears_started_at() {
        let _g = FRESH_LOCK.lock().await;
        let pool = test_pool().await;
        let id = MeetingsRepository::create_meeting(&pool, None, Local::now().naive_local()).await.unwrap();
        assert!(mark_started(&pool, &id, Utc::now()).await.unwrap());
        assert!(!release_failed_start(&pool, &id).await.unwrap());
        assert!(row_exists(&pool, &id).await);
        let s: Option<String> = sqlx::query_scalar("SELECT started_at FROM meetings WHERE id = ?").bind(&id).fetch_one(&pool).await.unwrap();
        assert!(s.is_none());
    }

    #[tokio::test]
    async fn pick_target_page_is_existing() {
        let pool = test_pool().await;
        let t = pick_target(&pool, Utc::now(), StartSource::Page("m1".into())).await.unwrap();
        assert_eq!(t, RouteTarget::Existing("m1".into()));
    }

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 11, 17, h, m, 0).unwrap()
    }
    fn row(id: &str, start: DateTime<Utc>, mins: i64, recorded: bool) -> CallRow {
        CallRow { id: id.into(), title: "Sync".into(), start, end: start + chrono::Duration::minutes(mins), recorded }
    }

    #[test]
    fn route_cases() {
        let now = at(10, 0);
        let new = RouteTarget::New { title: None };
        let ex = |s: &str| RouteTarget::Existing(s.into());
        let pick = |rows: &[CallRow]| pick_call(rows, now).unwrap_or(RouteTarget::New { title: None });
        assert_eq!(pick(&[row("a", at(9, 40), 60, false)]), ex("a"), "in_progress");
        assert_eq!(pick(&[row("a", at(10, 15), 30, false)]), ex("a"), "starts_in_15");
        assert_eq!(pick(&[row("a", at(10, 16), 30, false)]), new, "starts_in_16");
        assert_eq!(pick(&[row("far", at(9, 0), 90, false), row("near", at(9, 50), 60, false)]), ex("near"), "overlap_closest_wins");
        assert_eq!(pick(&[row("done", at(9, 59), 60, true), row("fresh", at(10, 10), 30, false)]), ex("fresh"), "overlap_fresh_beats_closer_finished");
        assert_eq!(pick(&[row("done", at(9, 30), 60, true)]), RouteTarget::New { title: Some("Sync · part 2".into()) }, "finished");
        assert_eq!(pick(&[row("a", at(9, 0), 60, false)]), new, "ended");
        assert_eq!(pick(&[]), new);
    }

    async fn insert_call(pool: &SqlitePool, id: &str, marker: &str, start: DateTime<Utc>, cancelled: bool) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, calendar_uid, calendar_start, calendar_end, calendar_marker, calendar_cancelled_at) VALUES (?, 'Sync', 'c', 'c', ?, ?, ?, ?, ?)")
            .bind(id).bind(id).bind(start.to_rfc3339()).bind((start + chrono::Duration::minutes(30)).to_rfc3339())
            .bind(marker).bind(cancelled.then(|| "2026-11-17T00:00:00Z")).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn pick_target_routes_calls_for_popup_tray_autodetect_only() {
        let now = at(10, 0);
        for (name, marker, cancelled, want) in [
            ("due", "📞", false, RouteTarget::Existing("c".into())),
            ("cancelled_row_ignored", "📞", true, RouteTarget::New { title: None }),
            ("old_marker_row_ignored", "☎", false, RouteTarget::New { title: None }),
        ] {
            for s in [StartSource::Popup, StartSource::Tray, StartSource::AutoDetect] {
                let pool = test_pool().await;
                insert_call(&pool, "c", marker, at(10, 5), cancelled).await;
                assert_eq!(pick_target(&pool, now, s.clone()).await.unwrap(), want, "{name} {s:?}");
            }
        }
        let pool = test_pool().await;
        insert_call(&pool, "c", "📞", at(10, 5), false).await;
        assert_eq!(pick_target(&pool, now, StartSource::Sidebar).await.unwrap(), RouteTarget::New { title: None }, "due_call_still_new");
        assert_eq!(pick_target(&pool, now, StartSource::Page("p".into())).await.unwrap(), RouteTarget::Existing("p".into()), "page_id_wins");
        // a transcript or started_at marks it recorded
        sqlx::query("UPDATE meetings SET started_at = 'x' WHERE id = 'c'").execute(&pool).await.unwrap();
        assert_eq!(pick_target(&pool, now, StartSource::Popup).await.unwrap(), RouteTarget::New { title: Some("Sync · part 2".into()) });
    }

    #[tokio::test]
    async fn due_call_with_transcript_but_no_started_at_is_recorded() {
        let pool = test_pool().await;
        insert_call(&pool, "c", "📞", at(10, 5), false).await;
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('t', 'c', 'hi', 'x')").execute(&pool).await.unwrap();
        assert_eq!(pick_target(&pool, at(10, 0), StartSource::Popup).await.unwrap(), RouteTarget::New { title: Some("Sync · part 2".into()) });
    }

    #[tokio::test]
    async fn due_call_without_calendar_end_is_skipped() {
        let pool = test_pool().await;
        insert_call(&pool, "c", "📞", at(10, 5), false).await;
        sqlx::query("UPDATE meetings SET calendar_end = NULL WHERE id = 'c'").execute(&pool).await.unwrap();
        assert_eq!(pick_target(&pool, at(10, 0), StartSource::Popup).await.unwrap(), RouteTarget::New { title: None });
    }

    #[tokio::test]
    async fn resolve_new_creates_default_row() {
        let _g = FRESH_LOCK.lock().await;
        let pool = test_pool().await;
        let r = resolve_target(&pool, Utc::now(), StartSource::Sidebar).await.unwrap();
        assert!(r.title.starts_with("Untitled · "));
        let src: String = sqlx::query_scalar("SELECT title_source FROM meetings WHERE id = ?").bind(&r.meeting_id).fetch_one(&pool).await.unwrap();
        assert_eq!(src, "default");
        assert!(take_fresh(&r.meeting_id));
        assert!(!take_fresh(&r.meeting_id));
    }

    #[tokio::test]
    async fn resolve_page_of_missing_row_errs() {
        let pool = test_pool().await;
        let e = resolve_target(&pool, Utc::now(), StartSource::Page("nope".into())).await.unwrap_err();
        assert_eq!(e.to_string(), "This meeting no longer exists");
    }

    #[tokio::test]
    async fn mark_started_sets_and_reports_missing() {
        let pool = test_pool().await;
        let id = MeetingsRepository::create_meeting(&pool, None, Local::now().naive_local()).await.unwrap();
        assert!(mark_started(&pool, &id, Utc::now()).await.unwrap());
        let s: Option<String> = sqlx::query_scalar("SELECT started_at FROM meetings WHERE id = ?").bind(&id).fetch_one(&pool).await.unwrap();
        assert!(s.is_some());
        assert!(!mark_started(&pool, "missing", Utc::now()).await.unwrap());
    }

    #[tokio::test]
    async fn mark_started_leaves_finished_meeting_alone() {
        let pool = test_pool().await;
        for extra in [
            "UPDATE meetings SET ended_at = '2026-11-17T11:00:00Z' WHERE id = 'f'",
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('t', 'f', 'hi', 'x')",
        ] {
            sqlx::query("INSERT OR REPLACE INTO meetings (id, title, created_at, updated_at, started_at) VALUES ('f', 'T', 'c', 'c', '2026-11-17T10:00:00Z')").execute(&pool).await.unwrap();
            sqlx::query("DELETE FROM transcripts").execute(&pool).await.unwrap();
            sqlx::query(extra).execute(&pool).await.unwrap();
            assert!(!mark_started(&pool, "f", Utc::now()).await.unwrap(), "{extra}");
            let s: String = sqlx::query_scalar("SELECT started_at FROM meetings WHERE id = 'f'").fetch_one(&pool).await.unwrap();
            assert_eq!(s, "2026-11-17T10:00:00Z", "{extra}");
        }
    }

    #[test]
    fn start_source_from_frontend_json() {
        assert_eq!(serde_json::from_str::<StartSource>("\"sidebar\"").unwrap(), StartSource::Sidebar);
        assert_eq!(serde_json::from_str::<StartSource>("{\"page\":\"m1\"}").unwrap(), StartSource::Page("m1".into()));
    }
}
