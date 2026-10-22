//! Pre-meeting briefs: what the last meeting of the same type and title left open, shown before the next one.

use crate::knowledge::calendar::Occurrence;
use crate::knowledge::digest;
use crate::knowledge::feed;
use crate::knowledge::settings;
use crate::knowledge::tasks::{self, normalise, stamp, TaskRow};
use crate::knowledge::BRIEF_LEAD_MINS;
use crate::notifications::commands::{show_brief_toast, NotificationManagerState};
use crate::state::AppState;
use crate::summary::templates::TemplateEntry;
use chrono::{DateTime, Duration, Local, SecondsFormat, Utc};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, Runtime, State, Wry};
use uuid::Uuid;

/// The brief as the page reads it. `open_tasks` is filled from the live task table, never stored.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BriefContent {
    pub event_title: String,
    pub event_start: String,
    pub template_name: String,
    pub previous_meeting_id: String,
    pub previous_title: String,
    pub previous_date: String,
    pub requests: Vec<String>,
    pub decisions: Vec<String>,
    pub open_tasks: Vec<TaskRow>,
}

/// content_json: the brief, with the ids of its open tasks in place of the tasks, which are looked up at read time.
#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(flatten)]
    content: BriefContent,
    open_task_ids: Vec<String>,
}

/// A row in the Briefs & digests list.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BriefSummary {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub created_at: String,
}

/// The first template whose title keywords match the event title.
pub fn event_type<'a>(title: &str, templates: &'a [TemplateEntry]) -> Option<&'a TemplateEntry> {
    templates.iter().find(|t| t.template.matches_title(title))
}

/// Is this occurrence due for a brief now? From BRIEF_LEAD_MINS before the start, up to the start.
pub fn due(occ: &Occurrence, now: DateTime<Utc>) -> bool {
    occ.start - Duration::minutes(BRIEF_LEAD_MINS) <= now && now < occ.start
}

/// Titles match case-insensitively with runs of whitespace collapsed.
pub(crate) fn same_title(a: &str, b: &str) -> bool {
    normalise(a) == normalise(b)
}

/// The UNIQUE key of a brief's event: the start in one fixed format.
pub(crate) fn start_key(start: DateTime<Utc>) -> String {
    start.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The feed and toast line, with the start in local time.
fn brief_line(occ: &Occurrence) -> String {
    format!("Brief: {} at {}", occ.title, occ.start.with_timezone(&Local).format("%H:%M"))
}

/// A past meeting that a brief can look back at.
struct Past {
    id: String,
    title: String,
    created_at: String,
}

/// Meetings with a completed summary of this template and a matching title, newest first.
async fn matching_meetings(pool: &SqlitePool, template_id: &str, title: &str) -> Result<Vec<Past>, String> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT m.id, m.title, m.created_at FROM meetings m JOIN summary_processes s ON s.meeting_id = m.id \
         WHERE s.status = 'completed' AND CASE WHEN json_valid(s.result) THEN json_extract(s.result, '$.template_id') END = ? \
         ORDER BY m.created_at DESC",
    )
    .bind(template_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter(|(_, meeting_title, _)| same_title(meeting_title, title))
        .map(|(id, title, created_at)| Past { id, title, created_at })
        .collect())
}

/// The open tasks of these meetings, in the order the task list shows them.
async fn open_task_ids(pool: &SqlitePool, meeting_ids: &[String]) -> Result<Vec<String>, String> {
    let open = tasks::list(pool, "open").await.map_err(|e| e.to_string())?;
    Ok(open
        .into_iter()
        .filter(|t| t.meeting_id.as_ref().is_some_and(|m| meeting_ids.contains(m)))
        .map(|t| t.id)
        .collect())
}

