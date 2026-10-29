// The LLM usage log: one row per model call, with tokens and cost. Rows are per meeting
// (meeting_id) or per recording until the recording is saved (recording_id).

pub mod prices;
pub mod stats;

use crate::summary::llm_client::TokenUsage;
use chrono::Utc;
use log::warn;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Purpose {
    Summary,
    TemplatePick,
    Extraction,
    Copilot,
    Dictate,
    Practice,
    Ask,
    Digest,
    ConnectionTest,
    Prep,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::Summary => "summary",
            Purpose::TemplatePick => "template_pick",
            Purpose::Extraction => "extraction",
            Purpose::Copilot => "copilot",
            Purpose::Dictate => "dictate",
            Purpose::Practice => "practice",
            Purpose::Ask => "ask",
            Purpose::Digest => "digest",
            Purpose::ConnectionTest => "connection_test",
            Purpose::Prep => "prep",
        }
    }
}

/// Never fails: a failed insert is logged (no content) and ignored. `None` usage writes nothing.
pub(crate) async fn record(
    pool: &SqlitePool,
    provider: &str,
    model: &str,
    purpose: Purpose,
    meeting_id: Option<&str>,
    recording_id: Option<&str>,
    usage: Option<&TokenUsage>,
) {
    let Some(u) = usage else { return };
    let cost_usd = u
        .reported_cost_usd
        .or_else(|| prices::cost(provider, model, u));
    // One statement, so a meeting deleted meanwhile leaves no orphan row.
    let result = sqlx::query(
        "INSERT INTO llm_usage (at, provider, model, purpose, meeting_id, recording_id, \
         input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_usd) \
         SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ? \
         WHERE ? IS NULL OR EXISTS (SELECT 1 FROM meetings WHERE id = ?)",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(provider)
    .bind(model)
    .bind(purpose.as_str())
    .bind(meeting_id)
    .bind(recording_id)
    .bind(u.input.map(|n| n as i64))
    .bind(u.output.map(|n| n as i64))
    .bind(u.cache_read.map(|n| n as i64))
    .bind(u.cache_write.map(|n| n as i64))
    .bind(cost_usd)
    .bind(meeting_id)
    .bind(meeting_id)
    .execute(pool)
    .await;
    if let Err(e) = result {
        warn!("usage: could not record {} usage: {e}", purpose.as_str());
    }
}

/// Copilot rows from a recording, attached when it is saved.
pub(crate) async fn attach_recording(
    pool: &SqlitePool,
    recording_id: &str,
    meeting_id: &str,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE llm_usage SET meeting_id = ? WHERE recording_id = ? AND meeting_id IS NULL \
         AND EXISTS (SELECT 1 FROM meetings WHERE id = ?)",
    )
    .bind(meeting_id)
    .bind(recording_id)
    .bind(meeting_id)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use crate::usage::prices::{set_cache_for_test, CACHE_TEST_LOCK};
    use std::sync::PoisonError;

    // Price lookups in record() read the shared cache, so these tests run one at a time.
    fn cache_guard() -> std::sync::MutexGuard<'static, ()> {
        CACHE_TEST_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    async fn insert_meeting(pool: &SqlitePool, id: &str) {
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'M', 't', 't')",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_row(pool: &SqlitePool, meeting_id: Option<&str>, recording_id: Option<&str>) {
        sqlx::query("INSERT INTO llm_usage (at, provider, model, purpose, meeting_id, recording_id) VALUES ('t', 'claude', 'm', 'ask', ?, ?)")
            .bind(meeting_id)
            .bind(recording_id)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn count(pool: &SqlitePool, where_sql: &str) -> i64 {
        sqlx::query_as::<_, (i64,)>(&format!("SELECT COUNT(*) FROM llm_usage WHERE {where_sql}"))
            .fetch_one(pool)
            .await
            .unwrap()
            .0
    }

    fn sonnet_usage(reported: Option<f64>) -> TokenUsage {
        TokenUsage {
            input: Some(1_000_000),
            output: Some(100_000),
            cache_read: None,
            cache_write: None,
            reported_cost_usd: reported,
        }
    }

    #[test]
    fn purpose_strings_match_the_contract() {
        assert_eq!(Purpose::ConnectionTest.as_str(), "connection_test");
        assert_eq!(Purpose::TemplatePick.as_str(), "template_pick");
        assert_eq!(Purpose::Ask.as_str(), "ask");
        assert_eq!(Purpose::Prep.as_str(), "prep");
    }

    #[tokio::test]
    async fn record_uses_reported_cost_first() {
        let _guard = cache_guard();
        set_cache_for_test(Vec::new());
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;

        record(
            &pool,
            "claude",
            "claude-sonnet-5-5",
            Purpose::Summary,
            Some("M"),
            None,
            Some(&sonnet_usage(Some(0.42))),
        )
        .await;
        record(
            &pool,
            "claude",
            "claude-sonnet-5-5",
            Purpose::Summary,
            Some("M"),
            None,
            Some(&sonnet_usage(None)),
        )
        .await;

        let costs: Vec<Option<f64>> =
            sqlx::query_as::<_, (Option<f64>,)>("SELECT cost_usd FROM llm_usage ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap()
                .into_iter()
                .map(|(c,)| c)
                .collect();
        assert_eq!(costs.len(), 2);
        assert_eq!(costs[0], Some(0.42));
        assert!((costs[1].unwrap() - 3.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn record_none_usage_writes_nothing() {
        let _guard = cache_guard();
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        record(
            &pool,
            "claude",
            "claude-sonnet-5-5",
            Purpose::Summary,
            Some("M"),
            None,
            None,
        )
        .await;
        assert_eq!(count(&pool, "1 = 1").await, 0);
    }

    #[tokio::test]
    async fn record_for_deleted_meeting_writes_nothing() {
        let _guard = cache_guard();
        let pool = test_pool().await;
        record(
            &pool,
            "claude",
            "claude-sonnet-5-5",
            Purpose::Ask,
            Some("ghost"),
            None,
            Some(&sonnet_usage(None)),
        )
        .await;
        assert_eq!(count(&pool, "1 = 1").await, 0);
        // A row with no meeting is kept whatever the meeting state.
        record(
            &pool,
            "claude",
            "claude-sonnet-5-5",
            Purpose::Copilot,
            None,
            Some("R"),
            Some(&sonnet_usage(None)),
        )
        .await;
        assert_eq!(
            count(&pool, "recording_id = 'R' AND meeting_id IS NULL").await,
            1
        );
    }

    #[tokio::test]
    async fn attach_recording_sets_meeting() {
        let _guard = cache_guard();
        let pool = test_pool().await;
        insert_meeting(&pool, "M").await;
        insert_row(&pool, None, Some("R")).await; // attached
        insert_row(&pool, Some("OTHER"), Some("R")).await; // already set, untouched
        insert_row(&pool, None, Some("S")).await; // other recording, untouched

        let attached = attach_recording(&pool, "R", "M").await.unwrap();

        assert_eq!(attached, 1);
        assert_eq!(count(&pool, "meeting_id = 'M'").await, 1);
        assert_eq!(count(&pool, "meeting_id = 'OTHER'").await, 1);
        assert_eq!(
            count(&pool, "recording_id = 'S' AND meeting_id IS NULL").await,
            1
        );
    }

    #[tokio::test]
    async fn attach_recording_to_deleted_meeting_changes_nothing() {
        let _guard = cache_guard();
        let pool = test_pool().await;
        insert_row(&pool, None, Some("R")).await;
        assert_eq!(attach_recording(&pool, "R", "ghost").await.unwrap(), 0);
        assert_eq!(count(&pool, "meeting_id IS NULL").await, 1);
    }
}
