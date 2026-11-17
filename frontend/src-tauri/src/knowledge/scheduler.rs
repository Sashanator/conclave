//! The knowledge scheduler: one loop, every minute. It refreshes the calendar, makes the briefs and the weekly digest that are due and prunes the feed once a day.

use crate::knowledge::briefs::{self, due, event_type, make_brief};
use crate::knowledge::calendar::{self, Occurrence};
use crate::knowledge::digest;
use crate::knowledge::extract;
use crate::knowledge::feed;
use crate::knowledge::settings;
use crate::knowledge::calendar::reconcile::{self, Window};
use crate::knowledge::{CALENDAR_BEHIND_DAYS, CALENDAR_REFRESH_MINS, RECONCILE_AHEAD_DAYS};
use crate::state::AppState;
use crate::summary::templates::{self, TemplateEntry};
use chrono::{DateTime, Duration, Utc};
use log::{error, info, warn};
use sqlx::SqlitePool;
use std::time::Duration as StdDuration;
use tauri::{AppHandle, Manager, Runtime};

const TICK: StdDuration = StdDuration::from_secs(60);

/// When the calendar was last tried and the feed last pruned. Both are unset at start, so the first tick does each.
#[derive(Default)]
pub(crate) struct Clock {
    refreshed_at: Option<DateTime<Utc>>,
    pruned_at: Option<DateTime<Utc>>,
    digest: digest::Tries,
    /// The last good feed, kept until a reconcile on it succeeds, so a failed one retries without a fetch.
    pending: Option<calendar::ExpandResult>,
    /// When and for which calendar `pending` was fetched.
    pending_from: Option<(DateTime<Utc>, String)>,
}

impl Clock {
    #[cfg(test)]
    fn keep(&mut self, feed: calendar::ExpandResult, fetched: DateTime<Utc>, url: &str) {
        self.pending = Some(feed);
        self.pending_from = Some((fetched, url.to_string()));
    }

    /// The kept feed covers CALENDAR_BEHIND_DAYS before its fetch, so the reconcile window stays inside it
    /// only while the feed is under a day old (the 7 days ahead are covered for longer). A feed of another
    /// calendar, or of a disconnected one, would cancel the wrong meetings: drop it too.
    fn drop_unusable_pending(&mut self, now: DateTime<Utc>) {
        let url = settings::current().calendar_url;
        let usable = matches!(&self.pending_from, Some((at, from))
            if url.as_deref() == Some(from.as_str())
                && now >= *at
                && now - *at <= Duration::days(CALENDAR_BEHIND_DAYS));
        if self.pending.is_some() && !usable {
            warn!("knowledge: kept calendar feed dropped, it is stale or not the connected calendar");
        }
        if !usable {
            self.pending = None;
            self.pending_from = None;
        }
    }
}

