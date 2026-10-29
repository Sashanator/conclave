use super::{
    count_label, ChatMessage, MeetingPrep, PrepItem, ProposedItem, KINDS, MAX_ITEM_CHARS,
    MAX_MATERIAL_CHARS, MAX_PREP_ITEMS,
};
use crate::{knowledge::MAX_EVENT_TITLE_CHARS, state::AppState, summary::templates::is_valid_id};
use chrono::Utc;
use log::warn;
use sqlx::{sqlite::SqliteRow, Row, SqliteConnection, SqlitePool};
use tauri::State;
use uuid::Uuid;

pub(crate) fn merge_items(current: &[PrepItem], proposed: Vec<ProposedItem>) -> Vec<PrepItem> {
    let mut used = vec![false; current.len()];
    proposed
        .into_iter()
        .filter_map(|p| {
            // Truncate before matching, so an over-long proposal still finds its stored copy.
            let text = clean_text(&p.text);
            if text.is_empty() {
                return None;
            }
            let kind = if KINDS.contains(&p.kind.as_str()) {
                p.kind
            } else {
                "other".into()
            };
            let matching = current
                .iter()
                .enumerate()
                .position(|(i, item)| !used[i] && item.text.trim() == text);
            let mut item = match matching {
                Some(i) => {
                    used[i] = true;
                    current[i].clone()
                }
                None => PrepItem {
                    id: Uuid::new_v4().to_string(),
                    text: String::new(),
                    kind: String::new(),
                    ticked: false,
                    by_user: false,
                },
            };
            item.text = text;
            item.kind = kind;
            Some(item)
        })
        .take(MAX_PREP_ITEMS)
        .collect()
}

