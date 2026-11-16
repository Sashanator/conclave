//! Repeatable startup backfill for the redesign: gists for finished summaries and a meeting row for every prep.
//! Idempotent; never deletes or edits prep content.
use crate::knowledge::extract::gist_from_summary;
use chrono::Utc;
use sqlx::SqlitePool;

pub async fn run(pool: &SqlitePool) -> anyhow::Result<()> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT m.id, json_extract(s.result, '$.markdown') FROM meetings m
         JOIN summary_processes s ON s.meeting_id = m.id AND s.status = 'completed'
         WHERE m.gist IS NULL AND json_valid(s.result) AND json_type(s.result, '$.markdown') = 'text'",
    )
    .fetch_all(pool)
    .await?;
    for (id, md) in rows {
        if let Some(gist) = gist_from_summary(&md) {
            sqlx::query("UPDATE meetings SET gist = ? WHERE id = ? AND gist IS NULL")
                .bind(gist)
                .bind(id)
                .execute(pool)
                .await?;
        }
    }

    let preps: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, event_uid, event_start, event_title FROM meeting_preps
         WHERE meeting_id IS NULL AND event_uid IS NOT NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    for (prep_id, uid, start, title) in preps {
        let mut tx = pool.begin().await?;
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM meetings m WHERE calendar_uid = ? AND calendar_start = ?
             AND NOT EXISTS (SELECT 1 FROM meeting_preps p WHERE p.meeting_id = m.id)
             ORDER BY created_at, id LIMIT 1",
        )
        .bind(&uid)
        .bind(&start)
        .fetch_optional(&mut *tx)
        .await?;
        let meeting_id = match existing {
            Some((id,)) => id,
            None => {
                let id = format!("meeting-{}", uuid::Uuid::new_v4());
                let now = Utc::now().to_rfc3339();
                sqlx::query(
                    "INSERT INTO meetings (id, title, created_at, updated_at, title_source, calendar_uid, calendar_start, reviewed)
                     VALUES (?, ?, ?, ?, 'calendar', ?, ?, 0)",
                )
                .bind(&id)
                .bind(title.unwrap_or_else(|| "Untitled".into()))
                .bind(&now)
                .bind(&now)
                .bind(&uid)
                .bind(&start)
                .execute(&mut *tx)
                .await?;
                id
            }
        };
        sqlx::query("UPDATE meeting_preps SET meeting_id = ? WHERE id = ? AND meeting_id IS NULL")
            .bind(&meeting_id)
            .bind(prep_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
    Ok(())
}

#[cfg(test)]
mod redesign_backfill_tests {
    use super::*;
    use crate::database::repositories::test_pool;

    async fn exec(pool: &SqlitePool, sql: &str) {
        sqlx::query(sql).execute(pool).await.unwrap();
    }
    async fn prep_meeting(pool: &SqlitePool, uid: &str) -> String {
        sqlx::query_scalar("SELECT meeting_id FROM meeting_preps WHERE event_uid=?").bind(uid).fetch_one(pool).await.unwrap()
    }
    async fn seed(pool: &SqlitePool) {
        exec(pool, "INSERT INTO meetings (id,title,created_at,updated_at,calendar_uid,calendar_start) VALUES ('cal','Cal','2026-11-02T10:00:00+00:00','2026-11-02T10:00:00+00:00','u1','s1')").await;
        exec(pool, "INSERT INTO meeting_preps (event_uid,event_start,event_title,created_at,updated_at) VALUES ('u1','s1','P1','c','u'),('u2','s2','Board','c','u')").await;
    }

    #[tokio::test]
    async fn backfill_links_and_creates() {
        let pool = test_pool().await;
        seed(&pool).await;
        run(&pool).await.unwrap();
        assert_eq!(prep_meeting(&pool, "u1").await, "cal");
        let id = prep_meeting(&pool, "u2").await;
        type R = (String, String, String, String, Option<String>, Option<String>, Option<String>, i64);
        let r: R = sqlx::query_as("SELECT title,title_source,calendar_uid,calendar_start,calendar_marker,calendar_original_start,started_at,reviewed FROM meetings WHERE id=?")
            .bind(&id).fetch_one(&pool).await.unwrap();
        assert_eq!(r, ("Board".into(), "calendar".into(), "u2".into(), "s2".into(), None, None, None, 0));
    }

    #[tokio::test]
    async fn backfill_shared_key_links_once() {
        let pool = test_pool().await;
        exec(&pool, "INSERT INTO meetings (id,title,created_at,updated_at,calendar_uid,calendar_start) VALUES ('old','A','2026-11-01T10:00:00+00:00','x','u1','s1'),('new','B','2026-11-02T10:00:00+00:00','x','u1','s1')").await;
        exec(&pool, "INSERT INTO meeting_preps (event_uid,event_start,created_at,updated_at) VALUES ('u1','s1','c','u')").await;
        run(&pool).await.unwrap();
        assert_eq!(prep_meeting(&pool, "u1").await, "old");
    }

    #[tokio::test]
    async fn backfill_is_idempotent() {
        let pool = test_pool().await;
        seed(&pool).await;
        run(&pool).await.unwrap();
        let snap = || async {
            let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings").fetch_one(&pool).await.unwrap();
            let m: Vec<(i64, Option<String>)> = sqlx::query_as("SELECT id, meeting_id FROM meeting_preps ORDER BY id").fetch_all(&pool).await.unwrap();
            (n, m)
        };
        let first = snap().await;
        run(&pool).await.unwrap();
        assert_eq!(snap().await, first);
    }

    #[tokio::test]
    async fn backfill_gist() {
        let pool = test_pool().await;
        for id in ["a", "b"] {
            exec(&pool, &format!("INSERT INTO meetings (id,title,created_at,updated_at) VALUES ('{id}','T','c','u')")).await;
            exec(&pool, &format!("INSERT INTO summary_processes (meeting_id,status,created_at,updated_at,result) VALUES ('{id}','completed','t','t','{{\"markdown\":\"## S\\nWe agreed. More.\"}}')")).await;
        }
        exec(&pool, "UPDATE meetings SET gist='kept' WHERE id='b'").await;
        run(&pool).await.unwrap();
        let g: Vec<(String, Option<String>)> = sqlx::query_as("SELECT id, gist FROM meetings ORDER BY id").fetch_all(&pool).await.unwrap();
        assert_eq!(g, vec![("a".into(), Some("We agreed.".into())), ("b".into(), Some("kept".into()))]);
    }
}
