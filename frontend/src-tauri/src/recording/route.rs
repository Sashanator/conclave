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

// shortcut: process-local; after a restart a routed row is only returned to New, never deleted
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;

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