// Trimmed, then cut to the item limit, then trimmed again so no trailing space survives the cut.
fn clean_text(text: &str) -> String {
    text.trim()
        .chars()
        .take(MAX_ITEM_CHARS)
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn decode(row: SqliteRow) -> Result<MeetingPrep, String> {
    // Parsing failures must reach callers; replacing corrupted JSON would lose data on the next save.
    let id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
    let parse = || -> Result<MeetingPrep, Box<dyn std::error::Error>> {
        Ok(MeetingPrep {
            id: row.try_get("id")?,
            event_uid: row.try_get("event_uid")?,
            event_start: row.try_get("event_start")?,
            event_title: row.try_get("event_title")?,
            template_id: row.try_get("template_id")?,
            material: row.try_get("material")?,
            chat: serde_json::from_str(row.try_get::<&str, _>("chat_json")?)?,
            items: serde_json::from_str(row.try_get::<&str, _>("items_json")?)?,
            approved_at: row.try_get("approved_at")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    };
    parse().map_err(|e| {
        warn!("Meeting prep {id} has unreadable stored data");
        format!("Prep data is unreadable (id {id}): {e}")
    })
}

pub(crate) async fn get(
    pool: &SqlitePool,
    event_uid: &str,
    event_start: &str,
) -> Result<Option<MeetingPrep>, String> {
    sqlx::query("SELECT * FROM meeting_preps WHERE event_uid = ? AND event_start = ?")
        .bind(event_uid)
        .bind(event_start)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .map(decode)
        .transpose()
}

pub(crate) async fn get_by_id(pool: &SqlitePool, id: i64) -> Result<Option<MeetingPrep>, String> {
    sqlx::query("SELECT * FROM meeting_preps WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .map(decode)
        .transpose()
}

async fn require_prep(conn: &mut SqliteConnection, id: i64) -> Result<MeetingPrep, String> {
    sqlx::query("SELECT * FROM meeting_preps WHERE id = ?")
        .bind(id)
        .fetch_optional(conn)
        .await
        .map_err(|e| e.to_string())?
        .map(decode)
        .transpose()?
        .ok_or_else(|| "Prep not found".into())
}

pub(crate) async fn for_meeting(
    pool: &SqlitePool,
    meeting_id: &str,
) -> Result<Option<MeetingPrep>, String> {
    sqlx::query("SELECT p.* FROM meeting_preps p JOIN meetings m ON m.calendar_uid = p.event_uid AND m.calendar_start = p.event_start WHERE m.id = ?")
        .bind(meeting_id).fetch_optional(pool).await.map_err(|e| e.to_string())?
        .map(decode).transpose()
}

pub(crate) async fn list_all(pool: &SqlitePool) -> Result<Vec<MeetingPrep>, String> {
    sqlx::query("SELECT * FROM meeting_preps ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(decode)
        .collect()
}

pub(crate) async fn save(
    pool: &SqlitePool,
    event_uid: &str,
    event_start: &str,
    event_title: &str,
    material: &str,
    template_id: Option<&str>,
    mut items: Vec<PrepItem>,
) -> Result<MeetingPrep, String> {
    let count = material.chars().count();
    if count > MAX_MATERIAL_CHARS {
        return Err(count_label(count, MAX_MATERIAL_CHARS));
    }
    if template_id.is_some_and(|id| !is_valid_id(id)) {
        return Err("Invalid template".into());
    }
    if items.len() > MAX_PREP_ITEMS {
        return Err(format!("Too many items (maximum {MAX_PREP_ITEMS})"));
    }
    for item in &mut items {
        // Trimmed here, so the stored copy matches what merge_items compares against.
        item.text = item.text.trim().to_string();
        if item.text.is_empty() {
            return Err("Item text is empty".into());
        }
        if item.text.chars().count() > MAX_ITEM_CHARS {
            return Err(format!(
                "Item too long (maximum {MAX_ITEM_CHARS} characters)"
            ));
        }
    }

    // Lock before reading so a concurrent tick cannot be overwritten by this save.
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|e| e.to_string())?;
    let current =
        sqlx::query("SELECT * FROM meeting_preps WHERE event_uid = ? AND event_start = ?")
            .bind(event_uid)
            .bind(event_start)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?
            .map(decode)
            .transpose()?;
    let mut seen = std::collections::HashSet::new();
    for item in &mut items {
        // A repeated id would share one stored tick, and tick() only reaches the first copy.
        if item.id.is_empty() || !seen.insert(item.id.clone()) {
            item.id = Uuid::new_v4().to_string();
        }
        if !KINDS.contains(&item.kind.as_str()) {
            item.kind = "other".into();
        }
        let previous = current
            .as_ref()
            .and_then(|p| p.items.iter().find(|old| old.id == item.id));
        item.ticked = previous.is_some_and(|old| old.ticked);
        item.by_user = previous.map_or(true, |old| {
            old.by_user || old.text != item.text || old.kind != item.kind
        });
    }
    let now = Utc::now().to_rfc3339();
    let updated_at = current
        .as_ref()
        .filter(|old| {
            old.material == material
                && old.template_id.as_deref() == template_id
                && old.items == items
        })
        .map_or(now.as_str(), |old| old.updated_at.as_str());
    let title: String = event_title.chars().take(MAX_EVENT_TITLE_CHARS).collect();
    let items_json = serde_json::to_string(&items).map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO meeting_preps (event_uid, event_start, event_title, material, template_id, items_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(event_uid, event_start) DO UPDATE SET event_title = excluded.event_title, material = excluded.material, template_id = excluded.template_id, items_json = excluded.items_json, updated_at = excluded.updated_at")
        .bind(event_uid).bind(event_start).bind(title).bind(material).bind(template_id).bind(items_json).bind(&now).bind(updated_at)
        .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    let row = sqlx::query("SELECT * FROM meeting_preps WHERE event_uid = ? AND event_start = ?")
        .bind(event_uid)
        .bind(event_start)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    let prep = decode(row)?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(prep)
}

pub(crate) async fn tick(
    pool: &SqlitePool,
    prep_id: i64,
    item_id: &str,
    ticked: bool,
) -> Result<MeetingPrep, String> {
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|e| e.to_string())?;
    let mut prep = require_prep(&mut tx, prep_id).await?;
    let item = prep
        .items
        .iter_mut()
        .find(|i| i.id == item_id)
        .ok_or("Item not found")?;
    item.ticked = ticked;
    sqlx::query("UPDATE meeting_preps SET items_json = ? WHERE id = ?")
        .bind(serde_json::to_string(&prep.items).map_err(|e| e.to_string())?)
        .bind(prep_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(prep)
}

pub(crate) async fn approve(pool: &SqlitePool, prep_id: i64) -> Result<MeetingPrep, String> {
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|e| e.to_string())?;
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE meeting_preps SET approved_at = ?, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(prep_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    let prep = require_prep(&mut tx, prep_id).await?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(prep)
}

pub(crate) async fn delete(pool: &SqlitePool, prep_id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM meeting_preps WHERE id = ?")
        .bind(prep_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn apply_chat(
    pool: &SqlitePool,
    prep_id: i64,
    user: ChatMessage,
    mut reply: ChatMessage,
    proposed: Option<Vec<ProposedItem>>,
) -> Result<(MeetingPrep, bool), String> {
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|e| e.to_string())?;
    let mut prep = require_prep(&mut tx, prep_id).await?;
    // An empty proposal must never wipe the list, so it counts as no proposal.
    let proposed = proposed.filter(|p| !p.is_empty());
    let list_updated = proposed.is_some();
    let items_changed = if let Some(proposed) = proposed {
        let items = merge_items(&prep.items, proposed);
        let changed = items != prep.items;
        prep.items = items;
        changed
    } else {
        reply.list_not_updated = true;
        false
    };
    prep.chat.push(user);
    prep.chat.push(reply);
    if items_changed {
        prep.updated_at = Utc::now().to_rfc3339();
    }
    sqlx::query(
        "UPDATE meeting_preps SET items_json = ?, chat_json = ?, updated_at = ? WHERE id = ?",
    )
    .bind(serde_json::to_string(&prep.items).map_err(|e| e.to_string())?)
    .bind(serde_json::to_string(&prep.chat).map_err(|e| e.to_string())?)
    .bind(&prep.updated_at)
    .bind(prep_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok((prep, list_updated))
}

#[tauri::command]
pub async fn prep_get(
    state: State<'_, AppState>,
    event_uid: String,
    event_start: String,
) -> Result<Option<MeetingPrep>, String> {
    get(state.db_manager.pool(), &event_uid, &event_start).await
}

#[tauri::command]
pub async fn prep_for_meeting(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<MeetingPrep>, String> {
    for_meeting(state.db_manager.pool(), &meeting_id).await
}

#[tauri::command]
pub async fn prep_save(
    state: State<'_, AppState>,
    event_uid: String,
    event_start: String,
    event_title: String,
    material: String,
    template_id: Option<String>,
    items: Vec<PrepItem>,
) -> Result<MeetingPrep, String> {
    save(
        state.db_manager.pool(),
        &event_uid,
        &event_start,
        &event_title,
        &material,
        template_id.as_deref(),
        items,
    )
    .await
}

#[tauri::command]
pub async fn prep_tick(
    state: State<'_, AppState>,
    prep_id: i64,
    item_id: String,
    ticked: bool,
) -> Result<MeetingPrep, String> {
    tick(state.db_manager.pool(), prep_id, &item_id, ticked).await
}

#[tauri::command]
pub async fn prep_approve(state: State<'_, AppState>, prep_id: i64) -> Result<MeetingPrep, String> {
    approve(state.db_manager.pool(), prep_id).await
}

#[tauri::command]
pub async fn prep_delete(state: State<'_, AppState>, prep_id: i64) -> Result<(), String> {
    delete(state.db_manager.pool(), prep_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::{meeting::MeetingsRepository, test_pool};

    fn item(id: &str, text: &str) -> PrepItem {
        PrepItem {
            id: id.into(),
            text: text.into(),
            kind: "question".into(),
            ticked: false,
            by_user: false,
        }
    }

    fn proposed(text: &str, kind: &str) -> ProposedItem {
        ProposedItem {
            text: text.into(),
            kind: kind.into(),
        }
    }

    fn message(role: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            text: "message".into(),
            at: chrono::Utc::now().to_rfc3339(),
            list_not_updated: false,
        }
    }

    async fn seed(pool: &sqlx::SqlitePool, items: Vec<PrepItem>) -> MeetingPrep {
        save(pool, "u", "2026-10-29T12:00:00Z", "Event", "", None, items)
            .await
            .unwrap()
    }

    async fn linked_meeting(pool: &sqlx::SqlitePool) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, calendar_uid, calendar_start) VALUES ('M', 'Meeting', 't', 't', 'u', '2026-10-29T12:00:00Z')")
            .execute(pool).await.unwrap();
    }

    #[test]
    fn merge_keeps_identity_ticks_and_user_items() {
        let mut a = item("a", "Ask X");
        a.ticked = true;
        let mut b = item("b", "Mine");
        b.by_user = true;
        let merged = merge_items(
            &[a, b],
            vec![
                proposed(" Ask X ", "check"),
                proposed("Mine", "clarify"),
                proposed("New", "bogus"),
            ],
        );
        assert_eq!(merged[0].id, "a");
        assert!(merged[0].ticked);
        assert_eq!(merged[0].kind, "check");
        assert_eq!(merged[1].id, "b");
        assert!(merged[1].by_user);
        assert!(uuid::Uuid::parse_str(&merged[2].id).is_ok());
        assert!(!merged[2].ticked);
        assert!(!merged[2].by_user);
        assert_eq!(merged[2].kind, "other");
    }

    #[test]
    fn merge_never_ticks_new_items() {
        let mut old = item("old", "old");
        old.ticked = true;
        let merged = merge_items(
            &[old],
            vec![proposed("new", "check"), proposed("another", "question")],
        );
        assert!(merged.iter().all(|i| !i.ticked));
    }

    #[test]
    fn merge_caps_and_drops_blank() {
        let proposed = (0..60)
            .map(|i| {
                proposed(
                    &if i % 20 == 0 {
                        "  ".into()
                    } else {
                        format!("{i}{}", "я".repeat(600))
                    },
                    "question",
                )
            })
            .collect();
        let merged = merge_items(&[], proposed);
        assert_eq!(merged.len(), 50);
        assert!(merged
            .iter()
            .all(|i| !i.text.is_empty() && i.text.chars().count() <= 500));
    }

    #[tokio::test]
    async fn save_keeps_ticks_from_db() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("i1", "First")]).await;
        tick(&pool, prep.id, "i1", true).await.unwrap();
        let saved = seed(&pool, vec![item("i1", "First"), item("", "New")]).await;
        assert!(saved.items[0].ticked);
        assert!(!saved.items[1].id.is_empty());
        assert!(!saved.items[1].ticked);
        assert!(saved.items.iter().all(|i| i.by_user));
    }

    #[tokio::test]
    async fn save_refuses_long_material() {
        let pool = test_pool().await;
        let err = save(&pool, "u", "s", "title", &"x".repeat(40_001), None, vec![])
            .await
            .unwrap_err();
        assert_eq!(err, "40,001 / 40,000 characters");
        assert!(
            save(&pool, "u", "s", "title", &"я".repeat(40_000), None, vec![])
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn tick_does_not_mark_edited() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("i1", "First")]).await;
        let approved = approve(&pool, prep.id).await.unwrap();
        let ticked = tick(&pool, prep.id, "i1", true).await.unwrap();
        assert_eq!(Some(&ticked.updated_at), approved.approved_at.as_ref());
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let edited = seed(&pool, vec![item("i1", "Changed")]).await;
        assert!(edited.updated_at > approved.approved_at.unwrap());
    }

    #[tokio::test]
    async fn apply_chat_merges_against_current_items() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("x", "x")]).await;
        tick(&pool, prep.id, "x", true).await.unwrap();
        let (after, updated) = apply_chat(
            &pool,
            prep.id,
            message("user"),
            message("assistant"),
            Some(vec![proposed("x", "question"), proposed("y", "question")]),
        )
        .await
        .unwrap();
        assert!(after.items[0].ticked);
        assert_eq!(after.items.len(), 2);
        assert!(updated);
    }

    #[tokio::test]
    async fn apply_chat_none_keeps_list_and_flags_reply() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("x", "x")]).await;
        let (after, updated) =
            apply_chat(&pool, prep.id, message("user"), message("assistant"), None)
                .await
                .unwrap();
        assert_eq!(after.items, prep.items);
        assert_eq!(after.updated_at, prep.updated_at);
        assert_eq!(after.chat.len(), 2);
        assert!(after.chat.last().unwrap().list_not_updated);
        assert!(!updated);
    }

    #[tokio::test]
    async fn corrupt_items_json_is_an_error_not_an_empty_list() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![]).await;
        sqlx::query("UPDATE meeting_preps SET items_json = '{'")
            .execute(&pool)
            .await
            .unwrap();
        let err = get_by_id(&pool, prep.id).await.unwrap_err();
        assert!(err.starts_with(&format!("Prep data is unreadable (id {}): ", prep.id)));
    }

    #[tokio::test]
    async fn deleting_a_meeting_keeps_its_prep() {
        let pool = test_pool().await;
        linked_meeting(&pool).await;
        let prep = seed(&pool, vec![]).await;
        assert_eq!(for_meeting(&pool, "M").await.unwrap(), Some(prep.clone()));
        assert!(MeetingsRepository::delete_meeting(&pool, "M")
            .await
            .unwrap());
        assert_eq!(
            get(&pool, &prep.event_uid, &prep.event_start)
                .await
                .unwrap(),
            Some(prep)
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meeting_preps")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn delete_removes_only_the_prep() {
        let pool = test_pool().await;
        linked_meeting(&pool).await;
        let prep = seed(&pool, vec![]).await;
        delete(&pool, prep.id).await.unwrap();
        assert!(get_by_id(&pool, prep.id).await.unwrap().is_none());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings WHERE id = 'M'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn merge_matches_each_current_item_only_once() {
        let mut first = item("first", "same");
        first.ticked = true;
        let second = item("second", "same");
        let result = merge_items(
            &[first, second],
            vec![
                proposed("same", "check"),
                proposed("same", "question"),
                proposed("same", "clarify"),
            ],
        );
        assert_eq!(result[0].id, "first");
        assert!(result[0].ticked);
        assert_eq!(result[1].id, "second");
        assert!(!result[1].ticked);
        assert_ne!(result[2].id, "first");
        assert_ne!(result[2].id, "second");
        assert!(!result[2].ticked);
    }

    #[tokio::test]
    async fn save_validates_template_and_item_limits_without_writing() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("x", "original")]).await;
        assert_eq!(
            save(
                &pool,
                "u",
                &prep.event_start,
                "Event",
                "",
                Some("../bad"),
                prep.items.clone()
            )
            .await
            .unwrap_err(),
            "Invalid template"
        );
        assert!(
            seed_invalid_items(&pool, vec![item("x", "x"); MAX_PREP_ITEMS + 1])
                .await
                .is_err()
        );
        assert!(
            seed_invalid_items(&pool, vec![item("x", &"я".repeat(MAX_ITEM_CHARS + 1))])
                .await
                .is_err()
        );
        assert_eq!(get_by_id(&pool, prep.id).await.unwrap(), Some(prep));
    }

    async fn seed_invalid_items(
        pool: &sqlx::SqlitePool,
        items: Vec<PrepItem>,
    ) -> Result<MeetingPrep, String> {
        save(pool, "u", "2026-10-29T12:00:00Z", "Event", "", None, items).await
    }

    #[tokio::test]
    async fn unchanged_save_and_chat_keep_approval_timestamp() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("x", "x")]).await;
        let approved = approve(&pool, prep.id).await.unwrap();
        let saved = save(
            &pool,
            "u",
            &prep.event_start,
            &"я".repeat(MAX_EVENT_TITLE_CHARS + 1),
            "",
            None,
            approved.items.clone(),
        )
        .await
        .unwrap();
        assert_eq!(saved.id, prep.id);
        assert_eq!(saved.created_at, prep.created_at);
        assert_eq!(saved.updated_at, approved.updated_at);
        assert_eq!(saved.event_title.chars().count(), MAX_EVENT_TITLE_CHARS);
        let (after, updated) = apply_chat(
            &pool,
            prep.id,
            message("user"),
            message("assistant"),
            Some(vec![proposed("x", "question")]),
        )
        .await
        .unwrap();
        assert!(updated);
        assert_eq!(after.updated_at, approved.updated_at);
        assert!(!after.chat.last().unwrap().list_not_updated);
        assert_eq!(list_all(&pool).await.unwrap(), vec![after]);
    }

    #[tokio::test]
    async fn unchanged_model_items_keep_provenance_and_edits_mark_user_items() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![]).await;
        let (generated, _) = apply_chat(
            &pool,
            prep.id,
            message("user"),
            message("assistant"),
            Some(vec![proposed("x", "check")]),
        )
        .await
        .unwrap();
        assert!(!generated.items[0].by_user);
        let saved = seed(&pool, generated.items.clone()).await;
        assert!(!saved.items[0].by_user);
        let mut edited = saved.items.clone();
        edited[0].kind = "clarify".into();
        let saved = seed(&pool, edited).await;
        assert!(saved.items[0].by_user);
    }

    #[tokio::test]
    async fn corrupt_chat_json_is_an_error_and_writes_do_not_replace_it() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("x", "x")]).await;
        sqlx::query("UPDATE meeting_preps SET chat_json = '{'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(get(&pool, "u", &prep.event_start).await.is_err());
        assert!(get_by_id(&pool, prep.id).await.is_err());
        assert!(list_all(&pool).await.is_err());
        assert!(
            save(&pool, "u", &prep.event_start, "Event", "", None, vec![])
                .await
                .is_err()
        );
        assert!(
            apply_chat(&pool, prep.id, message("user"), message("assistant"), None)
                .await
                .is_err()
        );
        let raw: (String, String) =
            sqlx::query_as("SELECT chat_json, items_json FROM meeting_preps WHERE id = ?")
                .bind(prep.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(raw.0, "{");
        assert_eq!(
            serde_json::from_str::<Vec<PrepItem>>(&raw.1).unwrap(),
            prep.items
        );
    }

    #[tokio::test]
    async fn unknown_item_does_not_write() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("x", "x")]).await;
        assert_eq!(
            tick(&pool, prep.id, "missing", true).await.unwrap_err(),
            "Item not found"
        );
        assert_eq!(get_by_id(&pool, prep.id).await.unwrap(), Some(prep));
    }

    #[tokio::test]
    async fn saved_trailing_space_item_keeps_its_tick_on_chat_merge() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("i1", "X ")]).await;
        assert_eq!(prep.items[0].text, "X");
        tick(&pool, prep.id, "i1", true).await.unwrap();
        let (after, _) = apply_chat(
            &pool,
            prep.id,
            message("user"),
            message("assistant"),
            Some(vec![proposed("X", "question")]),
        )
        .await
        .unwrap();
        assert_eq!(after.items.len(), 1);
        assert_eq!(after.items[0].id, "i1");
        assert!(after.items[0].ticked);
    }

    #[test]
    fn merge_matches_stored_text_with_trailing_space_and_long_proposal() {
        let long = "я".repeat(MAX_ITEM_CHARS);
        let mut legacy = item("a", "X ");
        legacy.ticked = true;
        let mut stored_long = item("b", &long);
        stored_long.ticked = true;
        let merged = merge_items(
            &[legacy, stored_long],
            vec![
                proposed("X", "check"),
                proposed(&format!("{long}я"), "check"),
            ],
        );
        assert_eq!(merged[0].id, "a");
        assert!(merged[0].ticked);
        assert_eq!(merged[1].id, "b");
        assert!(merged[1].ticked);
    }

    #[tokio::test]
    async fn save_rejects_blank_item_and_trims_text() {
        let pool = test_pool().await;
        assert_eq!(
            seed_invalid_items(&pool, vec![item("", "   ")])
                .await
                .unwrap_err(),
            "Item text is empty"
        );
        let saved = seed(&pool, vec![item("", "  Padded  ")]).await;
        assert_eq!(saved.items[0].text, "Padded");
    }

    #[tokio::test]
    async fn save_maps_unknown_kind_and_gives_repeated_id_a_fresh_one() {
        let pool = test_pool().await;
        let first = seed(&pool, vec![item("d", "A")]).await;
        tick(&pool, first.id, "d", true).await.unwrap();
        let mut bogus = item("", "New");
        bogus.kind = "bogus".into();
        let saved = seed(&pool, vec![item("d", "A"), item("d", "B"), bogus]).await;
        assert_eq!(saved.items[0].id, "d");
        assert!(saved.items[0].ticked);
        assert_ne!(saved.items[1].id, "d");
        assert!(!saved.items[1].ticked);
        assert_eq!(saved.items[2].kind, "other");
        let ids: std::collections::HashSet<_> = saved.items.iter().map(|i| i.id.clone()).collect();
        assert_eq!(ids.len(), 3);
    }

    #[tokio::test]
    async fn apply_chat_empty_proposal_keeps_list() {
        let pool = test_pool().await;
        let prep = seed(&pool, vec![item("x", "x")]).await;
        let (after, updated) = apply_chat(
            &pool,
            prep.id,
            message("user"),
            message("assistant"),
            Some(vec![]),
        )
        .await
        .unwrap();
        assert_eq!(after.items, prep.items);
        assert!(after.chat.last().unwrap().list_not_updated);
        assert!(!updated);
    }
}
