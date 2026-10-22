// Stats over the llm_usage log and the talk-time tables. Periods are local-time weeks
// (Monday 00:00) and months (the 1st 00:00). The pure helpers take any TimeZone, so the
// tests pin a zone instead of depending on the machine's.

use crate::state::AppState;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use tauri::State;

/// The most periods one query returns.
const MAX_PERIODS: u32 = 24;

const LLM_SELECT: &str = "SELECT at, provider, purpose, COALESCE(input_tokens, 0) AS input, \
    COALESCE(output_tokens, 0) AS output, COALESCE(cache_read_tokens, 0) AS cache_read, \
    COALESCE(cache_write_tokens, 0) AS cache_write, cost_usd FROM llm_usage";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Week,
    Month,
}

#[derive(Serialize)]
pub struct ProviderCost {
    pub provider: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub unpriced_calls: u32,
}

#[derive(Serialize)]
pub struct CostPeriod {
    pub start: String, // YYYY-MM-DD, local
    pub total_usd: f64,
    pub providers: Vec<ProviderCost>,
}

#[derive(Serialize)]
pub struct PurposeCost {
    pub purpose: String,
    pub tokens: u64,
    pub cost_usd: Option<f64>,
}

#[derive(Serialize)]
pub struct MeetingCost {
    pub total_usd: Option<f64>, // None if any call is unpriced
    pub tokens: u64,
    pub by_purpose: Vec<PurposeCost>,
}

#[derive(Serialize)]
pub struct TypeShare {
    pub template_id: Option<String>,
    pub template_name: String, // "No type" when template_id is None
    pub meetings: u32,
    pub self_seconds: f64,
    pub total_seconds: f64,
}

#[derive(Serialize)]
pub struct TalkTime {
    pub current: Vec<TypeShare>,
    pub previous: Vec<TypeShare>,
}

#[derive(Serialize)]
pub struct MeetingShare {
    pub meeting_id: String,
    pub title: String,
    pub date: String, // YYYY-MM-DD, local
    pub self_seconds: f64,
    pub total_seconds: f64,
}

#[derive(sqlx::FromRow)]
struct LlmRow {
    at: String, // RFC 3339 as written; read through `when` so one bad value cannot fail the query
    provider: String,
    purpose: String,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    cost_usd: Option<f64>,
}

impl LlmRow {
    /// None when `at` does not parse; such rows are left out of the stats.
    fn when(&self) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(&self.at).ok().map(|t| t.with_timezone(&Utc))
    }

    fn tokens(&self) -> u64 {
        (self.input + self.output + self.cache_read + self.cache_write) as u64
    }
}

#[derive(sqlx::FromRow)]
struct TalkRow {
    id: String,
    title: String,
    created_at: DateTime<Utc>,
    template_id: Option<String>,
    template_name: Option<String>,
    self_seconds: f64,
    total_seconds: f64,
}

/// Midnight, or the first instant after it when a DST change skips midnight.
fn start_of_day<Tz: TimeZone>(tz: &Tz, date: NaiveDate) -> DateTime<Tz> {
    (0..=3)
        .find_map(|h| tz.from_local_datetime(&date.and_hms_opt(h, 0, 0)?).earliest())
        .unwrap_or_else(|| tz.from_utc_datetime(&date.and_time(NaiveTime::MIN)))
}

fn period_start_in<Tz: TimeZone>(period: Period, now: &DateTime<Tz>, back: u32) -> DateTime<Tz> {
    let today = now.date_naive();
    let date = match period {
        // Whole days back from today, so a DST change inside the span does not shift the date.
        Period::Week => {
            today - Duration::days(i64::from(today.weekday().num_days_from_monday()) + 7 * i64::from(back))
        }
        Period::Month => {
            let index = i64::from(today.year()) * 12 + i64::from(today.month0()) - i64::from(back);
            NaiveDate::from_ymd_opt(index.div_euclid(12) as i32, index.rem_euclid(12) as u32 + 1, 1)
                .expect("month start is a valid date")
        }
    };
    start_of_day(&now.timezone(), date)
}

