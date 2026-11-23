//! Sends kept tasks to Notion, one at a time. The no-duplicate guarantee: a row is committed as `sending`
//! BEFORE its Notion call, and an unsure outcome (or a crash) leaves it marked "may already be in Notion";
//! such a row is never resent unless the user names it.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use sqlx::SqlitePool;

use super::client::{CallError, NotionClient};
use super::fill;
use super::mapping::{page_body, SendTask};
use super::{list_destinations, set_dest_status, Destination, SendReport, BLOCKING_STATES};
use crate::database::repositories::meeting::meeting_date_sql;
use crate::knowledge::extract::{self, Reply as ModelReply};
use crate::knowledge::tasks::stamp;

pub(crate) const INTERRUPTED: &str = "Conclave closed while sending this task.";
const NOT_SAVED: &str = "Couldn't save the send result. Restart Conclave; unsent tasks will show as may already be in Notion.";

static SENDING: AtomicBool = AtomicBool::new(false);

struct SendGuard;
impl Drop for SendGuard {
    fn drop(&mut self) {
        SENDING.store(false, Ordering::SeqCst);
    }
}

type TaskRow = (String, String, Option<f64>, Option<String>, Option<String>, Option<String>, Option<String>);

/// Writes a pre-send refusal on a row that is still `keep`. A failed write only loses the message.
async fn refuse(pool: &SqlitePool, id: &str, msg: &str) {
    if let Err(e) = sqlx::query("UPDATE tasks SET send_error = ? WHERE id = ? AND review = 'keep'").bind(msg).bind(id).execute(pool).await {
        log::warn!("notion: could not note why task {id} was not sent: {e}");
    }
}

async fn mark(pool: &SqlitePool, dests: &mut HashMap<String, Destination>, id: &str, state: &str, msg: Option<&str>) {
    if let Err(e) = set_dest_status(pool, id, state, msg).await {
        log::warn!("notion: could not save the status of destination {id}: {e}");
    }
    if let Some(d) = dests.get_mut(id) {
        d.last_status = Some(state.into());
        d.last_message = msg.map(Into::into);
    }
}

/// `task_ids` None: every kept task with a destination, except unsure ones. Some: exactly those, re-checked.
/// Unsure rows (`send_ambiguous`) are skipped unless `resend_ambiguous`, even when listed: only the UI's "Send again" sets it.
/// After a final 429 or an offline error the run stops and the rest keep that error (no burst of calls).
/// The AI pre-pass for the other columns runs before any row is claimed.
pub(crate) async fn send(pool: &SqlitePool, client: &NotionClient, task_ids: Option<Vec<String>>, resend_ambiguous: bool) -> Result<SendReport, String> {
    send_with(pool, client, task_ids, resend_ambiguous, |s, u| extract::call_model(pool.clone(), s, u)).await
}