/// Starts the loop once the database is ready, as the extraction worker does.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let pool = extract::wait_for_pool(|| {
            app.try_state::<AppState>()
                .map(|s| s.db_manager.pool().clone())
        })
        .await;
        let mut clock = Clock::default();
        loop {
            // The template folder is read on a blocking thread, not on the async runtime.
            let templates = match tauri::async_runtime::spawn_blocking(|| templates::list_templates().templates).await {
                Ok(templates) => templates,
                Err(e) => {
                    error!("knowledge: templates not read: {e}");
                    Vec::new()
                }
            };
            if let Err(e) = tick(&app, &pool, &mut clock, Utc::now(), &templates).await {
                error!("knowledge: scheduler tick failed: {e}");
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// The calendar is refreshed on the first tick, then every CALENDAR_REFRESH_MINS.
fn refresh_due(last: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    last.is_none_or(|at| now - at >= Duration::minutes(CALENDAR_REFRESH_MINS as i64))
}

/// The feed is pruned on the first tick, then once a day.
fn prune_due(last: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    last.is_none_or(|at| now - at >= Duration::days(1))
}

/// One pass: refresh the calendar if connected and due, prune if due, then make the due briefs.
pub(crate) async fn tick<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    clock: &mut Clock,
    now: DateTime<Utc>,
    templates: &[TemplateEntry],
) -> Result<(), String> {
    if refresh_due(clock.refreshed_at, now) && settings::current().calendar_url.is_some() {
        clock.refreshed_at = Some(now);
        match calendar::refresh(app, now).await {
            Ok(feed) => {
                clock.pending = Some(feed);
                clock.pending_from = settings::current().calendar_url.map(|url| (now, url));
            }
            Err(e) => {
                warn!("knowledge: calendar refresh failed: {e}");
                let _ = after_refresh(pool, Err(e), None, "", now).await;
            }
        }
    }
    clock.drop_unusable_pending(now);
    if let Some(feed) = clock.pending.clone() {
        let marker = settings::current().calendar_marker;
        let live = crate::audio::recording_commands::recording_target_id();
        if after_refresh(pool, Ok(feed), live.as_deref(), &marker, now).await.is_ok() {
            clock.pending = None;
            clock.pending_from = None;
        }
    }
    if prune_due(clock.pruned_at, now) {
        clock.pruned_at = Some(now);
        match feed::prune(pool, now).await {
            Ok(removed) => info!("knowledge: feed pruned, {removed} items removed"),
            Err(e) => warn!("knowledge: feed prune failed: {e}"),
        }
    }
    digest::digest_step(app, pool, now, settings::current().digest_enabled, &mut clock.digest).await;
    make_due_briefs(app, pool, now, &calendar::upcoming(), templates).await
}

/// Reconciles the meetings with a fetched feed. A failed fetch or reconcile reaches Settings as the last sync error
/// and changes nothing; the caller keeps the feed to retry.
pub(crate) async fn after_refresh(
    pool: &SqlitePool,
    result: Result<calendar::ExpandResult, String>,
    live: Option<&str>,
    marker: &str,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let outcome = match result {
        Ok(feed) => {
            let window = Window { from: now - Duration::days(CALENDAR_BEHIND_DAYS), to: now + Duration::days(RECONCILE_AHEAD_DAYS) };
            reconcile::run(pool, &feed, live, marker, &window, now).await.map(|changed| {
                info!("knowledge: calendar reconciled, {changed} changes");
            })
        }
        Err(e) => Err(e),
    };
    if let Err(e) = &outcome {
        calendar::record_sync_error(e.clone());
    }
    outcome
}

/// Makes a brief for each occurrence that is due, has a type and has no brief yet. A brief that fails is logged and the rest go on.
pub(crate) async fn make_due_briefs<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    now: DateTime<Utc>,
    occurrences: &[Occurrence],
    templates: &[TemplateEntry],
) -> Result<(), String> {
    // The clock the brief re-reads before it saves: the tick's time plus what has passed since (so a test clock works).
    let begun = std::time::Instant::now();
    let later = || now + Duration::from_std(begun.elapsed()).unwrap_or_else(|_| Duration::zero());
    for occ in occurrences.iter().filter(|o| due(o, now)) {
        let Some(template) = event_type(&occ.title, templates) else {
            continue;
        };
        if brief_made(pool, occ).await? {
            continue;
        }
        if let Err(e) = make_brief(app, pool, occ, template, &later).await {
            error!("knowledge: brief not made: {e}");
        }
    }
    Ok(())
}