/// Local-time period starts: weeks start Monday 00:00, months the 1st 00:00. Index 0 = current.
// The commands use the generic helper so tests can pin a zone; this is the Local-zone interface.
#[allow(dead_code)]
pub fn period_start(period: Period, now: DateTime<Local>, back: u32) -> DateTime<Local> {
    period_start_in(period, &now, back)
}

/// The period index (0 = current) holding `at`, or None when `at` is older than `count` periods.
fn bucket<Tz: TimeZone>(period: Period, now: &DateTime<Tz>, at: &DateTime<Tz>, count: u32) -> Option<u32> {
    // Starts only move back as the index grows, so the first period that starts at or before `at` wins.
    (0..count).find(|&b| *at >= period_start_in(period, now, b))
}

/// Groups rows by the period of their `when` instant. Index 0 is the current period.
/// Rows without a time, or older than `count` periods, are left out.
fn bucket_rows<'a, Tz: TimeZone, T>(
    period: Period,
    now: &DateTime<Tz>,
    count: u32,
    rows: &'a [T],
    when: impl Fn(&T) -> Option<DateTime<Utc>>,
) -> Vec<Vec<&'a T>> {
    let mut out: Vec<Vec<&T>> = (0..count).map(|_| Vec::new()).collect();
    for row in rows {
        let Some(at) = when(row) else { continue };
        let at = at.with_timezone(&now.timezone());
        if let Some(b) = bucket(period, now, &at, count) {
            out[b as usize].push(row);
        }
    }
    out
}

fn cost_period(start: String, rows: &[&LlmRow]) -> CostPeriod {
    let mut by_provider: BTreeMap<&str, ProviderCost> = BTreeMap::new();
    for r in rows {
        let p = by_provider.entry(r.provider.as_str()).or_insert_with(|| ProviderCost {
            provider: r.provider.clone(),
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
            unpriced_calls: 0,
        });
        p.input_tokens += r.input as u64;
        p.output_tokens += r.output as u64;
        match r.cost_usd {
            Some(c) => p.cost_usd += c,
            None => p.unpriced_calls += 1,
        }
    }
    let providers: Vec<ProviderCost> = by_provider.into_values().collect();
    let total_usd: f64 = providers.iter().map(|p| p.cost_usd).sum();
    CostPeriod { start, total_usd, providers }
}

pub(crate) async fn cost_periods<Tz: TimeZone>(
    pool: &SqlitePool,
    period: Period,
    now: DateTime<Tz>,
    count: u32,
) -> sqlx::Result<Vec<CostPeriod>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let since = period_start_in(period, &now, count - 1).with_timezone(&Utc).to_rfc3339();
    let rows = sqlx::query_as::<_, LlmRow>(&format!("{LLM_SELECT} WHERE at >= ?"))
        .bind(since)
        .fetch_all(pool)
        .await?;
    let buckets = bucket_rows(period, &now, count, rows.as_slice(), LlmRow::when);
    Ok(buckets
        .iter()
        .enumerate()
        .map(|(b, rs)| {
            let start = period_start_in(period, &now, b as u32).date_naive().to_string();
            cost_period(start, rs)
        })
        .collect())
}

/// None when the meeting has no usage rows.
fn meeting_summary(rows: &[LlmRow]) -> Option<MeetingCost> {
    if rows.is_empty() {
        return None;
    }
    let mut by_purpose: BTreeMap<&str, PurposeCost> = BTreeMap::new();
    for r in rows {
        let p = by_purpose.entry(r.purpose.as_str()).or_insert_with(|| PurposeCost {
            purpose: r.purpose.clone(),
            tokens: 0,
            cost_usd: Some(0.0),
        });
        p.tokens += r.tokens();
        // One unpriced call makes the purpose's cost (and so the total) unknown.
        p.cost_usd = match (p.cost_usd, r.cost_usd) {
            (Some(sum), Some(c)) => Some(sum + c),
            _ => None,
        };
    }
    let by_purpose: Vec<PurposeCost> = by_purpose.into_values().collect();
    let tokens = by_purpose.iter().map(|p| p.tokens).sum();
    let total_usd = by_purpose.iter().try_fold(0.0, |acc, p| Some(acc + p.cost_usd?));
    Some(MeetingCost { total_usd, tokens, by_purpose })
}