/// The decisions and requests the extraction stored for a meeting, if it is done.
pub(crate) async fn read_lists(pool: &SqlitePool, meeting_id: &str) -> Result<Option<(Vec<String>, Vec<String>)>, String> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT status, decisions_json, requests_json FROM meeting_extracts WHERE meeting_id = ?",
    )
    .bind(meeting_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row
        .filter(|(status, _, _)| status == "done")
        .map(|(_, decisions, requests)| (parse_list(&decisions), parse_list(&requests))))
}

/// A stored list is written by us; a broken one reads as empty.
fn parse_list(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// True when the error is a UNIQUE key violation: the brief or digest already exists.
pub(crate) fn is_duplicate(e: &sqlx::Error) -> bool {
    e.as_database_error().is_some_and(|d| d.is_unique_violation())
}

/// Builds and stores the brief, pushes the feed item, shows the toast. Ok(None) = nothing made: no previous meeting,
/// its lists not ready, the occurrence no longer due, the previous meeting deleted meanwhile, or the brief already exists.
/// `now` is read again just before the insert: waiting for the extraction can take minutes.
pub(crate) async fn make_brief<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    occ: &Occurrence,
    template: &TemplateEntry,
    now: &(dyn Fn() -> DateTime<Utc> + Sync),
) -> Result<Option<String>, String> {
    let past = matching_meetings(pool, &template.id, &occ.title).await?;
    let Some(previous) = past.first() else {
        return Ok(None);
    };
    // Its extraction is still running or queued: wait for the next tick rather than make the brief without its lists.
    let Some((decisions, requests)) = digest::ready_lists(pool, &previous.id, settings::current().extract_enabled).await? else {
        return Ok(None);
    };
    if !due(occ, now()) {
        return Ok(None);
    }
    let ids: Vec<String> = past.iter().map(|p| p.id.clone()).collect();
    let stored = Stored {
        content: BriefContent {
            event_title: occ.title.clone(),
            event_start: start_key(occ.start),
            template_name: template.template.name.clone(),
            previous_meeting_id: previous.id.clone(),
            previous_title: previous.title.clone(),
            previous_date: previous.created_at.clone(),
            requests,
            decisions,
            open_tasks: Vec::new(),
        },
        open_task_ids: open_task_ids(pool, &ids).await?,
    };
    let (decisions, requests, open) = (stored.content.decisions.len(), stored.content.requests.len(), stored.open_task_ids.len());
    let line = brief_line(occ);
    let Some(id) = store(pool, occ, template, &previous.id, &stored, &line).await? else {
        return Ok(None);
    };
    // Only after the commit: the UI and the toast never show a brief that was not saved.
    feed::announce(app);
    if let Some(manager) = app.try_state::<NotificationManagerState<Wry>>() {
        if let Err(e) = show_brief_toast(&manager, line).await {
            warn!("knowledge: brief toast not shown: {e}");
        }
    }
    info!("knowledge: brief made, {decisions} decisions, {requests} requests, {open} open tasks");
    Ok(Some(id))
}

