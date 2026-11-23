//! Tauri commands for Settings → Notion and sending. The token never leaves this file except to `NotionClient`.
use super::client::{CallError, NotionClient};
use super::mapping::{check_mapping, first_data_source, parse_database_id, parse_uuid, schema_from};
use super::{
    list_category_defaults, list_destinations, set_dest_status, DbSchema, DestStatus, Destination, NotionConfig, SendReport,
};
use crate::knowledge::tasks::tasks_changed;
use crate::state::AppState;
use serde_json::Value;
use sqlx::SqlitePool;
use tauri::State;

const NO_TOKEN: &str = "Add your Notion token in Settings → Notion first.";

fn token() -> Result<Option<String>, String> {
    crate::secrets::store()
        .get(crate::secrets::NOTION_TOKEN)
        .map_err(|e| format!("Couldn't read the saved Notion token: {e}"))
}

fn last4(token: &str) -> String {
    let n = token.chars().count();
    token.chars().skip(n.saturating_sub(4)).collect()
}

/// Cleans a pasted token: trims, drops a leading `Bearer`, and rejects anything that is not one printable word.
pub(crate) fn validate_token(raw: &str) -> Result<String, String> {
    let t = raw.trim();
    let t = match t.split_once(char::is_whitespace) {
        Some((w, rest)) if w.eq_ignore_ascii_case("bearer") => rest.trim(),
        _ => t,
    };
    if t.is_empty() {
        return Err("Paste a token first.".into());
    }
    if t.len() > 200 || !t.chars().all(|c| c.is_ascii_graphic()) {
        return Err("That doesn't look like a Notion token.".into());
    }
    Ok(t.to_string())
}

/// A new token gets a fresh chance: forget the token-related states.
pub(crate) async fn clear_token_states(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query("UPDATE notion_destinations SET last_status=NULL, last_message=NULL WHERE last_status IN ('token_missing','token_rejected')")
        .execute(pool)
        .await?;
    Ok(())
}

async fn record(pool: &SqlitePool, out: &mut Vec<DestStatus>, dest_id: &str, state: &str, message: Option<String>) -> anyhow::Result<()> {
    // Offline is transient: keep the last real outcome.
    if !dest_id.is_empty() && state != "offline" {
        set_dest_status(pool, dest_id, state, message.as_deref()).await?;
    }
    out.push(DestStatus { dest_id: dest_id.into(), state: state.into(), message });
    Ok(())
}

/// Element 0 is the token (`dest_id` ""), then one entry per destination in position order.
pub(crate) async fn test_all(pool: &SqlitePool, client: Option<&NotionClient>) -> anyhow::Result<Vec<DestStatus>> {
    let dests = list_destinations(pool).await?;
    let mut out = Vec::new();
    let Some(client) = client else {
        let m = || Some("Add your Notion token first.".to_string());
        record(pool, &mut out, "", "token_missing", m()).await?;
        for d in &dests {
            record(pool, &mut out, &d.id, "token_missing", m()).await?;
        }
        return Ok(out);
    };
    if let Err(e) = client.me().await {
        let (state, msg) = (e.state(), e.message("Notion"));
        if matches!(state, "token_rejected" | "offline") {
            record(pool, &mut out, "", state, Some(msg.clone())).await?;
            for d in &dests {
                record(pool, &mut out, &d.id, state, Some(msg.clone())).await?;
            }
            return Ok(out);
        }
    }
    record(pool, &mut out, "", "connected", None).await?;
    for d in &dests {
        let (state, msg) = check_destination(client, d).await;
        record(pool, &mut out, &d.id, state, msg).await?;
    }
    Ok(out)
}

