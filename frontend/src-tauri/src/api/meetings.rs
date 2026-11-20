//! The meetings table: rows, create, reviewed/template setters and saved views (redesign spec 6).
use crate::database::models::DateTimeUtc;
use crate::database::repositories::meeting::{meeting_date_sql, MeetingsRepository};
use crate::state::AppState;
use crate::summary::templates;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

const MAX_VIEW_NAME: usize = 80;
const MAX_VIEW_CONFIG: usize = 64 * 1024;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MeetingRow {
    pub id: String,
    pub title: String,
    pub title_source: String,
    pub template_id: Option<String>,
    #[sqlx(skip)]
    pub emoji: Option<String>,
    pub reviewed: bool,
    pub gist: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub created_at: DateTimeUtc,
    pub calendar_uid: Option<String>,
    pub calendar_start: Option<String>,
    pub calendar_end: Option<String>,
    pub calendar_cancelled_at: Option<String>,
    pub has_transcript: bool,
    pub has_summary: bool,
    pub prep_points: i64,
    pub has_prep: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MeetingView {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub builtin: bool,
    pub config_json: String,
}

pub(crate) async fn list_rows_inner(pool: &SqlitePool) -> Result<Vec<MeetingRow>, String> {
    let sql = format!(
        "SELECT m.id, m.title, m.title_source, m.template_id, m.reviewed, m.gist, m.started_at, m.ended_at, m.created_at,
                m.calendar_uid, m.calendar_start, m.calendar_end, m.calendar_cancelled_at,
                EXISTS(SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id) AS has_transcript,
                EXISTS(SELECT 1 FROM summary_processes s WHERE s.meeting_id = m.id AND LOWER(s.status) = 'completed') AS has_summary,
                COALESCE((SELECT CASE WHEN json_valid(p.items_json) AND json_type(p.items_json) = 'array'
                                      THEN json_array_length(p.items_json) END
                          FROM meeting_preps p WHERE p.meeting_id = m.id), 0) AS prep_points,
                EXISTS(SELECT 1 FROM meeting_preps p WHERE p.meeting_id = m.id
                       AND (TRIM(p.material) <> '' OR p.chat_json NOT IN ('','[]') OR p.items_json NOT IN ('','[]'))) AS has_prep
         FROM meetings m ORDER BY julianday({}) DESC, m.id",
        meeting_date_sql("m")
    );
    let mut rows: Vec<MeetingRow> = sqlx::query_as(&sql).fetch_all(pool).await.map_err(|e| format!("Failed to list meetings: {e}"))?;
    let listing = tokio::task::spawn_blocking(templates::list_templates).await.map_err(|e| format!("Failed to read templates: {e}"))?;
    fill_emoji(&mut rows, &listing);
    Ok(rows)
}

fn fill_emoji(rows: &mut [MeetingRow], listing: &templates::TemplateListing) {
    for r in rows {
        r.emoji = r.template_id.as_deref().and_then(|id| {
            listing.templates.iter().find(|t| t.id == id).and_then(|t| t.template.emoji.clone()).filter(|e| !e.is_empty())
        });
    }
}

async fn require_changed(res: Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error>, what: &str, id: &str) -> Result<(), String> {
    match res {
        Ok(r) if r.rows_affected() > 0 => Ok(()),
        Ok(_) => Err(format!("{what} not found: {id}")),
        Err(e) => Err(format!("Failed to update {what}: {e}")),
    }
}

pub(crate) async fn set_reviewed_inner(pool: &SqlitePool, meeting_id: &str, reviewed: bool) -> Result<(), String> {
    let res = sqlx::query("UPDATE meetings SET reviewed = ? WHERE id = ?").bind(reviewed).bind(meeting_id).execute(pool).await;
    require_changed(res, "Meeting", meeting_id).await
}

pub(crate) async fn set_template_inner(pool: &SqlitePool, meeting_id: &str, template_id: Option<String>) -> Result<(), String> {
    if let Some(id) = &template_id {
        if !templates::is_valid_id(id) {
            return Err(format!("Invalid template id: {id}"));
        }
    }
    let res = sqlx::query("UPDATE meetings SET template_id = ? WHERE id = ?").bind(template_id).bind(meeting_id).execute(pool).await;
    require_changed(res, "Meeting", meeting_id).await
}

pub(crate) async fn list_views_inner(pool: &SqlitePool) -> Result<Vec<MeetingView>, String> {
    sqlx::query_as("SELECT id, name, position, builtin, config_json FROM meeting_views ORDER BY position, id")
        .fetch_all(pool).await.map_err(|e| format!("Failed to list views: {e}"))
}

pub(crate) async fn save_view_inner(pool: &SqlitePool, view: MeetingView) -> Result<(), String> {
    if view.id.trim().is_empty() || view.id.len() > 200 {
        return Err("View id must be 1-200 characters".into());
    }
    if view.name.trim().is_empty() || view.name.chars().count() > MAX_VIEW_NAME {
        return Err(format!("View name must be 1-{MAX_VIEW_NAME} characters"));
    }
    if view.config_json.len() > MAX_VIEW_CONFIG {
        return Err("View config is too large".into());
    }
    serde_json::from_str::<serde_json::Value>(&view.config_json).map_err(|e| format!("View config is not valid JSON: {e}"))?;
    // `builtin` is never taken from the caller: an existing row keeps it, a new row is 0.
    sqlx::query(
        "INSERT INTO meeting_views (id, name, position, builtin, config_json) VALUES (?, ?, ?, 0, ?)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, position = excluded.position, config_json = excluded.config_json",
    )
    .bind(&view.id).bind(&view.name).bind(view.position).bind(&view.config_json)
    .execute(pool).await.map_err(|e| format!("Failed to save view: {e}"))?;
    Ok(())
}

pub(crate) async fn delete_view_inner(pool: &SqlitePool, id: &str) -> Result<(), String> {
    let builtin: Option<bool> = sqlx::query_scalar("SELECT builtin FROM meeting_views WHERE id = ?")
        .bind(id).fetch_optional(pool).await.map_err(|e| format!("Failed to delete view: {e}"))?;
    match builtin {
        None => Err(format!("View not found: {id}")),
        Some(true) => Err("Built-in views cannot be deleted".into()),
        Some(false) => {
            sqlx::query("DELETE FROM meeting_views WHERE id = ? AND builtin = 0").bind(id).execute(pool).await
                .map_err(|e| format!("Failed to delete view: {e}"))?;
            Ok(())
        }
    }
}

#[tauri::command]
pub async fn api_list_meeting_rows(state: tauri::State<'_, AppState>) -> Result<Vec<MeetingRow>, String> {
    list_rows_inner(state.db_manager.pool()).await
}

#[tauri::command]
pub async fn api_create_meeting(state: tauri::State<'_, AppState>, title: Option<String>) -> Result<String, String> {
    MeetingsRepository::create_meeting(state.db_manager.pool(), title.as_deref(), chrono::Local::now().naive_local())
        .await.map_err(|e| format!("Failed to create meeting: {e}"))
}

#[tauri::command]
pub async fn api_set_meeting_reviewed(state: tauri::State<'_, AppState>, meeting_id: String, reviewed: bool) -> Result<(), String> {
    set_reviewed_inner(state.db_manager.pool(), &meeting_id, reviewed).await
}

#[tauri::command]
pub async fn api_set_meeting_template(state: tauri::State<'_, AppState>, meeting_id: String, template_id: Option<String>) -> Result<(), String> {
    set_template_inner(state.db_manager.pool(), &meeting_id, template_id).await
}

#[tauri::command]
pub async fn api_list_meeting_views(state: tauri::State<'_, AppState>) -> Result<Vec<MeetingView>, String> {
    list_views_inner(state.db_manager.pool()).await
}

#[tauri::command]
pub async fn api_save_meeting_view(state: tauri::State<'_, AppState>, view: MeetingView) -> Result<(), String> {
    save_view_inner(state.db_manager.pool(), view).await
}

// The frontend wrapper sends `{ id }`, so the argument is `id` (P18 said `view_id`; the wrapper wins).
#[tauri::command]
pub async fn api_delete_meeting_view(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    delete_view_inner(state.db_manager.pool(), &id).await
}

/// "Delete recording": keeps the meeting, its prep and notes. Same result shape as `api_delete_meeting`.
#[tauri::command]
pub async fn api_delete_recording(app: tauri::AppHandle, state: tauri::State<'_, AppState>, meeting_id: String) -> Result<serde_json::Value, String> {
    crate::audio::recording_commands::ensure_not_live_now(&meeting_id)?;
    let base = crate::audio::recording_preferences::get_default_recordings_folder();
    let warning = crate::audio::meeting_folder::delete_recording_and_audio(
        state.db_manager.pool(),
        &meeting_id,
        &base,
        &crate::audio::meeting_folder::recycle,
    )
    .await
    .map_err(|e| format!("Failed to delete recording: {e}"))?
    .ok_or_else(|| format!("Meeting not found: {meeting_id}"))?;
    crate::knowledge::tasks::tasks_changed(&app); // the delete clears or removes task rows (R10)
    Ok(serde_json::json!({
        "status": "success",
        "message": "Recording deleted successfully",
        "folder_warning": warning,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;

    async fn exec(pool: &SqlitePool, sql: &str) {
        sqlx::query(sql).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn rows_flags_prep_points_and_order() {
        let pool = test_pool().await;
        exec(&pool, "INSERT INTO meetings (id,title,created_at,updated_at,started_at) VALUES ('rec','R','2026-11-10T10:00:00Z','x','2026-11-10T10:00:00+00:00')").await;
        exec(&pool, "INSERT INTO transcripts (id,meeting_id,transcript,timestamp) VALUES ('t','rec','hi','x')").await;
        exec(&pool, "INSERT INTO summary_processes (meeting_id,status,created_at,updated_at) VALUES ('rec','Completed','x','x')").await;
        exec(&pool, "INSERT INTO meetings (id,title,created_at,updated_at,calendar_uid,calendar_start) VALUES ('new','N','2026-11-01T10:00:00Z','x','u','2026-11-12T09:00:00+00:00')").await;
        exec(&pool, "INSERT INTO meeting_preps (meeting_id,created_at,updated_at,items_json) VALUES ('new','x','x','[\"a\",\"b\",\"c\"]')").await;
        exec(&pool, "INSERT INTO meetings (id,title,created_at,updated_at) VALUES ('bad','B','2026-11-02T10:00:00Z','x')").await;
        exec(&pool, "INSERT INTO meeting_preps (meeting_id,created_at,updated_at,items_json) VALUES ('bad','x','x','{oops')").await;
        exec(&pool, "INSERT INTO meetings (id,title,created_at,updated_at) VALUES ('pend','P','2026-11-01T00:00:00Z','x')").await;
        exec(&pool, "INSERT INTO summary_processes (meeting_id,status,created_at,updated_at) VALUES ('pend','processing','x','x')").await;
        let rows = list_rows_inner(&pool).await.unwrap();
        let got: Vec<_> = rows.iter().map(|r| (r.id.as_str(), r.has_transcript, r.has_summary, r.prep_points)).collect();
        assert_eq!(got, [("new", false, false, 3), ("rec", true, true, 0), ("bad", false, false, 0), ("pend", false, false, 0)]);
        assert!(rows[0].created_at.0.to_rfc3339().starts_with("2026-11-01T10:00:00"));
    }

    #[tokio::test]
    async fn has_prep_covers_material_and_chat_not_just_points() {
        let pool = test_pool().await;
        for id in ["mat", "chat", "empty", "none"] {
            exec(&pool, &format!("INSERT INTO meetings (id,title,created_at,updated_at) VALUES ('{id}','T','2026-11-01T10:00:00Z','x')")).await;
        }
        exec(&pool, "INSERT INTO meeting_preps (meeting_id,created_at,updated_at,material) VALUES ('mat','x','x','some text')").await;
        exec(&pool, "INSERT INTO meeting_preps (meeting_id,created_at,updated_at,chat_json) VALUES ('chat','x','x','[{\"role\":\"user\"}]')").await;
        exec(&pool, "INSERT INTO meeting_preps (meeting_id,created_at,updated_at,material) VALUES ('empty','x','x','  ')").await;
        let rows = list_rows_inner(&pool).await.unwrap();
        let mut got: Vec<_> = rows.iter().map(|r| (r.id.as_str(), r.has_prep, r.prep_points)).collect();
        got.sort();
        assert_eq!(got, [("chat", true, 0), ("empty", false, 0), ("mat", true, 0), ("none", false, 0)]);
    }

    #[tokio::test]
    async fn row_emoji_comes_from_the_template_listing() {
        let pool = test_pool().await;
        for (id, tpl) in [("a", "interview"), ("b", "ghost"), ("c", "mine")] {
            exec(&pool, &format!("INSERT INTO meetings (id,title,created_at,updated_at,template_id) VALUES ('{id}','T','2026-11-01T10:00:00Z','x','{tpl}')")).await;
        }
        let dir = tempfile::tempdir().unwrap();
        let write = |id: &str, emoji: &str| std::fs::write(dir.path().join(format!("{id}.json")),
            format!(r#"{{"name":"{id}",{emoji}"sections":[{{"title":"A","instruction":"B"}}]}}"#)).unwrap();
        write("interview", "");
        write("mine", r#""emoji":"","#);
        let listing = templates::list_templates_in(dir.path());
        let mut rows = list_rows_inner(&pool).await.unwrap();
        fill_emoji(&mut rows, &listing);
        let got: Vec<_> = rows.iter().map(|r| (r.id.as_str(), r.emoji.as_deref())).collect();
        assert_eq!(got, [("a", Some("🤝")), ("b", None), ("c", None)]);
    }

    #[test]
    fn meeting_view_wire_shape_is_camel_case() {
        let v: MeetingView = serde_json::from_str(r#"{"id":"x","name":"N","position":1,"builtin":false,"configJson":"{}"}"#).unwrap();
        assert_eq!((v.id.as_str(), v.name.as_str(), v.position, v.builtin, v.config_json.as_str()), ("x", "N", 1, false, "{}"));
        assert_eq!(serde_json::to_value(&v).unwrap(), serde_json::json!({"id":"x","name":"N","position":1,"builtin":false,"configJson":"{}"}));
    }

    #[tokio::test]
    async fn setters_validate() {
        let pool = test_pool().await;
        exec(&pool, "INSERT INTO meetings (id,title,created_at,updated_at) VALUES ('m','M','2026-11-01T10:00:00Z','x')").await;
        assert!(set_reviewed_inner(&pool, "nope", true).await.is_err());
        assert!(set_template_inner(&pool, "nope", None).await.is_err());
        assert!(set_template_inner(&pool, "m", Some("../x".into())).await.is_err());
        set_reviewed_inner(&pool, "m", true).await.unwrap();
        set_template_inner(&pool, "m", Some("weekly".into())).await.unwrap();
        let t: Option<String> = sqlx::query_scalar("SELECT template_id FROM meetings WHERE id='m'").fetch_one(&pool).await.unwrap();
        assert_eq!(t.as_deref(), Some("weekly"));
        set_template_inner(&pool, "m", None).await.unwrap();
        let t: Option<String> = sqlx::query_scalar("SELECT template_id FROM meetings WHERE id='m'").fetch_one(&pool).await.unwrap();
        assert_eq!(t, None);
    }

    fn view(id: &str, name: &str, cfg: &str, builtin: bool) -> MeetingView {
        MeetingView { id: id.into(), name: name.into(), position: 9, builtin, config_json: cfg.into() }
    }

    #[tokio::test]
    async fn views_crud_and_guards() {
        let pool = test_pool().await;
        save_view_inner(&pool, view("mine", "Mine", "{}", true)).await.unwrap();
        save_view_inner(&pool, view("all", "Everything", "{}", false)).await.unwrap();
        let views = list_views_inner(&pool).await.unwrap();
        assert_eq!(views.iter().find(|v| v.id == "mine").unwrap().builtin, false);
        let all = views.iter().find(|v| v.id == "all").unwrap();
        assert!(all.builtin && all.name == "Everything");
        assert_eq!(views.iter().map(|v| v.position).collect::<Vec<_>>(), [1, 2, 9, 9]);
        for bad in [view("x", "n", "{", false), view("x", "n", &format!("\"{}\"", "a".repeat(70_000)), false), view("", "n", "{}", false), view("x", &"n".repeat(81), "{}", false), view("x", " ", "{}", false)] {
            assert!(save_view_inner(&pool, bad).await.is_err());
        }
        assert_eq!(delete_view_inner(&pool, "week").await.unwrap_err(), "Built-in views cannot be deleted");
        assert!(delete_view_inner(&pool, "ghost").await.is_err());
        delete_view_inner(&pool, "mine").await.unwrap();
        assert_eq!(list_views_inner(&pool).await.unwrap().len(), 3);
    }
}