/// Writes the brief and its feed item in one transaction. None when the brief already exists or the previous meeting
/// was deleted while the lists were being made (the check is in the INSERT, so a delete cannot slip in between).
async fn store(
    pool: &SqlitePool,
    occ: &Occurrence,
    template: &TemplateEntry,
    previous_id: &str,
    stored: &Stored,
    line: &str,
) -> Result<Option<String>, String> {
    let id = Uuid::new_v4().to_string();
    let content_json = serde_json::to_string(stored).map_err(|e| e.to_string())?;
    // The brief and its feed item commit together. Dropping the transaction on an error rolls both back.
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let inserted = sqlx::query(
        "INSERT INTO briefs (id, event_uid, event_start, title, template_id, previous_meeting_id, content_json, created_at) \
         SELECT ?, ?, ?, ?, ?, ?, ?, ? WHERE EXISTS (SELECT 1 FROM meetings WHERE id = ?)",
    )
    .bind(&id)
    .bind(&occ.uid)
    .bind(start_key(occ.start))
    .bind(&occ.title)
    .bind(&template.id)
    .bind(previous_id)
    .bind(content_json)
    .bind(stamp())
    .bind(previous_id)
    .execute(&mut *tx)
    .await;
    match inserted {
        Ok(done) if done.rows_affected() == 0 => return Ok(None),
        Ok(_) => {}
        // Another run made this brief first; nothing more to do.
        Err(e) if is_duplicate(&e) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    feed::insert_in(&mut *tx, "brief", line, &id).await.map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(Some(id))
}

/// The brief with its open tasks looked up now. A task that is deleted, done or dismissed is left out.
pub(crate) async fn get(pool: &SqlitePool, id: &str) -> Result<Option<BriefContent>, String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT content_json FROM briefs WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    let Some((content_json,)) = row else {
        return Ok(None);
    };
    let stored: Stored = serde_json::from_str(&content_json).map_err(|e| e.to_string())?;
    let open = tasks::list(pool, "open").await.map_err(|e| e.to_string())?;
    let open_tasks = stored
        .open_task_ids
        .iter()
        .filter_map(|task_id| open.iter().find(|t| &t.id == task_id).cloned())
        .collect();
    Ok(Some(BriefContent { open_tasks, ..stored.content }))
}

#[tauri::command]
pub async fn briefs_list(state: State<'_, AppState>) -> Result<Vec<BriefSummary>, String> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id, title, created_at FROM briefs ORDER BY created_at DESC, rowid DESC")
            .fetch_all(state.db_manager.pool())
            .await
            .map_err(|e| e.to_string())?;
    let mut list: Vec<BriefSummary> = rows
        .into_iter()
        .map(|(id, title, created_at)| BriefSummary { id, kind: "brief".to_string(), title, created_at })
        .collect();
    list.extend(digest::summaries(state.db_manager.pool()).await?);
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(list)
}