async fn check_destination(client: &NotionClient, d: &Destination) -> (&'static str, Option<String>) {
    let fail = |e: CallError| (e.state(), Some(e.message(&d.name)));
    let db = match client.database(&d.database_id).await {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let listed = db
        .get("data_sources")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.get("id")?.as_str().and_then(parse_uuid)).collect::<Vec<_>>())
        .unwrap_or_default();
    let Some(ds_id) = d.data_source_id.as_deref().and_then(parse_uuid).filter(|id| listed.contains(id)) else {
        return ("not_found", Some(format!("{}: the database changed in Notion. Read its columns again in Settings.", d.name)));
    };
    let ds = match client.data_source(&ds_id).await {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let problems = check_mapping(&d.mapping, &schema_from(&d.database_id, &db, &ds));
    if problems.is_empty() {
        ("connected", None)
    } else {
        ("mapping_broken", Some(format!("{}: {}", d.name, problems.join("; "))))
    }
}

pub(crate) async fn read_columns(client: &NotionClient, link: &str) -> Result<DbSchema, String> {
    let id = parse_database_id(link)?;
    let this = |e: CallError| e.message("This database");
    let db = client.database(&id).await.map_err(this)?;
    let ds_id = first_data_source(&db).ok_or("This database has no data source Conclave can use.")?;
    let ds = client.data_source(&ds_id).await.map_err(this)?;
    Ok(schema_from(&id, &db, &ds))
}

fn client() -> Result<NotionClient, String> {
    token()?.map(NotionClient::new).ok_or_else(|| NO_TOKEN.to_string())
}

