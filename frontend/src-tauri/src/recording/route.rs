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

pub async fn pick_target(_pool: &SqlitePool, _now: DateTime<Utc>, source: StartSource) -> anyhow::Result<RouteTarget> {
    Ok(match source {
        StartSource::Page(id) => RouteTarget::Existing(id),
        _ => RouteTarget::New { title: None },
    })
}

pub async fn resolve_target(pool: &SqlitePool, now: DateTime<Utc>, source: StartSource) -> anyhow::Result<RoutedMeeting> {
    let id = match pick_target(pool, now, source).await? {
        RouteTarget::Existing(id) => id,
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
pub(crate) async fn release_unless_live(pool: &SqlitePool, meeting_id: &str, live: Option<String>) -> Result<bool, sqlx::Error> {
    if live.as_deref() == Some(meeting_id) {
        return Ok(false);
    }
    release_failed_start(pool, meeting_id).await
}

#[tauri::command]
pub async fn api_release_routed_start(state: tauri::State<'_, AppState>, meeting_id: String) -> Result<bool, String> {
    let live = crate::audio::recording_commands::recording_target_id();
    release_unless_live(state.db_manager.pool(), &meeting_id, live).await.map_err(|e| e.to_string())
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
        assert!(!release_unless_live(&pool, &r.meeting_id, Some(r.meeting_id.clone())).await.unwrap());
        assert!(row_exists(&pool, &r.meeting_id).await, "the live target stays");
        assert!(release_unless_live(&pool, &r.meeting_id, Some("other".into())).await.unwrap());
        assert!(!row_exists(&pool, &r.meeting_id).await);
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

    #[tokio::test]
    async fn pick_target_other_paths_are_new() {
        let pool = test_pool().await;
        for s in [StartSource::Sidebar, StartSource::Popup, StartSource::Tray, StartSource::AutoDetect] {
            assert_eq!(pick_target(&pool, Utc::now(), s).await.unwrap(), RouteTarget::New { title: None });
        }
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