#[tauri::command]
pub async fn brief_get(state: State<'_, AppState>, id: String) -> Result<Option<BriefContent>, String> {
    get(state.db_manager.pool(), &id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use crate::knowledge::{extract, tasks};
    use chrono::{Duration, Local, TimeZone};

    fn template(id: &str, name: &str, keywords: &[&str]) -> TemplateEntry {
        let mut t: crate::summary::templates::Template = serde_json::from_str(&format!(
            r#"{{"name":"{name}","sections":[{{"title":"A","instruction":"B"}}]}}"#
        ))
        .unwrap();
        t.title_keywords = keywords.iter().map(|k| k.to_string()).collect();
        TemplateEntry { id: id.into(), template: t }
    }

    fn occurrence(uid: &str, title: &str, start: DateTime<Utc>) -> Occurrence {
        Occurrence { uid: uid.into(), title: title.into(), start, end: start + Duration::minutes(30) }
    }

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 26, h, m, 0).unwrap()
    }

    async fn meeting(pool: &SqlitePool, id: &str, title: &str, created_at: &str, template_id: Option<&str>) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(title)
            .bind(created_at)
            .bind(created_at)
            .execute(pool)
            .await
            .unwrap();
        if let Some(template_id) = template_id {
            let result = serde_json::json!({ "markdown": "# Notes", "template_id": template_id }).to_string();
            sqlx::query(
                "INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) VALUES (?, 'completed', ?, ?, ?)",
            )
            .bind(id)
            .bind(created_at)
            .bind(created_at)
            .bind(result)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    async fn extracted(pool: &SqlitePool, meeting_id: &str, decisions: &str, requests: &str) {
        sqlx::query(
            "INSERT INTO meeting_extracts (meeting_id, decisions_json, requests_json, status, extracted_at) VALUES (?, ?, ?, 'done', 't')",
        )
        .bind(meeting_id)
        .bind(decisions)
        .bind(requests)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn task(pool: &SqlitePool, id: &str, meeting_id: &str, text: &str) {
        sqlx::query(
            "INSERT INTO tasks (id, meeting_id, text, owner, status, source, edited, created_at) VALUES (?, ?, ?, 'me', 'open', 'auto', 0, 't')",
        )
        .bind(id)
        .bind(meeting_id)
        .bind(text)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn feed_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM feed_items WHERE kind = 'brief'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn brief_rows(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM briefs").fetch_one(pool).await.unwrap()
    }

    #[test]
    fn event_type_first_match() {
        let templates = vec![
            template("one_on_one", "1:1", &["1:1"]),
            template("lesson", "Lesson", &["deutsch"]),
            template("second_one", "Second", &["1:1"]),
        ];
        assert_eq!(event_type("Deutsch 1:1 with Anna", &templates).unwrap().id, "one_on_one");
        assert_eq!(event_type("Урок DEUTSCH", &templates).unwrap().id, "lesson");
        assert!(event_type("Zoom Meeting", &templates).is_none());
    }

    #[test]
    fn due_window() {
        let occ = occurrence("u", "1:1", at(11, 0));
        assert!(!due(&occ, at(9, 59)), "61 min before is too early");
        assert!(due(&occ, at(10, 0)), "60 min before is due");
        assert!(due(&occ, at(10, 59)), "1 min before is due");
        assert!(!due(&occ, at(11, 0)), "at the start is too late");
    }

    #[test]
    fn same_title_folds_case_space() {
        assert!(same_title("1:1  Ivan\tPetrov", "1:1 ivan petrov"));
        assert!(same_title("ДЕУТШ урок", "деутш   Урок "));
        assert!(!same_title("1:1 Ivan", "1:1 Petr"));
    }

    #[tokio::test]
    async fn brief_uses_same_type_and_title() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        meeting(&pool, "ivan-brief_uses_same_type_and_title", "1:1 Ivan", "2026-10-19T10:00:00Z", Some("one_on_one")).await;
        meeting(&pool, "petr", "1:1 Petr", "2026-10-22T10:00:00Z", Some("one_on_one")).await;
        meeting(&pool, "ivan-other-type", "1:1 Ivan", "2026-10-23T10:00:00Z", Some("standard")).await;
        extracted(&pool, "ivan-brief_uses_same_type_and_title", r#"["Ship the plan"]"#, r#"["Send the deck"]"#).await;
        task(&pool, "t-ivan", "ivan-brief_uses_same_type_and_title", "Book the room").await;
        task(&pool, "t-petr", "petr", "Not Ivan's").await;

        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let made = make_brief(app.handle(), &pool, &occurrence("u1", "1:1 ivan", at(11, 0)), &one_on_one, &|| at(10, 30))
            .await
            .unwrap()
            .expect("a brief");
        let stored = get(&pool, &made).await.unwrap().unwrap();
        assert_eq!(stored.previous_meeting_id, "ivan-brief_uses_same_type_and_title");
        assert_eq!(stored.previous_title, "1:1 Ivan");
        assert_eq!(stored.decisions, vec!["Ship the plan".to_string()]);
        assert_eq!(stored.requests, vec!["Send the deck".to_string()]);
        let texts: Vec<&str> = stored.open_tasks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["Book the room"]);
    }

    #[tokio::test]
    async fn plain_text_summary_does_not_stop_the_brief() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        // A summary saved as plain text, not the JSON result, must not break the lookup.
        meeting(&pool, "plain", "1:1 Ivan", "2026-10-18T10:00:00Z", None).await;
        sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) VALUES ('plain', 'completed', 't', 't', '# Old notes')")
            .execute(&pool)
            .await
            .unwrap();
        meeting(&pool, "ivan-plain_text_summary_does_not_stop_the_brief", "1:1 Ivan", "2026-10-19T10:00:00Z", Some("one_on_one")).await;

        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let made = make_brief(app.handle(), &pool, &occurrence("u1", "1:1 Ivan", at(11, 0)), &one_on_one, &|| at(10, 30))
            .await
            .unwrap()
            .expect("a brief");
        assert_eq!(get(&pool, &made).await.unwrap().unwrap().previous_meeting_id, "ivan-plain_text_summary_does_not_stop_the_brief");
    }

    #[tokio::test]
    async fn brief_waits_while_extraction_runs() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        meeting(&pool, "ivan-in-flight", "1:1 Ivan", "2026-10-19T10:00:00Z", Some("one_on_one")).await;
        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let occ = occurrence("u1", "1:1 Ivan", at(11, 0));

        // The previous meeting's extraction is running: no brief goes out without its lists yet.
        let flight = extract::hold_in_flight("ivan-in-flight");
        assert_eq!(make_brief(app.handle(), &pool, &occ, &one_on_one, &|| at(10, 30)).await.unwrap(), None);
        assert_eq!(brief_rows(&pool).await, 0);
        assert_eq!(feed_count(&pool).await, 0);

        // Once it finishes, the next tick makes the brief.
        drop(flight);
        extracted(&pool, "ivan-in-flight", r#"["Ship the plan"]"#, "[]").await;
        assert!(make_brief(app.handle(), &pool, &occ, &one_on_one, &|| at(10, 30)).await.unwrap().is_some());
        assert_eq!(brief_rows(&pool).await, 1);
        assert_eq!(feed_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn failed_feed_insert_leaves_no_brief() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        meeting(&pool, "ivan-failed_feed_insert_leaves_no_brief", "1:1 Ivan", "2026-10-19T10:00:00Z", Some("one_on_one")).await;
        // The brief row and its feed item are written together: a failed feed insert rolls the brief back too.
        sqlx::query("CREATE TRIGGER feed_down BEFORE INSERT ON feed_items BEGIN SELECT RAISE(ABORT, 'feed is down'); END")
            .execute(&pool)
            .await
            .unwrap();

        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let result = make_brief(app.handle(), &pool, &occurrence("u1", "1:1 Ivan", at(11, 0)), &one_on_one, &|| at(10, 30)).await;
        assert!(result.is_err(), "{result:?}");
        assert_eq!(brief_rows(&pool).await, 0);
    }

    #[tokio::test]
    async fn no_previous_no_brief() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        meeting(&pool, "other", "1:1 Petr", "2026-10-19T10:00:00Z", Some("one_on_one")).await;

        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let made = make_brief(app.handle(), &pool, &occurrence("u1", "1:1 Ivan", at(11, 0)), &one_on_one, &|| at(10, 30))
            .await
            .unwrap();
        assert_eq!(made, None);
        assert_eq!(feed_count(&pool).await, 0);
        let briefs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM briefs").fetch_one(&pool).await.unwrap();
        assert_eq!(briefs, 0);
    }

    #[tokio::test]
    async fn brief_made_pushes_feed_item() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        meeting(&pool, "ivan-brief_made_pushes_feed_item", "1:1 Ivan", "2026-10-19T10:00:00Z", Some("one_on_one")).await;

        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let made = make_brief(app.handle(), &pool, &occurrence("u1", "1:1 Ivan", at(11, 0)), &one_on_one, &|| at(10, 30))
            .await
            .unwrap()
            .expect("a brief");
        let items: Vec<(String, String, String)> = sqlx::query_as("SELECT kind, target_id, title FROM feed_items")
            .fetch_all(&pool)
            .await
            .unwrap();
        let hhmm = at(11, 0).with_timezone(&Local).format("%H:%M");
        assert_eq!(items, vec![("brief".to_string(), made, format!("Brief: 1:1 Ivan at {hhmm}"))]);
    }

    #[tokio::test]
    async fn brief_get_reads_tasks_live() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        meeting(&pool, "ivan-brief_get_reads_tasks_live", "1:1 Ivan", "2026-10-19T10:00:00Z", Some("one_on_one")).await;
        task(&pool, "t1", "ivan-brief_get_reads_tasks_live", "First").await;
        task(&pool, "t2", "ivan-brief_get_reads_tasks_live", "Second").await;

        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let made = make_brief(app.handle(), &pool, &occurrence("u1", "1:1 Ivan", at(11, 0)), &one_on_one, &|| at(10, 30))
            .await
            .unwrap()
            .expect("a brief");
        // Deleted tasks vanish; a task ticked done leaves the open list too.
        tasks::set_status(&pool, "t1", "done").await.unwrap();
        let stored = get(&pool, &made).await.unwrap().unwrap();
        let texts: Vec<&str> = stored.open_tasks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["Second"]);
        assert_eq!(get(&pool, "missing").await.unwrap(), None);
    }

    #[tokio::test]
    async fn deleted_previous_meeting_leaves_no_brief() {
        let pool = test_pool().await;
        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let occ = occurrence("u1", "1:1 Ivan", at(11, 0));
        meeting(&pool, "ivan-deleted_previous", "1:1 Ivan", "2026-10-19T10:00:00Z", Some("one_on_one")).await;
        let stored = Stored {
            content: BriefContent {
                event_title: occ.title.clone(),
                event_start: start_key(occ.start),
                template_name: "1:1".into(),
                previous_meeting_id: "ivan-deleted_previous".into(),
                previous_title: "1:1 Ivan".into(),
                previous_date: "d".into(),
                requests: Vec::new(),
                decisions: Vec::new(),
                open_tasks: Vec::new(),
            },
            open_task_ids: Vec::new(),
        };

        // The meeting is deleted after the lookup, while the extraction was being waited for.
        sqlx::query("DELETE FROM meetings WHERE id = 'ivan-deleted_previous'").execute(&pool).await.unwrap();
        let made = store(&pool, &occ, &one_on_one, "ivan-deleted_previous", &stored, "Brief: x").await.unwrap();

        assert_eq!(made, None);
        assert_eq!(brief_rows(&pool).await, 0);
        assert_eq!(feed_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn no_longer_due_after_the_wait_makes_no_brief() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        meeting(&pool, "ivan-no_longer_due", "1:1 Ivan", "2026-10-19T10:00:00Z", Some("one_on_one")).await;
        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let occ = occurrence("u1", "1:1 Ivan", at(11, 0));

        // The meeting started while the previous one was being extracted.
        let made = make_brief(app.handle(), &pool, &occ, &one_on_one, &|| at(11, 1)).await.unwrap();

        assert_eq!(made, None);
        assert_eq!(brief_rows(&pool).await, 0);
        assert_eq!(feed_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn brief_waits_while_extraction_is_queued() {
        let pool = test_pool().await;
        let app = tauri::test::mock_app();
        meeting(&pool, "ivan-queued", "1:1 Ivan", "2026-10-19T10:00:00Z", Some("one_on_one")).await;
        // Queued and not running (extraction off, or its worker has not got to it): the brief waits for the lists.
        sqlx::query("INSERT INTO meeting_extracts (meeting_id, decisions_json, requests_json, status, extracted_at) VALUES ('ivan-queued', '[]', '[]', 'pending', 't')")
            .execute(&pool)
            .await
            .unwrap();
        let one_on_one = template("one_on_one", "1:1", &["1:1"]);
        let occ = occurrence("u1", "1:1 Ivan", at(11, 0));

        // With extraction off nothing runs it, so the lists are empty rather than blocking the brief.
        assert_eq!(
            digest::ready_lists(&pool, "ivan-queued", false).await.unwrap(),
            Some((Vec::new(), Vec::new()))
        );
        // With it on, hold the slot so the run inside ready_lists is a no-op and the row stays pending.
        let _flight = extract::hold_in_flight("ivan-queued");
        assert_eq!(make_brief(app.handle(), &pool, &occ, &one_on_one, &|| at(10, 30)).await.unwrap(), None);
        assert_eq!(brief_rows(&pool).await, 0);
    }
}