/// Whether a brief already exists for this occurrence.
async fn brief_made(pool: &SqlitePool, occ: &Occurrence) -> Result<bool, String> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM briefs WHERE event_uid = ? AND event_start = ?")
        .bind(&occ.uid)
        .bind(briefs::start_key(occ.start))
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use chrono::{SecondsFormat, TimeZone};
    use uuid::Uuid;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 26, h, m, 0).unwrap()
    }

    fn one_on_one() -> TemplateEntry {
        let mut t: crate::summary::templates::Template =
            serde_json::from_str(r#"{"name":"1:1","sections":[{"title":"A","instruction":"B"}]}"#).unwrap();
        t.title_keywords = vec!["1:1".into()];
        TemplateEntry { id: "one_on_one".into(), template: t }
    }

    fn event(title: &str, start: DateTime<Utc>) -> Occurrence {
        Occurrence { uid: format!("{title}-uid"), title: title.into(), start, end: start + Duration::minutes(30), original_start: start, recurring: false }
    }

    /// `id` is unique per test: the extraction slots are process-wide, so tests must not share a meeting.
    async fn previous_meeting(pool: &SqlitePool, id: &str) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, '1:1 Ivan', '2026-10-19T10:00:00Z', 't')")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) VALUES (?, 'completed', 't', 't', '{\"template_id\":\"one_on_one\",\"markdown\":\"x\"}')")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn brief_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM briefs").fetch_one(pool).await.unwrap()
    }

    async fn feed_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM feed_items").fetch_one(pool).await.unwrap()
    }

    #[test]
    fn calendar_refresh_every_thirty_minutes_from_the_first_tick() {
        assert!(refresh_due(None, at(10, 0)));
        assert!(!refresh_due(Some(at(9, 31)), at(10, 0)));
        assert!(refresh_due(Some(at(9, 30)), at(10, 0)));
    }

    #[tokio::test]
    async fn brief_once_per_occurrence() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        previous_meeting(&pool, "brief_once_per_occurrence").await;
        let occurrences = vec![event("1:1 Ivan", at(11, 0))];
        let templates = vec![one_on_one()];

        make_due_briefs(app.handle(), &pool, at(10, 10), &occurrences, &templates).await.unwrap();
        make_due_briefs(app.handle(), &pool, at(10, 20), &occurrences, &templates).await.unwrap();

        assert_eq!(brief_count(&pool).await, 1);
        assert_eq!(feed_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn late_start_still_briefs() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        previous_meeting(&pool, "late_start_still_briefs").await;
        // The app starts 20 minutes before the meeting: the brief is still made.
        let occurrences = vec![event("1:1 Ivan", at(11, 0))];

        make_due_briefs(app.handle(), &pool, at(10, 40), &occurrences, &[one_on_one()]).await.unwrap();

        assert_eq!(brief_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn untyped_or_not_due_events_get_no_brief() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        previous_meeting(&pool, "untyped_or_not_due_events_get_no_brief").await;
        let occurrences = vec![event("Standup", at(11, 0)), event("1:1 Ivan", at(14, 0))];

        make_due_briefs(app.handle(), &pool, at(10, 40), &occurrences, &[one_on_one()]).await.unwrap();

        assert_eq!(brief_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn feed_is_pruned_once_a_day() {
        let _guard = crate::knowledge::SETTINGS_TEST_LOCK.lock().await;
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        // The digest feature has started. Otherwise the tick would store the start, and mock apps have no store.
        let started = crate::knowledge::settings::KnowledgeSettings {
            digest_since: Some(at(0, 0).to_rfc3339()),
            ..crate::knowledge::settings::current()
        };
        crate::knowledge::settings::set_cache(started);
        let mut clock = Clock::default();
        let templates: Vec<TemplateEntry> = Vec::new();
        let old_item = |created: DateTime<Utc>| {
            let pool = pool.clone();
            async move {
                sqlx::query("INSERT INTO feed_items (id, kind, title, target_id, created_at) VALUES (?, 'brief', 't', 'x', ?)")
                    .bind(Uuid::new_v4().to_string())
                    .bind(created.to_rfc3339_opts(SecondsFormat::Millis, true))
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        };

        tick(app.handle(), &pool, &mut clock, at(10, 0), &templates).await.unwrap();
        // Old by the time of the next tick, but the prune has run today already.
        old_item(at(10, 0) - Duration::days(31)).await;
        tick(app.handle(), &pool, &mut clock, at(11, 0), &templates).await.unwrap();
        assert_eq!(feed_count(&pool).await, 1);

        tick(app.handle(), &pool, &mut clock, at(10, 0) + Duration::hours(25), &templates).await.unwrap();
        assert_eq!(feed_count(&pool).await, 0);
    }

    const MARKER: &str = "📞";
    const URL: &str = "https://localhost:1/cal.ics";

    fn call_feed(start: DateTime<Utc>) -> calendar::ExpandResult {
        calendar::ExpandResult { occurrences: vec![event("📞 Sync", start)], complete: true }
    }

    async fn calendar_rows(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM meetings WHERE calendar_uid = '📞 Sync-uid'").fetch_one(pool).await.unwrap()
    }

    /// Settings with the calendar connected (or not) and the digest started, so a tick on a mock app does not touch the store.
    fn connect(url: Option<&str>) {
        settings::set_cache(settings::KnowledgeSettings {
            calendar_url: url.map(String::from),
            digest_since: Some(at(0, 0).to_rfc3339()),
            ..settings::current()
        });
    }

    #[tokio::test]
    async fn reconcile_runs_only_on_success() {
        let pool = test_pool().await;
        assert!(after_refresh(&pool, Err("down".into()), None, MARKER, at(10, 0)).await.is_err());
        assert_eq!(calendar_rows(&pool).await, 0);

        after_refresh(&pool, Ok(call_feed(at(14, 0))), None, MARKER, at(10, 0)).await.unwrap();
        assert_eq!(calendar_rows(&pool).await, 1);
    }

    #[tokio::test]
    async fn failed_fetch_waits_for_next_refresh() {
        let _guard = crate::knowledge::SETTINGS_TEST_LOCK.lock().await;
        connect(Some(URL));
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        let mut clock = Clock::default();

        tick(app.handle(), &pool, &mut clock, at(10, 0), &[]).await.unwrap();

        assert_eq!(clock.refreshed_at, Some(at(10, 0)));
        assert!(clock.pending.is_none());
        assert!(calendar::status().last_error_at.is_some());
        connect(None);
    }

    #[tokio::test]
    async fn failed_reconcile_retries_from_pending_without_refetch() {
        let _guard = crate::knowledge::SETTINGS_TEST_LOCK.lock().await;
        connect(Some(URL));
        let app = tauri::test::mock_app();
        let bad = test_pool().await;
        bad.close().await;
        let good = test_pool().await;
        // Refreshed a minute ago: the tick must not fetch again. A fetch would fail and leave `pending` None.
        let mut clock = Clock::default();
        clock.refreshed_at = Some(at(10, 0));
        clock.keep(call_feed(at(14, 0)), at(10, 0), URL);

        tick(app.handle(), &bad, &mut clock, at(10, 1), &[]).await.unwrap();
        assert!(clock.pending.is_some());
        assert!(calendar::status().last_error.is_some());

        tick(app.handle(), &good, &mut clock, at(10, 2), &[]).await.unwrap();
        assert!(clock.pending.is_none());
        assert_eq!(calendar_rows(&good).await, 1);
        assert_eq!(clock.refreshed_at, Some(at(10, 0)));
        connect(None);
    }

    #[tokio::test]
    async fn kept_feed_is_dropped_when_stale_or_for_another_calendar() {
        let _guard = crate::knowledge::SETTINGS_TEST_LOCK.lock().await;
        let app = tauri::test::mock_app();
        let pool = test_pool().await;
        for (url_now, fetched, now) in [
            (Some(URL), at(10, 0), at(10, 0) + Duration::hours(25)),
            (Some("https://localhost:2/other.ics"), at(10, 0), at(10, 1)),
            (None, at(10, 0), at(10, 1)),
        ] {
            connect(url_now);
            let mut clock = Clock::default();
            clock.refreshed_at = Some(now);
            clock.keep(call_feed(now + Duration::hours(4)), fetched, URL);
            tick(app.handle(), &pool, &mut clock, now, &[]).await.unwrap();
            assert!(clock.pending.is_none());
            assert_eq!(calendar_rows(&pool).await, 0);
        }
        connect(None);
    }
}