pub(crate) async fn meeting_cost_for(pool: &SqlitePool, meeting_id: &str) -> sqlx::Result<Option<MeetingCost>> {
    let mut rows = sqlx::query_as::<_, LlmRow>(&format!("{LLM_SELECT} WHERE meeting_id = ?"))
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
    rows.retain(|r| r.when().is_some());
    Ok(meeting_summary(&rows))
}

async fn talk_rows(pool: &SqlitePool) -> sqlx::Result<Vec<TalkRow>> {
    // The type comes from the completed summary; one summary row per meeting.
    sqlx::query_as::<_, TalkRow>(
        "SELECT m.id, m.title, m.created_at, json_extract(p.result, '$.template_id') AS template_id, \
         json_extract(p.result, '$.template_name') AS template_name, \
         COALESCE(SUM(CASE WHEN s.idx = 0 THEN s.seconds END), 0.0) AS self_seconds, \
         SUM(s.seconds) AS total_seconds \
         FROM meetings m \
         JOIN meeting_speakers s ON s.meeting_id = m.id \
         LEFT JOIN summary_processes p ON p.meeting_id = m.id AND p.status = 'completed' \
         GROUP BY m.id",
    )
    .fetch_all(pool)
    .await
}

fn type_shares(rows: &[&TalkRow]) -> Vec<TypeShare> {
    let mut by_type: BTreeMap<Option<&str>, TypeShare> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.total_seconds > 0.0) {
        let share = by_type.entry(r.template_id.as_deref()).or_insert_with(|| TypeShare {
            template_id: r.template_id.clone(),
            template_name: match (&r.template_id, &r.template_name) {
                (None, _) => "No type".to_string(),
                (Some(_), Some(name)) => name.clone(),
                (Some(id), None) => id.clone(),
            },
            meetings: 0,
            self_seconds: 0.0,
            total_seconds: 0.0,
        });
        share.meetings += 1;
        share.self_seconds += r.self_seconds;
        share.total_seconds += r.total_seconds;
    }
    let mut shares: Vec<TypeShare> = by_type.into_values().collect();
    shares.sort_by(|a, b| b.total_seconds.total_cmp(&a.total_seconds));
    shares
}

pub(crate) async fn talk_time_for<Tz: TimeZone>(
    pool: &SqlitePool,
    period: Period,
    now: &DateTime<Tz>,
) -> sqlx::Result<TalkTime> {
    let rows = talk_rows(pool).await?;
    let buckets = bucket_rows(period, now, 2, rows.as_slice(), |r| Some(r.created_at));
    Ok(TalkTime {
        current: type_shares(&buckets[0]),
        previous: type_shares(&buckets[1]),
    })
}

pub(crate) async fn talk_meetings_for<Tz: TimeZone>(
    pool: &SqlitePool,
    period: Period,
    now: &DateTime<Tz>,
    back: u32,
    template_id: Option<&str>,
) -> sqlx::Result<Vec<MeetingShare>> {
    let rows = talk_rows(pool).await?;
    let mut out: Vec<MeetingShare> = rows
        .iter()
        .filter(|r| r.total_seconds > 0.0 && r.template_id.as_deref() == template_id)
        .filter(|r| {
            let at = r.created_at.with_timezone(&now.timezone());
            bucket(period, now, &at, back + 1) == Some(back)
        })
        .map(|r| MeetingShare {
            meeting_id: r.id.clone(),
            title: r.title.clone(),
            date: r.created_at.with_timezone(&now.timezone()).date_naive().to_string(),
            self_seconds: r.self_seconds,
            total_seconds: r.total_seconds,
        })
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(out)
}

