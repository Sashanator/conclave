//! The weekly digest: what the week's meetings and open tasks add up to. Made on Friday 09:00 local and listed in Briefs & digests.

use crate::copilot;
use crate::knowledge::briefs::{self, is_duplicate, start_key, BriefSummary};
use crate::knowledge::extract::{self, Reply, UNTRUSTED};
use crate::knowledge::feed;
use crate::knowledge::settings;
use crate::knowledge::tasks::stamp;
use crate::knowledge::{DIGEST_SUMMARY_HEAD_CHARS, DIGEST_TIMEOUT_SECS};
use crate::state::AppState;
use crate::summary::llm_client::LLMProvider;
use crate::summary::llm_target::complete;
use crate::summary::processor::language_name_from_code;
use crate::summary::templates::{self, TemplateEntry};
use crate::usage::{self, Purpose};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
use log::{error, info, warn};
use serde::Serialize;
use sqlx::SqlitePool;
use std::future::Future;
use tauri::{AppHandle, Runtime, State};
use uuid::Uuid;

/// Failed attempts per local day. The count starts again on the next day, and a new app start resets it too.
const MAX_FAILED_ATTEMPTS_PER_DAY: u32 = 3;

/// The Copilot answer languages: the codes of LANGUAGE_OPTIONS in lib/summary-languages.ts.
const COPILOT_LANGUAGE_CODES: &[&str] = &[
    "en", "zh", "zh-tw", "de", "es", "ru", "ko", "fr", "ja", "pt", "it", "nl", "pl", "ar", "hi", "ta", "tr", "vi", "th",
    "id", "sv", "cs", "da", "fi", "el", "he", "hu", "no", "ro", "uk",
];

/// A meeting in the period, as the digest prompt reads it.
#[derive(Debug, PartialEq)]
pub struct DigestMeeting {
    pub tag: String,
    pub title: String,
    pub date: String,
    pub kind: String,
    pub summary_head: String,
    pub decisions: Vec<String>,
    pub requests: Vec<String>,
}

/// A task line. `tag` names its meeting when it has one; manual tasks have none.
#[derive(Clone, Debug, PartialEq)]
pub struct DigestTask {
    pub text: String,
    pub tag: Option<String>,
}

/// The task lists: opened in the period, done in it, and still open from earlier.
#[derive(Debug, Default, PartialEq)]
pub struct DigestTasks {
    pub opened: Vec<DigestTask>,
    pub done: Vec<DigestTask>,
    pub open: Vec<DigestTask>,
}

/// A digest's meeting, for its tag. `id` is empty for a meeting that no longer exists.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingLink {
    pub id: String,
    pub title: String,
}

/// A stored digest. `meeting_links[i]` is the meeting for tag `M{i+1}`.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DigestView {
    pub period_start: String,
    pub period_end: String,
    pub markdown: String,
    pub meeting_links: Vec<MeetingLink>,
}

/// Failed digest attempts in the current local day, kept in memory.
#[derive(Default)]
pub(crate) struct Tries {
    day: Option<NaiveDate>,
    failures: u32,
}

impl Tries {
    /// Whether another attempt may run on `today`.
    pub(crate) fn may_try(&mut self, today: NaiveDate) -> bool {
        if self.day != Some(today) {
            *self = Self { day: Some(today), failures: 0 };
        }
        self.failures < MAX_FAILED_ATTEMPTS_PER_DAY
    }

    pub(crate) fn failed(&mut self) {
        self.failures += 1;
    }
}

/// The Friday 09:00 local at or before `now`.
pub fn last_friday_nine(now: DateTime<Local>) -> DateTime<Local> {
    let back = (i64::from(now.weekday().num_days_from_monday()) - 4).rem_euclid(7);
    let friday = now.date_naive() - Duration::days(back);
    let at = nine_on(friday);
    if at <= now {
        at
    } else {
        nine_on(friday - Duration::days(7))
    }
}

fn nine_on(day: NaiveDate) -> DateTime<Local> {
    let naive = day.and_hms_opt(9, 0, 0).expect("09:00 is a valid time");
    // A local day skipped by a clock change has no 09:00: read it as UTC rather than fail the digest step.
    Local
        .from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| Local.from_utc_datetime(&naive))
}