pub(crate) async fn send_with<F, Fut>(
    pool: &SqlitePool,
    client: &NotionClient,
    task_ids: Option<Vec<String>>,
    resend_ambiguous: bool,
    call: F,
) -> Result<SendReport, String>
where
    F: Fn(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<ModelReply, String>>,
{
    if SENDING.swap(true, Ordering::SeqCst) {
        return Err("A send is already running".into());
    }
    let _guard = SendGuard;
    let mut dests: HashMap<String, Destination> = list_destinations(pool)
        .await
        .map_err(|e| format!("Could not read the Notion databases: {e}"))?
        .into_iter()
        .map(|d| (d.id.clone(), d))
        .collect();
    let ids: Vec<String> = match task_ids {
        Some(ids) => ids,
        None => sqlx::query_scalar(
            "SELECT id FROM tasks WHERE review = 'keep' AND send_ambiguous = 0 AND dest_id IS NOT NULL ORDER BY created_at, rowid",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Could not read the tasks: {e}"))?,
    };
    let sql = format!(
        "SELECT t.id, t.text, t.at, t.due_date, t.dest_id, COALESCE(m.title, t.meeting_title), COALESCE({}, t.meeting_date) \
         FROM tasks t LEFT JOIN meetings m ON m.id = t.meeting_id WHERE t.id = ? AND t.review = 'keep'",
        meeting_date_sql("m")
    );
    let mut report = SendReport::default();
    let (ai, not_filled) = fill::ai_values(pool, client, &dests, &ids, resend_ambiguous, &call).await;
    report.ai_not_filled = not_filled;
    let mut stopped: Option<String> = None;
    for id in ids {
        if let Some(msg) = &stopped {
            let r = sqlx::query("UPDATE tasks SET send_error = ? WHERE id = ? AND review = 'keep' AND send_ambiguous = 0")
                .bind(msg)
                .bind(&id)
                .execute(pool)
                .await;
            if let Err(e) = &r {
                log::warn!("notion: could not note why task {id} was not sent: {e}");
            }
            report.failed += 1;
            continue;
        }
        let was_unsure: bool = sqlx::query_scalar::<_, i64>("SELECT send_ambiguous FROM tasks WHERE id = ? AND review = 'keep'")
            .bind(&id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("Could not read the tasks: {e}"))?
            == Some(1);
        if was_unsure && !resend_ambiguous {
            continue;
        }
        let row: Option<TaskRow> = sqlx::query_as(&sql)
            .bind(&id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("Could not read the tasks: {e}"))?;
        let Some((_, text, at, due_date, dest_id, meeting_title, meeting_date)) = row else {
            report.failed += 1;
            continue;
        };
        let Some(dest) = dest_id.as_ref().and_then(|d| dests.get(d)).cloned() else {
            refuse(pool, &id, "Choose a Notion database first.").await;
            report.failed += 1;
            continue;
        };
        if dest.last_status.as_deref().is_some_and(|s| BLOCKING_STATES.contains(&s)) {
            let msg = dest.last_message.clone().unwrap_or_else(|| format!("{} isn't connected. Check it in Settings → Notion.", dest.name));
            refuse(pool, &id, &msg).await;
            report.failed += 1;
            continue;
        }
        let task = SendTask { id: id.clone(), text, at, due_date, meeting_title, meeting_date, dest_id: dest.id.clone() };
        // The row's database may have changed during the pre-pass: values made for another database are dropped.
        let ai_map = ai.get(&id).filter(|(d, _)| *d == dest.id).map(|(_, m)| m);
        let body = page_body(dest.data_source_id.as_deref().unwrap_or_default(), &dest.mapping, &task, ai_map);
        // Autocommit: this must be durable before the request leaves.
        let claimed = sqlx::query("UPDATE tasks SET review = 'sending', send_error = NULL, send_ambiguous = 0 WHERE id = ? AND review = 'keep' AND (send_ambiguous = 0 OR ?)")
            .bind(&id)
            .bind(resend_ambiguous)
            .execute(pool)
            .await
            .map_err(|e| format!("Could not mark the task as sending: {e}"))?;
        if claimed.rows_affected() != 1 {
            continue; // the UI changed it meanwhile
        }
        let saved = match client.create_page(&body).await {
            Ok(p) => {
                report.sent += 1;
                let r = sqlx::query(
                    "UPDATE tasks SET review = 'sent', notion_page_id = ?, notion_url = ?, sent_at = ?, send_error = NULL WHERE id = ? AND review = 'sending'",
                )
                .bind(&p.id)
                .bind(&p.url)
                .bind(stamp())
                .bind(&id)
                .execute(pool)
                .await;
                if r.as_ref().is_ok_and(|r| r.rows_affected() == 0) {
                    log::warn!("notion: task {id} was sent but its row changed meanwhile");
                }
                mark(pool, &mut dests, &dest.id, "connected", None).await;
                r
            }
            Err(e) if e.is_ambiguous() => {
                report.ambiguous += 1;
                let msg = format!("{} The task may already be in Notion.", e.message(&dest.name));
                // An outage must not turn the whole run into "may already be in Notion".
                stopped = Some("Not sent: Notion didn't answer.".into());
                sqlx::query("UPDATE tasks SET review = 'keep', send_ambiguous = 1, send_error = ? WHERE id = ? AND review = 'sending'")
                    .bind(msg)
                    .bind(&id)
                    .execute(pool)
                    .await
            }
            Err(e) => {
                report.failed += 1;
                let msg = e.message(&dest.name);
                // A failed "Send again" must stay unsure: the first attempt may still have created the page.
                let r = sqlx::query("UPDATE tasks SET review = 'keep', send_error = ?, send_ambiguous = ? WHERE id = ? AND review = 'sending'")
                    .bind(&msg)
                    .bind(was_unsure)
                    .bind(&id)
                    .execute(pool)
                    .await;
                if e.state() == "token_rejected" {
                    let all: Vec<String> = dests.keys().cloned().collect();
                    for d in all {
                        mark(pool, &mut dests, &d, "token_rejected", Some(&msg)).await;
                    }
                } else if BLOCKING_STATES.contains(&e.state()) {
                    mark(pool, &mut dests, &dest.id, e.state(), Some(&msg)).await;
                }
                if matches!(e, CallError::Offline | CallError::Http { status: 429, .. }) {
                    stopped = Some(msg.clone());
                }
                r
            }
        };
        if let Err(e) = saved {
            log::error!("notion: outcome of task {id} not saved: {e}");
            return Err(NOT_SAVED.into());
        }
    }
    log::info!("notion: send finished: {} sent, {} failed, {} unsure", report.sent, report.failed, report.ambiguous);
    Ok(report)
}

/// Rows still `sending` at startup were cut off mid-call: Notion may or may not have the page.
pub(crate) async fn recover_sending(pool: &SqlitePool) -> anyhow::Result<u64> {
    let r = sqlx::query("UPDATE tasks SET review = 'keep', send_ambiguous = 1, send_error = ? WHERE review = 'sending'")
        .bind(INTERRUPTED)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

/// Startup sweep; skipped on first launch, when no database is open yet.
pub async fn recover_at_startup<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let Some(state) = app.try_state::<crate::state::AppState>() else { return };
    match recover_sending(state.db_manager.pool()).await {
        Ok(0) => {}
        Ok(n) => log::info!("notion: {n} tasks may already be in Notion (Conclave closed while sending)"),
        Err(e) => log::warn!("notion: could not recover interrupted sends: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use crate::notion::client::fake::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    static SEND_TEST_LOCK: Mutex<()> = Mutex::const_new(());
    const MAP: &str = r#"{"titleProp":"Name","dueProp":null,"meetingProp":null,"fixed":[],"aiFill":false}"#;
    const MAP_AI: &str = r#"{"titleProp":"Name","dueProp":null,"meetingProp":null,"fixed":[],"emoji":"👷","aiFill":true}"#;
    const DS: &str = "01234567-89ab-cdef-0123-456789abcdef";

    async fn setup() -> SqlitePool {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'Weekly sync', 't', 't')").execute(&pool).await.unwrap();
        for (id, name) in [("d1", "Job tasks"), ("d2", "Personal tasks")] {
            sqlx::query(
                "INSERT INTO notion_destinations (id, name, colour, position, database_id, data_source_id, mapping_json, created_at) \
                 VALUES (?, ?, 1, 0, 'db', ?, ?, 't')",
            )
            .bind(id).bind(name).bind(DS).bind(MAP)
            .execute(&pool).await.unwrap();
        }
        pool
    }

    async fn task_on(pool: &SqlitePool, id: &str, review: &str, ambiguous: i64, dest: Option<&str>) {
        sqlx::query(
            "INSERT INTO tasks (id, meeting_id, text, owner, status, source, edited, created_at, review, send_ambiguous, dest_id) \
             VALUES (?, 'm1', ?, 'me', 'open', 'auto', 0, ?, ?, ?, ?)",
        )
        .bind(id).bind(format!("text {id}")).bind(format!("2026-11-20T10:00:0{}Z", &id[1..])).bind(review).bind(ambiguous).bind(dest)
        .execute(pool).await.unwrap();
    }
    async fn task(pool: &SqlitePool, id: &str, review: &str, ambiguous: i64) {
        task_on(pool, id, review, ambiguous, Some("d1")).await
    }

    async fn row(pool: &SqlitePool, id: &str) -> (String, Option<String>, Option<String>, bool) {
        let (r, u, e, a): (String, Option<String>, Option<String>, i64) =
            sqlx::query_as("SELECT review, notion_url, send_error, send_ambiguous FROM tasks WHERE id = ?").bind(id).fetch_one(pool).await.unwrap();
        (r, u, e, a == 1)
    }
    async fn status(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar("SELECT last_status FROM notion_destinations WHERE id = ?").bind(id).fetch_one(pool).await.unwrap()
    }
    fn ok(id: &str) -> Reply {
        reply(200, "", &format!(r#"{{"id":"{id}","url":"https://www.notion.so/{id}"}}"#))
    }
    fn client(f: &Fake) -> NotionClient {
        NotionClient::with_base(&f.base, "ntn_SECRET123", Duration::from_millis(500))
    }
    fn reqs(f: &Fake) -> usize {
        f.requests.lock().unwrap().len()
    }

    #[tokio::test]
    async fn sends_kept_tasks_in_order_and_marks_them_sent() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 0).await;
        task(&pool, "t2", "keep", 0).await;
        task(&pool, "t3", "open", 0).await;
        let f = serve(vec![ok("p1"), ok("p2")]).await;
        let r = send(&pool, &client(&f), None, false).await.unwrap();
        assert_eq!((r.sent, r.failed, r.ambiguous), (2, 0, 0));
        assert_eq!(row(&pool, "t1").await, ("sent".into(), Some("https://www.notion.so/p1".into()), None, false));
        assert_eq!(row(&pool, "t2").await.1.as_deref(), Some("https://www.notion.so/p2"));
        assert_eq!(row(&pool, "t3").await.0, "open");
        assert_eq!(reqs(&f), 2);
        assert!(f.requests.lock().unwrap()[0].body.contains(r#""data_source_id":"01234567-89ab-cdef-0123-456789abcdef""#));
        assert_eq!(status(&pool, "d1").await.as_deref(), Some("connected"));
    }

    #[tokio::test]
    async fn row_is_committed_as_sending_before_the_call_and_timeout_is_ambiguous() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![Reply::Hang]).await;
        let c = Arc::new(client(&f));
        let h = { let (p, c) = (pool.clone(), c.clone()); tokio::spawn(async move { send(&p, &c, None, false).await }) };
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(row(&pool, "t1").await.0, "sending");
        let r = h.await.unwrap().unwrap();
        assert_eq!((r.sent, r.failed, r.ambiguous), (0, 0, 1));
        let (rev, _, err, amb) = row(&pool, "t1").await;
        assert_eq!((rev.as_str(), amb), ("keep", true));
        assert!(err.unwrap().contains("may already be in Notion"));
    }

    #[tokio::test]
    async fn definite_failure_returns_to_keep_with_error_and_no_token() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![reply(400, "", r#"{"code":"validation_error","message":"Due is not a property that exists. ntn_SECRET123"}"#)]).await;
        let r = send(&pool, &client(&f), None, false).await.unwrap();
        assert_eq!((r.sent, r.failed, r.ambiguous), (0, 1, 0));
        let (rev, _, err, amb) = row(&pool, "t1").await;
        assert_eq!((rev.as_str(), amb), ("keep", false));
        let err = err.unwrap();
        assert!(err.starts_with("Job tasks: ") && err.contains("Due") && !err.contains("ntn_SECRET123"), "{err}");
    }

    #[tokio::test]
    async fn server_error_is_ambiguous() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![reply(503, "", "{}")]).await;
        let r = send(&pool, &client(&f), None, false).await.unwrap();
        assert_eq!(r.ambiguous, 1);
        assert!(row(&pool, "t1").await.3);
    }

    #[tokio::test]
    async fn ambiguous_rows_are_sent_only_when_named_and_resent() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 1).await;
        let f = serve(vec![ok("p1")]).await;
        assert_eq!(send(&pool, &client(&f), None, false).await.unwrap(), SendReport::default());
        assert_eq!(reqs(&f), 0);
        let r = send(&pool, &client(&f), Some(vec!["t1".into()]), true).await.unwrap();
        assert_eq!(r.sent, 1);
        assert!(!row(&pool, "t1").await.3);
    }

    #[tokio::test]
    async fn named_ids_are_rechecked() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        sqlx::query("UPDATE notion_destinations SET last_status = 'not_shared' WHERE id = 'd2'").execute(&pool).await.unwrap();
        task(&pool, "t1", "open", 0).await;
        task_on(&pool, "t2", "keep", 0, None).await;
        task_on(&pool, "t3", "keep", 0, Some("d2")).await;
        let f = serve(vec![]).await;
        let ids = ["t1", "t2", "t3", "nope"].map(String::from).to_vec();
        let r = send(&pool, &client(&f), Some(ids), false).await.unwrap();
        assert_eq!((r.sent, r.failed, r.ambiguous), (0, 4, 0));
        assert_eq!(reqs(&f), 0);
        assert_eq!(row(&pool, "t2").await.2.as_deref(), Some("Choose a Notion database first."));
        assert!(row(&pool, "t3").await.2.unwrap().starts_with("Personal tasks"));
    }

    #[tokio::test]
    async fn unsure_rows_are_skipped_unless_resend_is_set() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 1).await;
        let f = serve(vec![ok("p1")]).await;
        let ids = || Some(vec!["t1".to_string()]);
        let r = send(&pool, &client(&f), ids(), false).await.unwrap();
        assert_eq!((r.sent, r.failed, reqs(&f)), (0, 0, 0));
        assert_eq!(row(&pool, "t1").await.0, "keep");
        let r = send(&pool, &client(&f), ids(), true).await.unwrap();
        assert_eq!((r.sent, reqs(&f)), (1, 1));
        assert_eq!(row(&pool, "t1").await.0, "sent");
    }

    #[tokio::test]
    async fn final_429_stops_the_run_and_marks_the_rest() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        for t in ["t1", "t2", "t3"] {
            task(&pool, t, "keep", 0).await;
        }
        let f = serve((0..4).map(|_| reply(429, "Retry-After: 0\r\n", r#"{"code":"rate_limited"}"#)).collect()).await;
        let r = send(&pool, &client(&f), None, false).await.unwrap();
        assert_eq!((r.sent, r.failed, reqs(&f)), (0, 3, 4));
        for t in ["t1", "t2", "t3"] {
            let (rev, _, err, amb) = row(&pool, t).await;
            assert_eq!((rev.as_str(), amb), ("keep", false));
            assert!(err.unwrap().contains("slow down"));
        }
    }

    #[tokio::test]
    async fn failed_send_again_stays_unsure() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 1).await;
        let f = serve(vec![reply(400, "", r#"{"code":"validation_error","message":"Due is not a property that exists."}"#)]).await;
        let r = send(&pool, &client(&f), Some(vec!["t1".into()]), true).await.unwrap();
        assert_eq!((r.sent, r.failed), (0, 1));
        let (rev, _, _, amb) = row(&pool, "t1").await;
        assert_eq!((rev.as_str(), amb), ("keep", true));
    }

    #[tokio::test]
    async fn an_unanswered_call_stops_the_run_without_more_unsure_rows() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        for t in ["t1", "t2", "t3"] {
            task(&pool, t, "keep", 0).await;
        }
        let f = serve(vec![reply(500, "", "{}"), ok("p2"), ok("p3")]).await;
        let r = send(&pool, &client(&f), None, false).await.unwrap();
        assert_eq!((r.sent, r.failed, r.ambiguous, reqs(&f)), (0, 2, 1, 1));
        assert_eq!(row(&pool, "t1").await.3, true);
        for t in ["t2", "t3"] {
            let (rev, _, err, amb) = row(&pool, t).await;
            assert_eq!((rev.as_str(), amb), ("keep", false));
            assert_eq!(err.as_deref(), Some("Not sent: Notion didn't answer."));
        }
    }

    #[tokio::test]
    async fn offline_stops_the_run() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 0).await;
        task(&pool, "t2", "keep", 0).await;
        // Windows retries a refused connect for ~2s; a shorter timeout turns "offline" into "maybe sent".
        let dead = NotionClient::with_base("http://127.0.0.1:1", "t", Duration::from_secs(5));
        let r = send(&pool, &dead, None, false).await.unwrap();
        assert_eq!((r.sent, r.failed), (0, 2));
        assert!(row(&pool, "t2").await.2.unwrap().contains("could not be reached"));
    }

    #[tokio::test]
    async fn renamed_column_blocks_the_destination() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        for t in ["t1", "t2", "t3"] {
            task(&pool, t, "keep", 0).await;
        }
        let f = serve(vec![reply(400, "", r#"{"code":"validation_error","message":"Due is not a property that exists."}"#)]).await;
        let r = send(&pool, &client(&f), None, false).await.unwrap();
        assert_eq!((r.sent, r.failed, r.ambiguous), (0, 3, 0));
        assert_eq!(reqs(&f), 1);
        assert_eq!(status(&pool, "d1").await.as_deref(), Some("mapping_broken"));
        for t in ["t2", "t3"] {
            let (rev, _, err, _) = row(&pool, t).await;
            assert_eq!(rev, "keep");
            assert!(err.unwrap().contains("Due"));
        }
    }

    #[tokio::test]
    async fn rejected_token_stops_the_run() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 0).await;
        task(&pool, "t2", "keep", 0).await;
        let f = serve(vec![reply(401, "", r#"{"code":"unauthorized","message":"bad"}"#)]).await;
        let r = send(&pool, &client(&f), None, false).await.unwrap();
        assert_eq!((r.sent, r.failed), (0, 2));
        assert_eq!(reqs(&f), 1);
        assert_eq!((status(&pool, "d1").await.as_deref(), status(&pool, "d2").await.as_deref()), (Some("token_rejected"), Some("token_rejected")));
    }

    #[tokio::test]
    async fn outcome_not_saved_stays_sending_then_recovers() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 0).await;
        sqlx::query("CREATE TRIGGER t BEFORE UPDATE OF review ON tasks WHEN NEW.review = 'sent' BEGIN SELECT RAISE(ABORT, 'disk'); END")
            .execute(&pool).await.unwrap();
        let f = serve(vec![ok("p1")]).await;
        assert_eq!(send(&pool, &client(&f), None, false).await.unwrap_err(), NOT_SAVED);
        assert_eq!(row(&pool, "t1").await.0, "sending");
        sqlx::query("DROP TRIGGER t").execute(&pool).await.unwrap();
        assert_eq!(recover_sending(&pool).await.unwrap(), 1);
        assert_eq!(row(&pool, "t1").await, ("keep".into(), None, Some(INTERRUPTED.into()), true));
    }

    #[tokio::test]
    async fn second_send_is_refused_while_one_runs() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![Reply::Hang, ok("p1")]).await;
        let c = Arc::new(client(&f));
        let h = { let (p, c) = (pool.clone(), c.clone()); tokio::spawn(async move { send(&p, &c, None, false).await }) };
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(send(&pool, &c, None, false).await.unwrap_err(), "A send is already running");
        h.await.unwrap().unwrap();
        assert!(send(&pool, &c, None, false).await.is_ok());
    }

    #[tokio::test]
    async fn startup_turns_sending_rows_into_may_already_be_in_notion() {
        use tauri::Manager;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("meeting_minutes.sqlite").to_string_lossy().to_string();
        let legacy = dir.path().join("meeting_minutes.db").to_string_lossy().to_string();
        let m = crate::database::manager::DatabaseManager::new(&db, &legacy).await.unwrap();
        task_on(m.pool(), "t1", "sending", 0, None).await;
        let app = tauri::test::mock_app();
        app.manage(crate::state::AppState { db_manager: m.clone() });
        recover_at_startup(app.handle()).await;
        assert_eq!(row(m.pool(), "t1").await, ("keep".into(), None, Some(INTERRUPTED.into()), true));
    }

    #[tokio::test]
    async fn startup_hook_skips_first_launch() {
        recover_at_startup(tauri::test::mock_app().handle()).await;
    }

    fn ds_reply() -> Reply {
        reply(
            200,
            "",
            &format!(
                r#"{{"id":"{DS}","properties":{{"Name":{{"id":"n","type":"title","title":{{}}}},"Thema":{{"id":"t","type":"rich_text","rich_text":{{}}}},"Status":{{"id":"s","type":"status","status":{{"options":[{{"name":"To do"}}]}}}}}}}}"#
            ),
        )
    }
    async fn ai_on(pool: &SqlitePool) {
        sqlx::query("UPDATE notion_destinations SET mapping_json = ? WHERE id = 'd1'").bind(MAP_AI).execute(pool).await.unwrap();
    }
    fn model(content: &'static str) -> ModelReply {
        ModelReply {
            provider: "fake".into(),
            model: "m".into(),
            completion: crate::summary::llm_client::LlmCompletion { content: content.into(), reasoning_stripped: false, usage: None, truncated: false },
        }
    }
    /// Counting fake model call.
    fn fake_ai(
        n: &Arc<std::sync::atomic::AtomicUsize>,
        content: Result<&'static str, &'static str>,
    ) -> impl Fn(String, String) -> std::future::Ready<Result<ModelReply, String>> {
        let n = n.clone();
        move |_, _| {
            n.fetch_add(1, Ordering::SeqCst);
            std::future::ready(content.map(model).map_err(String::from))
        }
    }
    fn calls(n: &Arc<std::sync::atomic::AtomicUsize>) -> usize {
        n.load(Ordering::SeqCst)
    }

    #[tokio::test]
    async fn ai_fills_other_columns_and_sets_the_icon() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        task(&pool, "t2", "keep", 0).await;
        let f = serve(vec![ds_reply(), ok("p1"), ok("p2")]).await;
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let call = fake_ai(&n, Ok(r#"{"1":{"Thema":"Dativ","Status":"To do"},"2":{"Thema":"Akkusativ"}}"#));
        let r = send_with(&pool, &client(&f), None, false, call).await.unwrap();
        assert_eq!((r.sent, r.failed, r.ambiguous), (2, 0, 0));
        assert!(r.ai_not_filled.is_empty());
        assert_eq!(calls(&n), 1);
        let rq = f.requests.lock().unwrap();
        assert!(rq[0].head.starts_with("GET") && rq[0].head.contains("/data_sources/"), "{}", rq[0].head);
        for want in [r#""Thema""#, r#""Dativ""#, r#"{"status":{"name":"To do"}}"#, r#""icon":{"emoji":"👷","type":"emoji"}"#] {
            assert!(rq[1].body.contains(want), "{want} in {}", rq[1].body);
        }
    }

    #[tokio::test]
    async fn ai_call_happens_before_any_row_is_sending() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        task(&pool, "t2", "keep", 0).await;
        let f = serve(vec![ds_reply(), ok("p1"), ok("p2")]).await;
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let call = {
            let (pool, seen) = (pool.clone(), seen.clone());
            move |_: String, _: String| {
                let (pool, seen) = (pool.clone(), seen.clone());
                async move {
                    for id in ["t1", "t2"] {
                        seen.lock().unwrap().push(row(&pool, id).await.0);
                    }
                    Ok(model(r#"{"1":{"Thema":"A"},"2":{"Thema":"B"}}"#))
                }
            }
        };
        let r = send_with(&pool, &client(&f), None, false, call).await.unwrap();
        assert_eq!(*seen.lock().unwrap(), vec!["keep", "keep"]);
        assert_eq!(r.sent, 2);
    }

    #[tokio::test]
    async fn ai_failure_still_sends_and_reports_the_database() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![ds_reply(), ok("p1")]).await;
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let r = send_with(&pool, &client(&f), None, false, fake_ai(&n, Err("boom"))).await.unwrap();
        assert_eq!((r.sent, r.ai_not_filled), (1, vec!["Job tasks".to_string()]));
        assert!(!f.requests.lock().unwrap()[1].body.contains("Thema"));
        assert_eq!(row(&pool, "t1").await.0, "sent");
    }

    #[tokio::test]
    async fn schema_read_failure_still_sends() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![reply(503, "", "{}"), ok("p1")]).await;
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let r = send_with(&pool, &client(&f), None, false, fake_ai(&n, Ok("{}"))).await.unwrap();
        assert_eq!(calls(&n), 0);
        assert_eq!((r.sent, r.ambiguous, r.ai_not_filled), (1, 0, vec!["Job tasks".to_string()]));
    }

    #[tokio::test]
    async fn bad_ai_values_are_dropped_and_reported() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![ds_reply(), ok("p1")]).await;
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // "Ghost" is no column of the database; "Later" is not a Status option; 5 is not text.
        let call = fake_ai(&n, Ok(r#"{"1":{"Status":"Later","Thema":5,"Ghost":"x"}}"#));
        let r = send_with(&pool, &client(&f), None, false, call).await.unwrap();
        let body = f.requests.lock().unwrap()[1].body.clone();
        assert!(!body.contains("Later") && !body.contains("Thema") && !body.contains("Ghost") && !body.contains("Status"), "{body}");
        assert_eq!((r.sent, r.ai_not_filled), (1, vec!["Job tasks".to_string()]));
    }

    #[tokio::test]
    async fn settings_problem_is_named_in_the_report() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![ds_reply(), ok("p1")]).await;
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let call = fake_ai(&n, Err("Built-in AI is not supported for this feature"));
        let r = send_with(&pool, &client(&f), None, false, call).await.unwrap();
        assert_eq!(r.sent, 1);
        assert_eq!(r.ai_not_filled, vec!["Job tasks: Built-in AI can't fill columns, choose a cloud model in Models & keys".to_string()]);
    }

    #[tokio::test]
    async fn one_deadline_covers_the_whole_pre_pass() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        for (i, m) in ["m2", "m3"].iter().enumerate() {
            sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'Other', 't', 't')").bind(m).execute(&pool).await.unwrap();
            task(&pool, &format!("t{}", i + 2), "keep", 0).await;
            sqlx::query("UPDATE tasks SET meeting_id = ? WHERE id = ?").bind(m).bind(format!("t{}", i + 2)).execute(&pool).await.unwrap();
        }
        task(&pool, "t1", "keep", 0).await;
        let dests: HashMap<String, Destination> = list_destinations(&pool).await.unwrap().into_iter().map(|d| (d.id.clone(), d)).collect();
        let ids: Vec<String> = ["t1", "t2", "t3"].iter().map(|s| s.to_string()).collect();
        let f = serve(vec![ds_reply()]).await;
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hang = {
            let n = n.clone();
            move |_: String, _: String| {
                n.fetch_add(1, Ordering::SeqCst);
                async { tokio::time::sleep(Duration::from_secs(30)).await; Err::<ModelReply, _>("never".to_string()) }
            }
        };
        let t0 = std::time::Instant::now();
        let (out, not_filled) = fill::ai_values_within(Duration::from_millis(150), &pool, &client(&f), &dests, &ids, false, &hang).await;
        assert!(t0.elapsed() < Duration::from_secs(5));
        assert_eq!((out.len(), calls(&n)), (0, 1));
        assert_eq!(not_filled, vec!["Job tasks".to_string()]);
    }

    #[tokio::test]
    async fn tasks_of_deleted_meetings_keep_their_own_title() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        task(&pool, "t2", "keep", 0).await;
        for (id, title) in [("t1", "Alpha call"), ("t2", "Beta call")] {
            sqlx::query("UPDATE tasks SET meeting_id = NULL, meeting_title = ? WHERE id = ?").bind(title).bind(id).execute(&pool).await.unwrap();
        }
        let f = serve(vec![ds_reply(), ok("p1"), ok("p2")]).await;
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let call = {
            let seen = seen.clone();
            move |_: String, user: String| {
                seen.lock().unwrap().push(user);
                std::future::ready(Ok::<_, String>(model("{}")))
            }
        };
        send_with(&pool, &client(&f), None, false, call).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].contains("Alpha call") && !seen[0].contains("Beta call"));
        assert!(seen[1].contains("Beta call"));
    }

    #[tokio::test]
    async fn summary_still_reaches_the_model_without_a_transcript() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) VALUES ('m1', 'completed', 't', 't', ?)")
            .bind(r##"{"markdown":"# Notes on Dativ"}"##).execute(&pool).await.unwrap();
        let f = serve(vec![ds_reply(), ok("p1")]).await;
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let call = {
            let seen = seen.clone();
            move |_: String, user: String| {
                seen.lock().unwrap().push(user);
                std::future::ready(Ok::<_, String>(model("{}")))
            }
        };
        send_with(&pool, &client(&f), None, false, call).await.unwrap();
        let u = seen.lock().unwrap()[0].clone();
        assert!(u.contains("Notes on Dativ") && u.contains("No transcript."), "{u}");
    }

    #[tokio::test]
    async fn no_ai_call_when_off_or_nothing_to_fill() {
        let _l = SEND_TEST_LOCK.lock().await;
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // (a) aiFill off
        let pool = setup().await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![ok("p1")]).await;
        send_with(&pool, &client(&f), None, false, fake_ai(&n, Ok("{}"))).await.unwrap();
        assert_eq!((calls(&n), reqs(&f)), (0, 1));
        // (b) blocked destination: no model call, no schema request
        let pool = setup().await;
        ai_on(&pool).await;
        sqlx::query("UPDATE notion_destinations SET last_status = 'not_shared' WHERE id = 'd1'").execute(&pool).await.unwrap();
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![]).await;
        send_with(&pool, &client(&f), None, false, fake_ai(&n, Ok("{}"))).await.unwrap();
        assert_eq!((calls(&n), reqs(&f)), (0, 0));
        // (c) only a title column: one GET, no model call, no notice
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        let only_name = reply(200, "", &format!(r#"{{"id":"{DS}","properties":{{"Name":{{"id":"n","type":"title","title":{{}}}}}}}}"#));
        let f = serve(vec![only_name, ok("p1")]).await;
        let r = send_with(&pool, &client(&f), None, false, fake_ai(&n, Ok("{}"))).await.unwrap();
        assert_eq!((calls(&n), reqs(&f), r.sent), (0, 2, 1));
        assert!(r.ai_not_filled.is_empty());
    }

    #[tokio::test]
    async fn one_call_per_meeting_per_database() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m2', 'Other', 't', 't')").execute(&pool).await.unwrap();
        task(&pool, "t1", "keep", 0).await;
        task(&pool, "t2", "keep", 0).await;
        task(&pool, "t3", "keep", 0).await;
        sqlx::query("UPDATE tasks SET meeting_id = 'm2' WHERE id IN ('t2', 't3')").execute(&pool).await.unwrap();
        let f = serve(vec![ds_reply(), ok("p1"), ok("p2"), ok("p3")]).await;
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let call = fake_ai(&n, Ok(r#"{"1":{"Thema":"A"},"2":{"Thema":"B"}}"#));
        let r = send_with(&pool, &client(&f), None, false, call).await.unwrap();
        assert_eq!((calls(&n), r.sent), (2, 3));
        let gets = f.requests.lock().unwrap().iter().filter(|q| q.head.starts_with("GET")).count();
        assert_eq!(gets, 1);
    }

    #[tokio::test]
    async fn values_made_for_another_database_are_not_sent() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        let f = serve(vec![ds_reply(), ok("p1")]).await;
        let call = {
            let pool = pool.clone();
            move |_: String, _: String| {
                let pool = pool.clone();
                async move {
                    sqlx::query("UPDATE tasks SET dest_id = 'd2' WHERE id = 't1'").execute(&pool).await.unwrap();
                    Ok(model(r#"{"1":{"Thema":"Dativ"}}"#))
                }
            }
        };
        let r = send_with(&pool, &client(&f), None, false, call).await.unwrap();
        assert_eq!(r.sent, 1);
        assert!(!f.requests.lock().unwrap()[1].body.contains("Dativ"));
        assert_eq!(status(&pool, "d2").await.as_deref(), Some("connected"));
    }

    #[tokio::test]
    async fn a_task_the_reply_skips_is_reported() {
        let _l = SEND_TEST_LOCK.lock().await;
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        task(&pool, "t2", "keep", 0).await;
        let f = serve(vec![ds_reply(), ok("p1"), ok("p2")]).await;
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let r = send_with(&pool, &client(&f), None, false, fake_ai(&n, Ok(r#"{"1":{"Thema":"A"}}"#))).await.unwrap();
        assert_eq!((r.sent, r.ai_not_filled), (2, vec!["Job tasks".to_string()]));
        // An empty map for a present task is fine.
        let pool = setup().await;
        ai_on(&pool).await;
        task(&pool, "t1", "keep", 0).await;
        task(&pool, "t2", "keep", 0).await;
        let f = serve(vec![ds_reply(), ok("p1"), ok("p2")]).await;
        let r = send_with(&pool, &client(&f), None, false, fake_ai(&n, Ok(r#"{"1":{"Thema":"A"},"2":{}}"#))).await.unwrap();
        assert!(r.ai_not_filled.is_empty());
    }
}