#[tauri::command]
pub async fn notion_get_config(state: State<'_, AppState>) -> Result<NotionConfig, String> {
    let pool = state.db_manager.pool();
    Ok(NotionConfig {
        token_last4: token()?.map(|t| last4(&t)),
        destinations: list_destinations(pool).await.map_err(|e| e.to_string())?,
        category_defaults: list_category_defaults(pool).await.map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
pub async fn notion_set_token(state: State<'_, AppState>, token: String) -> Result<(), String> {
    let t = validate_token(&token)?;
    crate::secrets::store()
        .set(crate::secrets::NOTION_TOKEN, &t)
        .map_err(|e| format!("Couldn't save the Notion token: {e}"))?;
    // The token is stored; a failed state reset only leaves stale statuses until the next Test.
    if let Err(e) = clear_token_states(state.db_manager.pool()).await {
        log::warn!("notion: could not clear token states after saving a token: {e}");
    }
    Ok(())
}

#[tauri::command]
pub async fn notion_remove_token() -> Result<(), String> {
    crate::secrets::store()
        .delete(crate::secrets::NOTION_TOKEN)
        .map_err(|e| format!("Couldn't remove the Notion token: {e}"))
}

#[tauri::command]
pub async fn notion_test(state: State<'_, AppState>) -> Result<Vec<DestStatus>, String> {
    let client = token()?.map(NotionClient::new);
    test_all(state.db_manager.pool(), client.as_ref()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn notion_read_columns(link: String) -> Result<DbSchema, String> {
    read_columns(&client()?, &link).await
}

#[tauri::command]
pub async fn notion_save_destination(state: State<'_, AppState>, dest: Destination) -> Result<Destination, String> {
    super::save_destination(state.db_manager.pool(), dest).await
}

#[tauri::command]
pub async fn notion_delete_destination(app: tauri::AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let r = super::delete_destination(state.db_manager.pool(), &id).await;
    tasks_changed(&app);
    r
}

#[tauri::command]
pub async fn notion_set_category_default(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    template_key: String,
    dest_id: Option<String>,
) -> Result<(), String> {
    let r = super::set_category_default(state.db_manager.pool(), &template_key, dest_id.as_deref()).await;
    tasks_changed(&app);
    r
}

#[tauri::command]
pub async fn notion_send(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    task_ids: Option<Vec<String>>,
    resend_ambiguous: Option<bool>,
) -> Result<SendReport, String> {
    let r = match client() {
        Ok(c) => super::send::send(state.db_manager.pool(), &c, task_ids, resend_ambiguous.unwrap_or(false)).await,
        Err(e) => Err(e),
    };
    // Rows may have changed even when send() failed part-way.
    tasks_changed(&app);
    r
}

#[cfg(test)]
mod tests {
    use super::super::client::fake::{reply, serve};
    use super::super::{save_destination, Mapping};
    use super::*;
    use crate::database::repositories::test_pool;
    use std::time::Duration;

    const DB: &str = "fedcba98-7654-3210-fedc-ba9876543210";
    const DS: &str = "01234567-89ab-cdef-0123-456789abcdef";

    fn client(base: &str) -> NotionClient {
        NotionClient::with_base(base, "ntn_abc123XYZ", Duration::from_millis(500))
    }

    async fn add(pool: &SqlitePool, name: &str, due: Option<&str>) -> Destination {
        let mapping = Mapping { title_prop: "Name".into(), due_prop: due.map(Into::into), meeting_prop: None, fixed: vec![], emoji: None, ai_fill: true };
        let d = Destination {
            id: String::new(),
            name: name.into(),
            colour: 1,
            position: 0,
            database_id: if name == "Job tasks" { DB.into() } else { name.bytes().map(|b| format!("{b:02x}")).collect::<String>() + &"0".repeat(32 - name.len() * 2) },
            data_source_id: Some(DS.into()),
            mapping,
            last_status: None,
            last_message: None,
        };
        save_destination(pool, d).await.unwrap()
    }

    fn db_json() -> String {
        format!(r#"{{"title":[{{"plain_text":"Job tasks"}}],"data_sources":[{{"id":"{DS}"}}]}}"#)
    }
    fn ds_json() -> String {
        format!(r#"{{"id":"{DS}","properties":{{"Name":{{"id":"title","type":"title"}}}}}}"#)
    }

    #[test]
    fn token_is_cleaned_and_only_last4_leaves() {
        for raw in ["  ntn_abc123XYZ \n", "Bearer ntn_abc123XYZ", "bearer  ntn_abc123XYZ"] {
            assert_eq!(validate_token(raw).unwrap(), "ntn_abc123XYZ");
        }
        assert_eq!(validate_token(" ").unwrap_err(), "Paste a token first.");
        assert!(validate_token("ntn abc").is_err());
        assert!(validate_token(&"a".repeat(201)).is_err());
        assert_eq!(last4("ntn_abc123XYZ"), "3XYZ");
    }

    #[tokio::test]
    async fn setting_a_token_clears_token_states() {
        let pool = test_pool().await;
        let a = add(&pool, "A", None).await;
        let b = add(&pool, "B", None).await;
        set_dest_status(&pool, &a.id, "token_rejected", Some("x")).await.unwrap();
        set_dest_status(&pool, &b.id, "not_shared", Some("y")).await.unwrap();
        clear_token_states(&pool).await.unwrap();
        let l = list_destinations(&pool).await.unwrap();
        assert_eq!((l[0].last_status.clone(), l[1].last_status.as_deref()), (None, Some("not_shared")));
    }

    #[tokio::test]
    async fn test_all_without_token_marks_everything_missing() {
        let pool = test_pool().await;
        let d = add(&pool, "A", None).await;
        let s = test_all(&pool, None).await.unwrap();
        assert_eq!(s.iter().map(|x| x.state.as_str()).collect::<Vec<_>>(), ["token_missing", "token_missing"]);
        assert_eq!((s[0].dest_id.as_str(), s[1].dest_id.as_str()), ("", d.id.as_str()));
    }

    #[tokio::test]
    async fn test_all_reports_rejected_token_without_database_calls() {
        let pool = test_pool().await;
        add(&pool, "A", None).await;
        let f = serve(vec![reply(401, "", r#"{"code":"unauthorized"}"#)]).await;
        let s = test_all(&pool, Some(&client(&f.base))).await.unwrap();
        assert!(s.iter().all(|x| x.state == "token_rejected"));
        assert_eq!(s.len(), 2);
        assert_eq!(f.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_all_checks_each_database_and_mapping() {
        let pool = test_pool().await;
        let d = add(&pool, "Job tasks", Some("Deadline")).await;
        let f = serve(vec![reply(200, "", "{}"), reply(200, "", &db_json()), reply(200, "", &ds_json())]).await;
        let s = test_all(&pool, Some(&client(&f.base))).await.unwrap();
        assert_eq!(s[0].state, "connected");
        assert_eq!(
            (s[1].dest_id.as_str(), s[1].state.as_str(), s[1].message.as_deref()),
            (d.id.as_str(), "mapping_broken", Some("Job tasks: “Deadline” is missing"))
        );
        let heads: Vec<String> = f.requests.lock().unwrap().iter().map(|r| r.head.clone()).collect();
        assert!(heads[1].starts_with(&format!("GET /databases/{DB} ")) && heads[2].starts_with(&format!("GET /data_sources/{DS} ")));
        assert_eq!(list_destinations(&pool).await.unwrap()[0].last_status.as_deref(), Some("mapping_broken"));
    }

    #[tokio::test]
    async fn test_all_maps_404_to_not_shared() {
        let pool = test_pool().await;
        add(&pool, "Personal tasks", None).await;
        let f = serve(vec![reply(200, "", "{}"), reply(404, "", "{}")]).await;
        let s = test_all(&pool, Some(&client(&f.base))).await.unwrap();
        assert_eq!(s[1].state, "not_shared");
        assert_eq!(
            s[1].message.as_deref(),
            Some("Personal tasks isn't shared with Conclave, or was deleted. In Notion: database ••• → Connections → add Conclave")
        );
    }

    #[tokio::test]
    async fn test_all_flags_a_changed_data_source_and_keeps_state_when_offline() {
        let pool = test_pool().await;
        let d = add(&pool, "Job tasks", None).await;
        set_dest_status(&pool, &d.id, "connected", None).await.unwrap();
        let f = serve(vec![reply(200, "", "{}"), reply(200, "", r#"{"data_sources":[{"id":"11111111-1111-1111-1111-111111111111"}]}"#)]).await;
        let s = test_all(&pool, Some(&client(&f.base))).await.unwrap();
        assert_eq!(s[1].state, "not_found");
        let dead = NotionClient::with_base("http://127.0.0.1:1", "t", Duration::from_millis(200));
        let s = test_all(&pool, Some(&dead)).await.unwrap();
        assert_eq!(s[1].state, "offline");
        assert_eq!(list_destinations(&pool).await.unwrap()[0].last_status.as_deref(), Some("not_found"));
    }

    #[tokio::test]
    async fn a_malformed_stored_id_is_a_local_problem_not_a_notion_refusal() {
        let pool = test_pool().await;
        let d = add(&pool, "Job tasks", None).await;
        sqlx::query("UPDATE notion_destinations SET database_id = 'junk' WHERE id = ?").bind(&d.id).execute(&pool).await.unwrap();
        let f = serve(vec![reply(200, "", "{}")]).await;
        let s = test_all(&pool, Some(&client(&f.base))).await.unwrap();
        assert_eq!(s[1].message.as_deref(), Some("Job tasks: the stored database id is invalid. Re-add it in Settings → Notion."));
        assert_eq!(f.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn read_columns_reads_database_then_its_first_data_source() {
        let f = serve(vec![reply(200, "", &db_json()), reply(200, "", &ds_json())]).await;
        let link = format!("https://www.notion.so/ws/Job-tasks-{}?v=1", DB.replace('-', ""));
        let s = read_columns(&client(&f.base), &link).await.unwrap();
        assert_eq!((s.title.as_str(), s.database_id.as_str(), s.data_source_id.as_deref()), ("Job tasks", DB, Some(DS)));
        assert!(read_columns(&client(&f.base), "not a link").await.is_err());
    }
}