/// The digest period due at `now`, if none exists for it yet: (start, end) with end = last_friday_nine(now),
/// start = previous digest's end if newer than end-7d, else end-7d.
pub fn due_period(now: DateTime<Local>, last_end: Option<DateTime<Local>>) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let end = last_friday_nine(now);
    if last_end.is_some_and(|at| at >= end) {
        return None;
    }
    let week_ago = end - Duration::days(7);
    let start = match last_end {
        Some(at) if at > week_ago => at,
        _ => week_ago,
    };
    Some((start, end))
}

/// The system and user prompts. The meeting and task text is data, so the user prompt carries the untrusted line.
pub fn build_prompts(meetings: &[DigestMeeting], tasks: &DigestTasks, language: &str) -> (String, String) {
    let system = format!(
        "You write a weekly digest of the user's meetings, in {language}.\n\
         Return markdown with exactly these three sections, in this order: ## Your tasks, ## Most important meetings, ## Decisions.\n\
         Refer to meetings only by their tags, such as [M3], using only the tags in the material. No other text."
    );
    let mut user = format!("{UNTRUSTED}\n\nMeetings:\n");
    if meetings.is_empty() {
        user.push_str("(none)\n");
    }
    for m in meetings {
        user.push_str(&format!(
            "\n[{}] {} ({}, {})\nSummary:\n{}\nDecisions:\n{}\nRequests:\n{}\n",
            m.tag,
            m.title,
            m.date,
            m.kind,
            m.summary_head,
            bullets(&m.decisions),
            bullets(&m.requests)
        ));
    }
    user.push_str(&format!(
        "\nTasks opened in the period:\n{}\nTasks done in the period:\n{}\nTasks still open:\n{}",
        task_lines(&tasks.opened),
        task_lines(&tasks.done),
        task_lines(&tasks.open)
    ));
    (system, user)
}

fn bullets(items: &[String]) -> String {
    if items.is_empty() {
        return "(none)".to_string();
    }
    items.iter().map(|item| format!("- {item}")).collect::<Vec<_>>().join("\n")
}

fn task_lines(tasks: &[DigestTask]) -> String {
    if tasks.is_empty() {
        return "(none)".to_string();
    }
    tasks
        .iter()
        .map(|t| match &t.tag {
            Some(tag) => format!("- [{tag}] {}", t.text),
            None => format!("- {}", t.text),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The language name the digest is written in: the Copilot answer language, else the pinned language when Copilot
/// supports it, else Russian.
pub(crate) fn resolve_language(setting: Option<&str>, pinned: Option<&str>) -> &'static str {
    let code = setting.or(pinned.filter(|p| COPILOT_LANGUAGE_CODES.contains(p)));
    code.and_then(language_name_from_code).unwrap_or("Russian")
}

/// The summary up to its second heading, cut to DIGEST_SUMMARY_HEAD_CHARS characters.
pub(crate) fn summary_head(md: &str) -> String {
    let mut headings = 0;
    let mut end = md.len();
    let mut offset = 0;
    for line in md.split_inclusive('\n') {
        if line.trim_start().starts_with('#') {
            headings += 1;
            if headings == 2 {
                end = offset;
                break;
            }
        }
        offset += line.len();
    }
    md[..end].trim().chars().take(DIGEST_SUMMARY_HEAD_CHARS).collect()
}

/// The feed and list line: the period's first and last day, and the end month.
pub(crate) fn digest_line(start: DateTime<Local>, end: DateTime<Local>) -> String {
    format!("Weekly digest: {}–{} {}", start.day(), end.day(), end.format("%b"))
}

/// The scheduler's digest step. Makes the due digest when digests are on and today's attempts are not used up.
pub(crate) async fn digest_step<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    now: DateTime<Utc>,
    enabled: bool,
    tries: &mut Tries,
) {
    digest_step_with(app, pool, now, enabled, tries, |system, user| call_model(pool.clone(), system, user)).await;
}

/// The body of `digest_step`, with the model call injected so tests can count calls.
pub(crate) async fn digest_step_with<R: Runtime, F, Fut>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    now: DateTime<Utc>,
    enabled: bool,
    tries: &mut Tries,
    call: F,
) where
    F: Fn(String, String) -> Fut,
    Fut: Future<Output = Result<Reply, String>>,
{
    if !enabled {
        return;
    }
    let local = now.with_timezone(&Local);
    if !tries.may_try(local.date_naive()) {
        return;
    }
    let last_end = match last_period_end(pool).await {
        Ok(last) => last,
        Err(e) => {
            error!("knowledge: digest periods not read: {e}");
            return;
        }
    };
    let Some((start, end)) = due_period(local, last_end) else {
        return;
    };
    if let Err(e) = make_digest_with(app, pool, start, end, call).await {
        tries.failed();
        error!("knowledge: digest not made: {e}");
    }
}

