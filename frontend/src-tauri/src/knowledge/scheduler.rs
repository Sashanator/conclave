//! The knowledge scheduler: one loop, every minute. It refreshes the calendar, makes the briefs and the weekly digest that are due and prunes the feed once a day.

use crate::knowledge::briefs::{self, due, event_type, make_brief};
use crate::knowledge::calendar::{self, Occurrence};
use crate::knowledge::digest;
use crate::knowledge::extract;
use crate::knowledge::feed;
use crate::knowledge::settings;
use crate::knowledge::CALENDAR_REFRESH_MINS;
use crate::state::AppState;
use crate::summary::templates::{self, TemplateEntry};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
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
        if let Err(e) = calendar::refresh(app).await {
            warn!("knowledge: calendar refresh failed: {e}");
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

/// Makes a brief for each occurrence that is due, has a type and has no brief yet. A brief that fails is logged and the rest go on.
pub(crate) async fn make_due_briefs<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    now: DateTime<Utc>,
    occurrences: &[Occurrence],
    templates: &[TemplateEntry],
) -> Result<(), String> {
    for occ in occurrences.iter().filter(|o| due(o, now)) {
        let Some(template) = event_type(&occ.title, templates) else {
            continue;
        };
        if brief_made(pool, occ).await? {
            continue;
        }
        if let Err(e) = make_brief(app, pool, occ, template).await {
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
    use chrono::TimeZone;
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
        Occurrence { uid: format!("{title}-uid"), title: title.into(), start, end: start + Duration::minutes(30) }
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
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
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
}
