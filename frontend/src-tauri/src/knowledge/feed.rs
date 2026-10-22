//! Notification feed: the briefs and digests the sidebar bell lists. Items point at their target and never own it.

use super::FEED_RETENTION_DAYS;
use crate::state::AppState;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use sqlx::{SqliteConnection, SqliteExecutor, SqlitePool};
use tauri::{AppHandle, Emitter, Runtime, State};

/// Newest items the bell lists.
const FEED_LIST_LIMIT: i64 = 100;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub target_id: String,
    pub created_at: String,
    pub read: bool,
}

// Fixed width, so the text order of created_at is the time order.
fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Inserts a feed item. The caller's executor decides whether it commits, so it can share a transaction.
async fn insert<'c>(exec: impl SqliteExecutor<'c>, kind: &str, title: &str, target_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO feed_items (id, kind, title, target_id, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(kind)
        .bind(title)
        .bind(target_id)
        .bind(stamp(Utc::now()))
        .execute(exec)
        .await?;
    Ok(())
}

/// Tells the open UI that the feed changed. Call it after the item's transaction has committed.
pub(crate) fn announce<R: Runtime>(app: &AppHandle<R>) {
    // A failed emit only delays the badge until the next refresh
    let _ = app.emit("feed-updated", ());
}

pub(crate) async fn push<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    kind: &str,
    title: &str,
    target_id: &str,
) -> Result<(), sqlx::Error> {
    insert(pool, kind, title, target_id).await?;
    announce(app);
    Ok(())
}

/// Inserts a feed item in the caller's transaction, for a brief written with it.
pub(crate) async fn insert_in(conn: &mut SqliteConnection, kind: &str, title: &str, target_id: &str) -> Result<(), sqlx::Error> {
    insert(conn, kind, title, target_id).await
}

pub(crate) async fn prune(pool: &SqlitePool, now: DateTime<Utc>) -> Result<u64, sqlx::Error> {
    let cutoff = stamp(now - Duration::days(FEED_RETENTION_DAYS));
    let result = sqlx::query("DELETE FROM feed_items WHERE created_at < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Removes the feed items for one target. The caller's transaction decides whether it commits.
pub(crate) async fn delete_for_target(conn: &mut SqliteConnection, kind: &str, target_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM feed_items WHERE kind = ? AND target_id = ?")
        .bind(kind)
        .bind(target_id)
        .execute(conn)
        .await?;
    Ok(())
}

async fn list(pool: &SqlitePool) -> Result<Vec<FeedItem>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, kind, title, target_id, created_at, read_at FROM feed_items ORDER BY created_at DESC, rowid DESC LIMIT ?",
    )
    .bind(FEED_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, kind, title, target_id, created_at, read_at)| FeedItem {
            id,
            kind,
            title,
            target_id,
            created_at,
            read: read_at.is_some(),
        })
        .collect())
}

async fn unread_count(pool: &SqlitePool) -> Result<u32, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM feed_items WHERE read_at IS NULL")
        .fetch_one(pool)
        .await?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

async fn mark_read(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE feed_items SET read_at = ? WHERE id = ? AND read_at IS NULL")
        .bind(stamp(Utc::now()))
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn mark_all_read(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE feed_items SET read_at = ? WHERE read_at IS NULL")
        .bind(stamp(Utc::now()))
        .execute(pool)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn feed_list(state: State<'_, AppState>) -> Result<Vec<FeedItem>, String> {
    list(state.db_manager.pool()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn feed_unread_count(state: State<'_, AppState>) -> Result<u32, String> {
    unread_count(state.db_manager.pool()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn feed_mark_read(state: State<'_, AppState>, id: String) -> Result<(), String> {
    mark_read(state.db_manager.pool(), &id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn feed_mark_all_read(state: State<'_, AppState>) -> Result<(), String> {
    mark_all_read(state.db_manager.pool()).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn push_list_count_mark() {
        let pool = test_pool().await;
        insert(&pool, "brief", "Brief: 1-2-1 Ivan at 11:00", "brief-1").await.unwrap();
        insert(&pool, "digest", "Weekly digest: 16–21 Oct", "digest-1").await.unwrap();

        assert_eq!(unread_count(&pool).await.unwrap(), 2);
        let items = list(&pool).await.unwrap();
        assert_eq!(items.len(), 2);
        // Newest first
        assert_eq!(items[0].kind, "digest");
        assert_eq!(items[0].title, "Weekly digest: 16–21 Oct");
        assert_eq!(items[0].target_id, "digest-1");
        assert!(!items[0].read);

        mark_read(&pool, &items[0].id).await.unwrap();
        assert_eq!(unread_count(&pool).await.unwrap(), 1);
        let items = list(&pool).await.unwrap();
        assert!(items[0].read);
        assert!(!items[1].read);
    }

    #[tokio::test]
    async fn prune_drops_old_only() {
        let pool = test_pool().await;
        let now = Utc::now();
        for (id, age_days) in [("old", FEED_RETENTION_DAYS + 1), ("recent", FEED_RETENTION_DAYS - 1)] {
            sqlx::query("INSERT INTO feed_items (id, kind, title, target_id, created_at) VALUES (?, 'brief', 't', 'x', ?)")
                .bind(id)
                .bind(stamp(now - Duration::days(age_days)))
                .execute(&pool)
                .await
                .unwrap();
        }

        assert_eq!(prune(&pool, now).await.unwrap(), 1);
        let ids: Vec<String> = list(&pool).await.unwrap().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, vec!["recent".to_string()]);
    }

    #[tokio::test]
    async fn mark_all_read() {
        let pool = test_pool().await;
        for n in 0..3 {
            insert(&pool, "brief", "Brief", &format!("brief-{n}")).await.unwrap();
        }
        assert_eq!(unread_count(&pool).await.unwrap(), 3);

        super::mark_all_read(&pool).await.unwrap();
        assert_eq!(unread_count(&pool).await.unwrap(), 0);
        assert!(list(&pool).await.unwrap().iter().all(|i| i.read));
    }

    #[tokio::test]
    async fn delete_for_target_removes_only_that_target() {
        let pool = test_pool().await;
        insert(&pool, "brief", "Brief", "brief-1").await.unwrap();
        insert(&pool, "brief", "Brief", "brief-2").await.unwrap();

        let mut tx = pool.begin().await.unwrap();
        delete_for_target(&mut *tx, "brief", "brief-1").await.unwrap();
        tx.commit().await.unwrap();

        let items = list(&pool).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].target_id, "brief-2");
    }
}