async fn last_period_end(pool: &SqlitePool) -> Result<Option<DateTime<Local>>, String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT period_end FROM digests ORDER BY period_end DESC LIMIT 1")
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    row.map(|(end,)| parse_local(&end)).transpose()
}

fn parse_local(text: &str) -> Result<DateTime<Local>, String> {
    DateTime::parse_from_rfc3339(text)
        .map(|at| at.with_timezone(&Local))
        .map_err(|e| e.to_string())
}

/// Builds and stores the digest for [start, end) and pushes its feed item. Ok(None) = nothing made: no meetings in
/// the period, a meeting still extracting, or the digest already exists.
pub(crate) async fn make_digest<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    start: DateTime<Local>,
    end: DateTime<Local>,
) -> Result<Option<String>, String> {
    make_digest_with(app, pool, start, end, |system, user| call_model(pool.clone(), system, user)).await
}

/// The body of `make_digest`, with the model call injected so tests can count calls and return canned replies.
pub(crate) async fn make_digest_with<R: Runtime, F, Fut>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    start: DateTime<Local>,
    end: DateTime<Local>,
    call: F,
) -> Result<Option<String>, String>
where
    F: Fn(String, String) -> Fut,
    Fut: Future<Output = Result<Reply, String>>,
{
    let (from, to) = (start.with_timezone(&Utc), end.with_timezone(&Utc));
    let meetings = period_meetings(pool, from, to).await?;
    if meetings.is_empty() {
        return Ok(None);
    }
    // A meeting still being extracted: the digest waits for a later tick, so it has the meeting's lists.
    if meetings.iter().any(|m| extract::extraction_running(&m.id)) {
        return Ok(None);
    }

    let extract_on = settings::current().extract_enabled;
    let templates = tauri::async_runtime::spawn_blocking(|| templates::list_templates().templates)
        .await
        .unwrap_or_default();
    let mut tag_ids: Vec<String> = meetings.iter().map(|m| m.id.clone()).collect();
    let mut digest_meetings = Vec::with_capacity(meetings.len());
    for (i, m) in meetings.iter().enumerate() {
        let (decisions, requests) = match briefs::read_lists(pool, &m.id).await? {
            Some(lists) => lists,
            None => {
                if extract_on && may_extract(pool, &m.id).await? {
                    if let Err(e) = extract::run_one(pool, &m.id).await {
                        warn!("knowledge: digest could not extract a meeting: {e}");
                    }
                }
                briefs::read_lists(pool, &m.id).await?.unwrap_or_default()
            }
        };
        let (markdown, template_id) = summary_of(pool, &m.id).await?;
        digest_meetings.push(DigestMeeting {
            tag: format!("M{}", i + 1),
            title: m.title.clone(),
            date: m.created_at.with_timezone(&Local).format("%Y-%m-%d").to_string(),
            kind: type_name(&templates, template_id.as_deref()),
            summary_head: summary_head(&markdown),
            decisions,
            requests,
        });
    }
    let tasks = task_buckets(pool, from, to, &mut tag_ids).await?;

    let language = resolve_language(
        copilot::settings::current().answer_language.as_deref(),
        crate::get_language_preference_internal().as_deref(),
    );
    let (system, user) = build_prompts(&digest_meetings, &tasks, language);
    let reply = match tokio::time::timeout(std::time::Duration::from_secs(DIGEST_TIMEOUT_SECS), call(system, user)).await {
        Err(_) => return Err("The digest call timed out".to_string()),
        Ok(reply) => reply?,
    };
    usage::record(
        pool,
        &reply.provider,
        &reply.model,
        Purpose::Digest,
        None,
        None,
        reply.completion.usage.as_ref(),
    )
    .await;
    let markdown = reply.completion.content.trim().to_string();
    if markdown.is_empty() {
        return Err("The model returned no digest".to_string());
    }

    let id = Uuid::new_v4().to_string();
    let meeting_ids_json = serde_json::to_string(&tag_ids).map_err(|e| e.to_string())?;
    let line = digest_line(start, end);
    // The digest and its feed item commit together, as a brief's do.
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let inserted = sqlx::query(
        "INSERT INTO digests (id, period_start, period_end, markdown, meeting_ids_json, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(start_key(from))
    .bind(start_key(to))
    .bind(&markdown)
    .bind(meeting_ids_json)
    .bind(stamp())
    .execute(&mut *tx)
    .await;
    match inserted {
        Ok(_) => {}
        // Another run stored this period first.
        Err(e) if is_duplicate(&e) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    feed::insert_in(&mut *tx, "digest", &line, &id).await.map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    // Only after the commit, so the feed never points at a digest that was not saved.
    feed::announce(app);
    info!("knowledge: digest made, {} meetings, {} tags", digest_meetings.len(), tag_ids.len());
    Ok(Some(id))
}

/// Whether the digest may run this meeting's extraction now. A failed one waits for the user's Retry, as the worker's does.
async fn may_extract(pool: &SqlitePool, meeting_id: &str) -> Result<bool, String> {
    let status: Option<(String,)> = sqlx::query_as("SELECT status FROM meeting_extracts WHERE meeting_id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(status.is_none_or(|(s,)| s == "pending"))
}

/// The meeting's type name, or "no type" when it has no summary template that still exists.
fn type_name(templates: &[TemplateEntry], template_id: Option<&str>) -> String {
    template_id
        .and_then(|id| templates.iter().find(|t| t.id == id))
        .map_or_else(|| "no type".to_string(), |t| t.template.name.clone())
}

/// Resolves the knowledge model and makes one completion. Target errors are user-facing; call errors are not shown.
async fn call_model(pool: SqlitePool, system: String, user: String) -> Result<Reply, String> {
    let target = settings::target(&pool).await?;
    // The local sidecar cannot be cancelled, so a timed-out call would leave it busy (as extraction does).
    if target.provider == LLMProvider::BuiltInAI {
        return Err("Built-in AI is not supported for the digest".to_string());
    }
    let completion = complete(&target, &system, &user, None, None)
        .await
        .map_err(|_| "The model call failed".to_string())?;
    Ok(Reply {
        provider: target.provider.as_str().to_string(),
        model: target.model.clone(),
        completion,
    })
}

struct PeriodMeeting {
    id: String,
    title: String,
    created_at: DateTime<Utc>,
}

/// The meetings created in [from, to), oldest first. The first one is tag M1.
async fn period_meetings(pool: &SqlitePool, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<PeriodMeeting>, String> {
    let rows: Vec<(String, String, DateTime<Utc>)> = sqlx::query_as("SELECT id, title, created_at FROM meetings")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut meetings: Vec<PeriodMeeting> = rows
        .into_iter()
        .filter(|(_, _, at)| from <= *at && *at < to)
        .map(|(id, title, created_at)| PeriodMeeting { id, title, created_at })
        .collect();
    meetings.sort_by_key(|m| m.created_at);
    Ok(meetings)
}

/// The meeting's completed summary markdown and its template id. Both are empty when there is no summary.
async fn summary_of(pool: &SqlitePool, meeting_id: &str) -> Result<(String, Option<String>), String> {
    let raw: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT result FROM summary_processes WHERE meeting_id = ? AND status = 'completed'",
    )
    .bind(meeting_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .flatten();
    let value: Option<serde_json::Value> = raw.and_then(|r| serde_json::from_str(&r).ok());
    let markdown = value
        .as_ref()
        .and_then(|v| v["markdown"].as_str())
        .unwrap_or_default()
        .to_string();
    let template_id = value
        .as_ref()
        .and_then(|v| v["template_id"].as_str())
        .map(str::to_string);
    Ok((markdown, template_id))
}

/// The tag of a meeting. A meeting outside the period is added to the list, so its task still has a tag.
fn tag_for(tag_ids: &mut Vec<String>, meeting_id: String) -> String {
    let index = match tag_ids.iter().position(|id| *id == meeting_id) {
        Some(index) => index,
        None => {
            tag_ids.push(meeting_id);
            tag_ids.len() - 1
        }
    };
    format!("M{}", index + 1)
}

/// The tasks in three lists: opened in the period, done in it, and still open from before it. Dismissed tasks are left out.
async fn task_buckets(
    pool: &SqlitePool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tag_ids: &mut Vec<String>,
) -> Result<DigestTasks, String> {
    let rows: Vec<(Option<String>, String, String, DateTime<Utc>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT meeting_id, text, status, created_at, done_at FROM tasks WHERE status != 'dismissed' ORDER BY created_at, rowid",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let within = |at: &DateTime<Utc>| from <= *at && *at < to;
    let mut out = DigestTasks::default();
    for (meeting_id, text, status, created_at, done_at) in rows {
        let tag = meeting_id.map(|id| tag_for(tag_ids, id));
        let task = DigestTask { text, tag };
        if status == "done" && done_at.as_ref().is_some_and(within) {
            out.done.push(task.clone());
        }
        if within(&created_at) {
            out.opened.push(task);
        } else if status == "open" {
            out.open.push(task);
        }
    }
    Ok(out)
}

/// The digest with its meeting links looked up now. A meeting that is gone is shown as "meeting deleted".
pub(crate) async fn get(pool: &SqlitePool, id: &str) -> Result<Option<DigestView>, String> {
    let row: Option<(String, String, String, String)> =
        sqlx::query_as("SELECT period_start, period_end, markdown, meeting_ids_json FROM digests WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    let Some((period_start, period_end, markdown, ids_json)) = row else {
        return Ok(None);
    };
    let ids: Vec<String> = serde_json::from_str(&ids_json).map_err(|e| e.to_string())?;
    let mut meeting_links = Vec::with_capacity(ids.len());
    for meeting_id in ids {
        let title: Option<(String,)> = sqlx::query_as("SELECT title FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
        meeting_links.push(match title {
            Some((title,)) => MeetingLink { id: meeting_id, title },
            None => MeetingLink { id: String::new(), title: "meeting deleted".to_string() },
        });
    }
    Ok(Some(DigestView { period_start, period_end, markdown, meeting_links }))
}

/// The digests as Briefs & digests rows.
pub(crate) async fn summaries(pool: &SqlitePool) -> Result<Vec<BriefSummary>, String> {
    let rows: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT id, period_start, period_end, created_at FROM digests")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|(id, start, end, created_at)| {
            Ok(BriefSummary {
                id,
                kind: "digest".to_string(),
                title: digest_line(parse_local(&start)?, parse_local(&end)?),
                created_at,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn digest_get(state: State<'_, AppState>, id: String) -> Result<Option<DigestView>, String> {
    get(state.db_manager.pool(), &id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use crate::knowledge::extract::hold_in_flight;
    use crate::summary::llm_client::LlmCompletion;
    use std::cell::{Cell, RefCell};

    /// October 2026, local time. 23 Oct is a Friday.
    fn at(day: u32, h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, day, h, m, 0).unwrap()
    }

    fn reply(content: &str) -> Reply {
        Reply {
            provider: "fake".to_string(),
            model: "m".to_string(),
            completion: LlmCompletion {
                content: content.to_string(),
                reasoning_stripped: false,
                usage: None,
            },
        }
    }

    const CANNED: &str = "## Your tasks\n- [M1] Send the deck\n## Most important meetings\n- [M1] Sync\n## Decisions\n- [M1] Ship";

    /// A meeting with a completed 1:1 summary and a done extraction, so the digest does not call the model for it.
    async fn done_meeting(pool: &SqlitePool, id: &str, created_at: &str) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'Sync Ivan', ?, ?)")
            .bind(id)
            .bind(created_at)
            .bind(created_at)
            .execute(pool)
            .await
            .unwrap();
        let result = serde_json::json!({"markdown": "# Sync\nWe agreed on the plan.\n## Next\n- later", "template_id": "one_on_one"});
        sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) VALUES (?, 'completed', 't', 't', ?)")
            .bind(id)
            .bind(result.to_string())
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meeting_extracts (meeting_id, decisions_json, requests_json, status, extracted_at) VALUES (?, '[\"Ship the plan\"]', '[]', 'done', 't')")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn digest_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM digests").fetch_one(pool).await.unwrap()
    }

    async fn feed_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM feed_items WHERE kind = 'digest'").fetch_one(pool).await.unwrap()
    }

    #[test]
    fn last_friday_nine_cases() {
        assert_eq!(last_friday_nine(at(23, 8, 59)), at(16, 9, 0));
        assert_eq!(last_friday_nine(at(23, 9, 0)), at(23, 9, 0));
        assert_eq!(last_friday_nine(at(25, 12, 0)), at(23, 9, 0));
        assert_eq!(last_friday_nine(at(22, 10, 0)), at(16, 9, 0));
        assert_eq!(last_friday_nine(at(26, 0, 30)), at(23, 9, 0));
    }

    #[test]
    fn due_period_first_and_catchup() {
        // No digest yet: the week up to the last Friday 09:00.
        assert_eq!(due_period(at(26, 10, 0), None), Some((at(16, 9, 0), at(23, 9, 0))));
        // Started on Monday after a missed Friday: still the same period.
        assert_eq!(due_period(at(26, 10, 0), Some(at(16, 9, 0))), Some((at(16, 9, 0), at(23, 9, 0))));
        // Already made: nothing due.
        assert_eq!(due_period(at(26, 10, 0), Some(at(23, 9, 0))), None);
        assert_eq!(due_period(at(23, 8, 59), Some(at(16, 9, 0))), None);
    }

    #[test]
    fn prompt_has_sections_tags_language_safety() {
        let meetings = vec![DigestMeeting {
            tag: "M1".into(),
            title: "Sync Ivan".into(),
            date: "2026-10-19".into(),
            kind: "1:1".into(),
            summary_head: "We agreed on the plan.".into(),
            decisions: vec!["Ship the plan".into()],
            requests: vec![],
        }];
        let tasks = DigestTasks {
            opened: vec![DigestTask { text: "Send the deck".into(), tag: Some("M1".into()) }],
            ..Default::default()
        };

        let (system, user) = build_prompts(&meetings, &tasks, "German");

        for section in ["## Your tasks", "## Most important meetings", "## Decisions"] {
            assert!(system.contains(section), "missing {section}");
        }
        assert!(system.contains("[M3]"));
        assert!(system.contains("German"));
        assert!(user.contains(UNTRUSTED));
        assert!(user.contains("[M1] Sync Ivan"));
        assert!(user.contains("- [M1] Send the deck"));
        assert!(user.contains("We agreed on the plan."));
    }

    #[test]
    fn language_resolves_as_copilot() {
        assert_eq!(resolve_language(None, None), "Russian");
        assert_eq!(resolve_language(None, Some("de")), "German");
        assert_eq!(resolve_language(None, Some("auto")), "Russian");
        assert_eq!(resolve_language(Some("fr"), Some("de")), "French");
    }

    #[test]
    fn summary_head_cuts_at_second_heading_and_length() {
        let md = "# Sync\nWe talked.\n## Decisions\n- Ship\n## Next\n- x";
        assert_eq!(summary_head(md), "# Sync\nWe talked.");
        assert_eq!(summary_head(&"é".repeat(1000)).chars().count(), DIGEST_SUMMARY_HEAD_CHARS);
    }

    #[test]
    fn tries_limit_per_day() {
        let mut tries = Tries::default();
        let day = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        for _ in 0..3 {
            assert!(tries.may_try(day));
            tries.failed();
        }
        assert!(!tries.may_try(day));
        assert!(tries.may_try(day.succ_opt().unwrap()));
    }

    #[tokio::test]
    async fn no_meetings_no_digest() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        done_meeting(&pool, "no_meetings_outside", "2026-10-01T10:00:00Z").await;
        let calls = Cell::new(0);
        let call = |_s: String, _u: String| {
            calls.set(calls.get() + 1);
            async { Ok(reply(CANNED)) }
        };

        let made = make_digest_with(app.handle(), &pool, at(16, 9, 0), at(23, 9, 0), call).await.unwrap();

        assert_eq!(made, None);
        assert_eq!(calls.get(), 0);
        assert_eq!(digest_count(&pool).await, 0);
        assert_eq!(feed_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn digest_respects_disabled() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        done_meeting(&pool, "digest_respects_disabled", "2026-10-19T10:00:00Z").await;
        let now = at(23, 10, 0).with_timezone(&Utc);
        let calls = Cell::new(0);
        let call = |_s: String, _u: String| {
            calls.set(calls.get() + 1);
            async { Ok(reply(CANNED)) }
        };
        let mut tries = Tries::default();

        digest_step_with(app.handle(), &pool, now, false, &mut tries, call).await;
        assert_eq!(calls.get(), 0);
        assert_eq!(digest_count(&pool).await, 0);

        digest_step_with(app.handle(), &pool, now, true, &mut tries, call).await;
        assert_eq!(calls.get(), 1);
        assert_eq!(digest_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn digest_stored_with_feed_item() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        done_meeting(&pool, "digest_stored", "2026-10-19T10:00:00Z").await;
        sqlx::query("INSERT INTO tasks (id, meeting_id, text, owner, status, source, edited, created_at) VALUES ('t1', 'digest_stored', 'Send the deck', 'me', 'open', 'auto', 0, '2026-10-19T11:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        let sent = RefCell::new(String::new());
        let call = |_s: String, u: String| {
            sent.borrow_mut().push_str(&u);
            async { Ok(reply(CANNED)) }
        };

        let id = make_digest_with(app.handle(), &pool, at(16, 9, 0), at(23, 9, 0), call)
            .await
            .unwrap()
            .unwrap();

        let stored: (String, String) = sqlx::query_as("SELECT markdown, meeting_ids_json FROM digests WHERE id = ?")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, (CANNED.to_string(), r#"["digest_stored"]"#.to_string()));
        let feed: (String, String) = sqlx::query_as("SELECT title, target_id FROM feed_items WHERE kind = 'digest'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(feed, ("Weekly digest: 16–23 Oct".to_string(), id));
        assert!(sent.borrow().contains("[M1] Sync Ivan"));
        assert!(sent.borrow().contains("- [M1] Send the deck"));
    }

    #[tokio::test]
    async fn digest_waits_while_a_meeting_extracts() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        done_meeting(&pool, "digest_busy", "2026-10-19T10:00:00Z").await;
        let _busy = hold_in_flight("digest_busy");
        let calls = Cell::new(0);
        let call = |_s: String, _u: String| {
            calls.set(calls.get() + 1);
            async { Ok(reply(CANNED)) }
        };

        let made = make_digest_with(app.handle(), &pool, at(16, 9, 0), at(23, 9, 0), call).await.unwrap();

        assert_eq!(made, None);
        assert_eq!(calls.get(), 0);
        assert_eq!(digest_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn failed_extraction_is_not_retried_by_digest() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        done_meeting(&pool, "digest_failed", "2026-10-19T10:00:00Z").await;
        sqlx::query("UPDATE meeting_extracts SET status = 'failed', error = 'kept' WHERE meeting_id = 'digest_failed'")
            .execute(&pool)
            .await
            .unwrap();
        let call = |_s: String, _u: String| async { Ok(reply(CANNED)) };

        make_digest_with(app.handle(), &pool, at(16, 9, 0), at(23, 9, 0), call).await.unwrap();

        let error: Option<String> = sqlx::query_scalar("SELECT error FROM meeting_extracts WHERE meeting_id = 'digest_failed'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(error.as_deref(), Some("kept"));
    }

    #[tokio::test]
    async fn digest_get_marks_deleted_meetings() {
        let pool = test_pool().await;
        done_meeting(&pool, "digest_live", "2026-10-19T10:00:00Z").await;
        sqlx::query("INSERT INTO digests (id, period_start, period_end, markdown, meeting_ids_json, created_at) VALUES ('d1', '2026-10-16T09:00:00Z', '2026-10-23T09:00:00Z', 'x', '[\"digest_live\",\"digest_gone\"]', 't')")
            .execute(&pool)
            .await
            .unwrap();

        let view = get(&pool, "d1").await.unwrap().unwrap();

        assert_eq!(
            view.meeting_links,
            vec![
                MeetingLink { id: "digest_live".into(), title: "Sync Ivan".into() },
                MeetingLink { id: String::new(), title: "meeting deleted".into() },
            ]
        );
        assert!(get(&pool, "missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn digest_listed_with_its_period() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO digests (id, period_start, period_end, markdown, meeting_ids_json, created_at) VALUES ('d1', ?, ?, 'x', '[]', '2026-10-23T09:00:01Z')")
            .bind(start_key(at(16, 9, 0).with_timezone(&Utc)))
            .bind(start_key(at(23, 9, 0).with_timezone(&Utc)))
            .execute(&pool)
            .await
            .unwrap();

        let rows = summaries(&pool).await.unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "digest");
        assert_eq!(rows[0].title, "Weekly digest: 16–23 Oct");
    }
}