#[tauri::command]
pub async fn stats_cost(state: State<'_, AppState>, period: Period, count: u32) -> Result<Vec<CostPeriod>, String> {
    if count > MAX_PERIODS {
        return Err(format!("count must be at most {MAX_PERIODS}"));
    }
    cost_periods(state.db_manager.pool(), period, Local::now(), count)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn meeting_cost(state: State<'_, AppState>, meeting_id: String) -> Result<Option<MeetingCost>, String> {
    meeting_cost_for(state.db_manager.pool(), &meeting_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stats_talk_time(state: State<'_, AppState>, period: Period) -> Result<TalkTime, String> {
    talk_time_for(state.db_manager.pool(), period, &Local::now())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stats_talk_time_meetings(
    state: State<'_, AppState>,
    period: Period,
    back: u32,
    template_id: Option<String>,
) -> Result<Vec<MeetingShare>, String> {
    if back >= MAX_PERIODS {
        return Err(format!("back must be below {MAX_PERIODS}"));
    }
    talk_meetings_for(state.db_manager.pool(), period, &Local::now(), back, template_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use chrono::{FixedOffset, MappedLocalTime, NaiveDateTime};
    use chrono::offset::Offset;

    /// Test zone: UTC+1, and UTC+2 from 2026-03-29 01:00Z to 2026-10-25 01:00Z (EU-like).
    #[derive(Clone, Copy, Debug)]
    struct Dst;

    #[derive(Clone, Copy, Debug)]
    struct DstOffset(FixedOffset);

    impl Offset for DstOffset {
        fn fix(&self) -> FixedOffset {
            self.0
        }
    }

    fn dst_offset_utc(utc: &NaiveDateTime) -> DstOffset {
        let at = |m, d| ndt(2026, m, d, 1);
        let summer = *utc >= at(3, 29) && *utc < at(10, 25);
        DstOffset(FixedOffset::east_opt(if summer { 7200 } else { 3600 }).unwrap())
    }

    impl TimeZone for Dst {
        type Offset = DstOffset;

        fn from_offset(_: &DstOffset) -> Self {
            Dst
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<DstOffset> {
            self.offset_from_local_datetime(&local.and_time(NaiveTime::MIN))
        }

        // Test approximation: the local time minus the standard offset picks the offset.
        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<DstOffset> {
            MappedLocalTime::Single(dst_offset_utc(&(*local - Duration::hours(1))))
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> DstOffset {
            dst_offset_utc(&utc.and_time(NaiveTime::MIN))
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> DstOffset {
            dst_offset_utc(utc)
        }
    }

    fn ndt(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    /// Wednesday 2026-10-21 15:00, UTC+2.
    fn wed_afternoon() -> DateTime<Dst> {
        Dst.with_ymd_and_hms(2026, 10, 21, 15, 0, 0).unwrap()
    }

    #[test]
    fn period_start_week_monday() {
        let now = wed_afternoon();
        assert_eq!(period_start_in(Period::Week, &now, 0).naive_local(), ndt(2026, 10, 19, 0));
        assert_eq!(period_start_in(Period::Week, &now, 1).naive_local(), ndt(2026, 10, 12, 0));
    }

    #[test]
    fn period_start_week_across_dst() {
        // Back one week from Wed 2026-04-01 crosses the 2026-03-29 change. Monday 00:00 is
        // 2026-03-23 at UTC+1, which is 2026-03-22 23:00Z; subtracting 168h would give 22:00Z.
        let now = Dst.with_ymd_and_hms(2026, 4, 1, 12, 0, 0).unwrap();
        let start = period_start_in(Period::Week, &now, 1);
        assert_eq!(start.naive_local(), ndt(2026, 3, 23, 0));
        assert_eq!(start.naive_utc(), ndt(2026, 3, 22, 23));
    }

    #[test]
    fn period_start_month() {
        let now = Dst.with_ymd_and_hms(2026, 1, 10, 12, 0, 0).unwrap();
        assert_eq!(period_start_in(Period::Month, &now, 1).naive_local(), ndt(2025, 12, 1, 0));
        assert_eq!(period_start_in(Period::Month, &now, 0).naive_local(), ndt(2026, 1, 1, 0));
    }

    async fn insert_usage(
        pool: &SqlitePool,
        at: &str,
        provider: &str,
        meeting_id: Option<&str>,
        (input, output, cache_read): (i64, i64, i64),
        cost: Option<f64>,
    ) {
        sqlx::query(
            "INSERT INTO llm_usage (at, provider, model, purpose, meeting_id, input_tokens, output_tokens, \
             cache_read_tokens, cache_write_tokens, cost_usd) VALUES (?, ?, 'm', 'ask', ?, ?, ?, ?, 0, ?)",
        )
        .bind(at)
        .bind(provider)
        .bind(meeting_id)
        .bind(input)
        .bind(output)
        .bind(cache_read)
        .bind(cost)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn bucket_costs_by_provider() {
        let pool = test_pool().await;
        insert_usage(&pool, "2026-10-20T10:00:00+00:00", "claude", None, (1_000_000, 100_000, 0), Some(1.5)).await;
        insert_usage(&pool, "2026-10-21T09:00:00+00:00", "openai", None, (2000, 1000, 0), Some(0.5)).await;
        insert_usage(&pool, "2026-10-14T12:00:00+00:00", "claude", None, (500, 500, 0), Some(2.0)).await;
        insert_usage(&pool, "2026-10-14T12:30:00+00:00", "claude", None, (100, 0, 0), None).await;
        insert_usage(&pool, "2026-09-01T00:00:00+00:00", "claude", None, (1, 1, 0), Some(9.0)).await;

        let periods = cost_periods(&pool, Period::Week, wed_afternoon(), 2).await.unwrap();

        assert_eq!(periods.len(), 2);
        assert_eq!(periods[0].start, "2026-10-19");
        assert_eq!(periods[0].total_usd, 2.0);
        assert_eq!(periods[0].providers.len(), 2);
        assert_eq!(periods[0].providers[0].provider, "claude");
        assert_eq!(periods[0].providers[0].input_tokens, 1_000_000);
        assert_eq!(periods[0].providers[0].output_tokens, 100_000);
        assert_eq!(periods[0].providers[0].cost_usd, 1.5);
        assert_eq!(periods[0].providers[0].unpriced_calls, 0);
        assert_eq!(periods[0].providers[1].provider, "openai");
        assert_eq!(periods[0].providers[1].cost_usd, 0.5);

        assert_eq!(periods[1].start, "2026-10-12");
        assert_eq!(periods[1].total_usd, 2.0);
        assert_eq!(periods[1].providers.len(), 1);
        assert_eq!(periods[1].providers[0].input_tokens, 600);
        assert_eq!(periods[1].providers[0].output_tokens, 500);
        assert_eq!(periods[1].providers[0].cost_usd, 2.0);
        assert_eq!(periods[1].providers[0].unpriced_calls, 1);
    }

    #[tokio::test]
    async fn unreadable_timestamp_row_is_skipped() {
        let pool = test_pool().await;
        insert_usage(&pool, "2026-10-20T10:00:00+00:00", "claude", Some("m1"), (100, 10, 0), Some(1.0)).await;
        insert_usage(&pool, "not a time", "claude", Some("m1"), (100, 10, 0), Some(5.0)).await;

        let periods = cost_periods(&pool, Period::Week, wed_afternoon(), 1).await.unwrap();
        assert_eq!(periods[0].total_usd, 1.0);
        assert_eq!(periods[0].providers[0].unpriced_calls, 0);

        let m1 = meeting_cost_for(&pool, "m1").await.unwrap().unwrap();
        assert_eq!(m1.total_usd, Some(1.0));
        assert_eq!(m1.tokens, 110);
    }

    #[tokio::test]
    async fn meeting_cost_unpriced_total_none() {
        let pool = test_pool().await;
        insert_usage(&pool, "2026-10-20T10:00:00+00:00", "claude", Some("m1"), (100, 10, 0), Some(0.25)).await;
        insert_usage(&pool, "2026-10-20T10:05:00+00:00", "claude", Some("m1"), (200, 20, 0), None).await;
        insert_usage(&pool, "2026-10-20T11:00:00+00:00", "openai", Some("m2"), (1, 1, 0), Some(0.5)).await;

        assert!(meeting_cost_for(&pool, "none").await.unwrap().is_none());

        let m1 = meeting_cost_for(&pool, "m1").await.unwrap().unwrap();
        assert_eq!(m1.total_usd, None);
        assert_eq!(m1.tokens, 330);
        assert_eq!(m1.by_purpose.len(), 1);
        assert_eq!(m1.by_purpose[0].purpose, "ask");
        assert_eq!(m1.by_purpose[0].tokens, 330);
        assert_eq!(m1.by_purpose[0].cost_usd, None);

        let m2 = meeting_cost_for(&pool, "m2").await.unwrap().unwrap();
        assert_eq!(m2.total_usd, Some(0.5));
        assert_eq!(m2.by_purpose[0].cost_usd, Some(0.5));
    }

    async fn insert_meeting(pool: &SqlitePool, id: &str, created_at: &str) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(format!("Title {id}"))
            .bind(created_at)
            .bind(created_at)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn insert_speakers(pool: &SqlitePool, id: &str, seconds: &[f64]) {
        for (idx, s) in seconds.iter().enumerate() {
            sqlx::query("INSERT INTO meeting_speakers (meeting_id, idx, seconds) VALUES (?, ?, ?)")
                .bind(id)
                .bind(idx as i64)
                .bind(s)
                .execute(pool)
                .await
                .unwrap();
        }
    }

    async fn insert_summary(pool: &SqlitePool, id: &str, template: Option<(&str, &str)>) {
        let result = match template {
            Some((tid, name)) => serde_json::json!({"markdown": "x", "template_id": tid, "template_name": name}),
            None => serde_json::json!({"markdown": "x"}),
        };
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) \
             VALUES (?, 'completed', 't', 't', ?)",
        )
        .bind(id)
        .bind(result.to_string())
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn talk_time_share_by_type() {
        let pool = test_pool().await;
        // Current week (from Mon 2026-10-19): two "standup" meetings and one untyped.
        insert_meeting(&pool, "m1", "2026-10-20T10:00:00Z").await;
        insert_speakers(&pool, "m1", &[60.0, 40.0]).await;
        insert_summary(&pool, "m1", Some(("standup", "Standup"))).await;
        insert_meeting(&pool, "m2", "2026-10-21T09:00:00Z").await;
        insert_speakers(&pool, "m2", &[30.0, 70.0]).await;
        insert_summary(&pool, "m2", Some(("standup", "Standup"))).await;
        insert_meeting(&pool, "m3", "2026-10-19T12:00:00Z").await;
        insert_speakers(&pool, "m3", &[10.0, 30.0]).await;
        // No speakers: excluded even with a summary.
        insert_meeting(&pool, "m4", "2026-10-20T11:00:00Z").await;
        insert_summary(&pool, "m4", Some(("one_on_one", "One-to-one"))).await;
        // Previous week.
        insert_meeting(&pool, "m5", "2026-10-13T10:00:00Z").await;
        insert_speakers(&pool, "m5", &[5.0, 5.0]).await;
        insert_summary(&pool, "m5", Some(("one_on_one", "One-to-one"))).await;

        let talk = talk_time_for(&pool, Period::Week, &wed_afternoon()).await.unwrap();

        assert_eq!(talk.current.len(), 2);
        let standup = &talk.current[0];
        assert_eq!(standup.template_id.as_deref(), Some("standup"));
        assert_eq!(standup.template_name, "Standup");
        assert_eq!(standup.meetings, 2);
        assert_eq!(standup.self_seconds, 90.0);
        assert_eq!(standup.total_seconds, 200.0);
        let untyped = &talk.current[1];
        assert_eq!(untyped.template_id, None);
        assert_eq!(untyped.template_name, "No type");
        assert_eq!(untyped.meetings, 1);
        assert_eq!(untyped.self_seconds, 10.0);
        assert_eq!(untyped.total_seconds, 40.0);

        assert_eq!(talk.previous.len(), 1);
        assert_eq!(talk.previous[0].template_name, "One-to-one");
        assert_eq!(talk.previous[0].meetings, 1);
        assert_eq!(talk.previous[0].total_seconds, 10.0);

        let untyped_meetings = talk_meetings_for(&pool, Period::Week, &wed_afternoon(), 0, None).await.unwrap();
        assert_eq!(untyped_meetings.len(), 1);
        assert_eq!(untyped_meetings[0].meeting_id, "m3");
        assert_eq!(untyped_meetings[0].date, "2026-10-19");
    }
}
