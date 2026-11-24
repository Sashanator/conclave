pub mod client;
pub mod commands;
pub mod fill;
pub mod mapping;
pub mod send;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub id: String,
    pub name: String,
    pub colour: u8,
    pub position: i64,
    pub database_id: String,
    pub data_source_id: Option<String>,
    pub mapping: Mapping,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub last_message: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub title_prop: String,
    pub due_prop: Option<String>,
    pub meeting_prop: Option<String>,
    pub fixed: Vec<FixedValue>,
    #[serde(default)]
    pub emoji: Option<String>,
    #[serde(default = "default_true")]
    pub ai_fill: bool,
}

fn default_true() -> bool {
    true
}

/// `value`: a string for select/status, a string array for multi_select, a bool for checkbox.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FixedValue {
    pub prop: String,
    pub kind: String,
    pub value: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbSchema {
    pub database_id: String,
    pub data_source_id: Option<String>,
    pub title: String,
    pub properties: Vec<PropInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PropInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub options: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DestStatus {
    pub dest_id: String,
    pub state: String,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotionConfig {
    pub token_last4: Option<String>,
    pub destinations: Vec<Destination>,
    pub category_defaults: Vec<CategoryDefault>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDefault {
    pub template_key: String,
    pub dest_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SendReport {
    pub sent: u32,
    pub failed: u32,
    pub ambiguous: u32,
    /// Destination names whose extra columns the AI could not fill (the tasks were still sent).
    #[serde(default)]
    pub ai_not_filled: Vec<String>,
}

/// States of a destination that stop sending to it.
pub(crate) const BLOCKING_STATES: [&str; 5] = ["not_shared", "not_found", "token_missing", "token_rejected", "mapping_broken"];

pub(crate) const MAX_DESTINATIONS: usize = 9;

const DEST_COLS: &str = "id, name, colour, position, database_id, data_source_id, mapping_json, last_status, last_message";

type DestRow = (String, String, i64, i64, String, Option<String>, String, Option<String>, Option<String>);

fn dest_from_row(r: DestRow) -> anyhow::Result<Destination> {
    Ok(Destination {
        id: r.0,
        name: r.1,
        colour: r.2 as u8,
        position: r.3,
        database_id: r.4,
        data_source_id: r.5,
        mapping: serde_json::from_str(&r.6)?,
        last_status: r.7,
        last_message: r.8,
    })
}

pub(crate) async fn list_destinations(pool: &sqlx::SqlitePool) -> anyhow::Result<Vec<Destination>> {
    let rows: Vec<DestRow> =
        sqlx::query_as(&format!("SELECT {DEST_COLS} FROM notion_destinations ORDER BY position")).fetch_all(pool).await?;
    rows.into_iter().map(dest_from_row).collect()
}

/// R4 value shape per kind.
fn fixed_value_ok(f: &FixedValue) -> bool {
    match f.kind.as_str() {
        "select" | "status" => f.value.is_string(),
        "multi_select" => f.value.as_array().is_some_and(|a| a.iter().all(|v| v.is_string())),
        "checkbox" => f.value.is_boolean(),
        "date" => f.value.as_str() == Some("today"),
        _ => false,
    }
}

pub(crate) const BAD_EMOJI: &str = "Use one emoji for the page icon.";

/// Notion answers 400 to a non-emoji icon, which would block the whole database, so check here.
/// shortcut: an allowlist of the Unicode emoji blocks (text symbols such as ★ ☆ ✓ ✗ → ™ № are out), not Notion's real
/// list; a rare unsupported pictograph still gets Notion's error, and the page is then sent without the icon.
fn is_emoji(e: &str) -> bool {
    const RANGES: [(u32, u32); 18] = [
        (0x1F000, 0x1FAFF), // pictographs, flags (regional indicators), skin tones
        (0x2600, 0x27BF),   // misc symbols and dingbats (minus the text symbols below)
        (0x231A, 0x231B), (0x23E9, 0x23F3), (0x23F8, 0x23FA),
        (0x2B05, 0x2B07), (0x2B1B, 0x2B1C), (0x2B50, 0x2B50), (0x2B55, 0x2B55),
        (0x2194, 0x2199), (0x21A9, 0x21AA), (0x25AA, 0x25AB), (0x25FB, 0x25FE),
        (0x00A9, 0x00A9), (0x00AE, 0x00AE), (0x203C, 0x2049), (0x3030, 0x3030),
        (0x3297, 0x3299),
    ];
    const TEXT_SYMBOLS: [char; 4] = ['\u{2605}', '\u{2606}', '\u{2713}', '\u{2717}'];
    let chars: Vec<char> = e.chars().collect();
    chars.iter().enumerate().all(|(i, &c)| match c {
        '\u{200D}' | '\u{FE0F}' | '\u{20E3}' => true,
        // keycap base: 1\u{FE0F}\u{20E3}
        '0'..='9' | '#' | '*' => chars[i + 1..].iter().find(|&&n| n != '\u{FE0F}') == Some(&'\u{20E3}'),
        _ => !TEXT_SYMBOLS.contains(&c) && RANGES.iter().any(|&(a, b)| (a..=b).contains(&(c as u32))),
    })
}

fn validate(d: &Destination) -> Result<(String, String, String), String> {
    let name = d.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("Give the database a name.".into());
    }
    if !(1..=9).contains(&d.colour) {
        return Err("Pick a colour.".into());
    }
    let database_id = mapping::parse_database_id(&d.database_id)?;
    let data_source_id = d
        .data_source_id
        .as_deref()
        .and_then(mapping::parse_uuid)
        .ok_or("Read the database's columns first.")?;
    if d.mapping.title_prop.trim().is_empty() {
        return Err("Choose the task text column.".into());
    }
    if let Some(f) = d.mapping.fixed.iter().find(|f| !fixed_value_ok(f)) {
        return Err(format!("\u{201c}{}\u{201d} has a value Conclave can't send.", f.prop));
    }
    if let Some(f) = d.mapping.fixed.iter().find(|f| Some(&f.prop) == d.mapping.due_prop.as_ref()) {
        return Err(format!("\u{201c}{}\u{201d} is the due date column, so it can't also be fixed.", f.prop));
    }
    if let Some(e) = &d.mapping.emoji {
        if !(1..=8).contains(&e.chars().count()) || !is_emoji(e) {
            return Err(BAD_EMOJI.into());
        }
    }
    Ok((name, database_id, data_source_id))
}

fn db_err(e: sqlx::Error) -> String {
    format!("Could not save: {e}")
}

/// Saves a destination (empty id = new). Status is never taken from the input: a saved mapping is untested again.
pub(crate) async fn save_destination(pool: &sqlx::SqlitePool, dest: Destination) -> Result<Destination, String> {
    let (name, database_id, data_source_id) = validate(&dest)?;
    let mapping_json = serde_json::to_string(&dest.mapping).map_err(|e| e.to_string())?;
    let dup: Option<(String,)> = sqlx::query_as("SELECT id FROM notion_destinations WHERE database_id = ? AND id != ?")
        .bind(&database_id)
        .bind(&dest.id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    if dup.is_some() {
        return Err("That Notion database is already in Settings.".into());
    }
    let id = if dest.id.is_empty() {
        let id = uuid::Uuid::new_v4().to_string();
        // One statement: the cap check and the insert cannot interleave with another save.
        let r = sqlx::query(
            "INSERT INTO notion_destinations (id, name, colour, position, database_id, data_source_id, mapping_json, created_at)
             SELECT ?1, ?2, ?3, COALESCE(MAX(position),0)+1, ?4, ?5, ?6, ?7 FROM notion_destinations
             HAVING COUNT(*) < ?8",
        )
        .bind(&id)
        .bind(&name)
        .bind(dest.colour as i64)
        .bind(&database_id)
        .bind(&data_source_id)
        .bind(&mapping_json)
        .bind(crate::knowledge::tasks::stamp())
        .bind(MAX_DESTINATIONS as i64)
        .execute(pool)
        .await
        .map_err(db_err)?;
        if r.rows_affected() == 0 {
            return Err(format!("Conclave supports up to {MAX_DESTINATIONS} Notion databases."));
        }
        id
    } else {
        let r = sqlx::query(
            "UPDATE notion_destinations SET name=?2, colour=?3, database_id=?4, data_source_id=?5, mapping_json=?6,
             last_status=NULL, last_message=NULL WHERE id=?1",
        )
        .bind(&dest.id)
        .bind(&name)
        .bind(dest.colour as i64)
        .bind(&database_id)
        .bind(&data_source_id)
        .bind(&mapping_json)
        .execute(pool)
        .await
        .map_err(db_err)?;
        if r.rows_affected() == 0 {
            return Err("That Notion database is no longer in Settings.".into());
        }
        dest.id.clone()
    };
    let row: DestRow = sqlx::query_as(&format!("SELECT {DEST_COLS} FROM notion_destinations WHERE id = ?"))
        .bind(&id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    dest_from_row(row).map_err(|e| e.to_string())
}

pub(crate) async fn delete_destination(pool: &sqlx::SqlitePool, id: &str) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM notion_destinations WHERE id = ?").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("UPDATE tasks SET dest_id = NULL WHERE dest_id = ? AND review IN ('open','keep','drop')")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("UPDATE notion_category_defaults SET dest_id = NULL WHERE dest_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

pub(crate) async fn list_category_defaults(pool: &sqlx::SqlitePool) -> anyhow::Result<Vec<CategoryDefault>> {
    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT template_key, dest_id FROM notion_category_defaults ORDER BY template_key").fetch_all(pool).await?;
    Ok(rows.into_iter().map(|(template_key, dest_id)| CategoryDefault { template_key, dest_id }).collect())
}

/// Sets the default for a category ('' = no template); a `Some` also fills its undecided open/keep tasks.
pub(crate) async fn set_category_default(pool: &sqlx::SqlitePool, template_key: &str, dest_id: Option<&str>) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    if let Some(d) = dest_id {
        let found: Option<(String,)> =
            sqlx::query_as("SELECT id FROM notion_destinations WHERE id = ?").bind(d).fetch_optional(&mut *tx).await.map_err(db_err)?;
        if found.is_none() {
            return Err("That Notion database is no longer in Settings.".into());
        }
    }
    sqlx::query(
        "INSERT INTO notion_category_defaults (template_key, dest_id) VALUES (?1, ?2)
         ON CONFLICT(template_key) DO UPDATE SET dest_id = excluded.dest_id",
    )
    .bind(template_key)
    .bind(dest_id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    if let Some(d) = dest_id {
        sqlx::query(
            "UPDATE tasks SET dest_id = ?1 WHERE dest_id IS NULL AND review IN ('open','keep')
             AND meeting_id IN (SELECT id FROM meetings WHERE COALESCE(template_id,'') = ?2)",
        )
        .bind(d)
        .bind(template_key)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)
}

/// Persists the last Test/send outcome of a destination (R1).
pub(crate) async fn set_dest_status(pool: &sqlx::SqlitePool, dest_id: &str, state: &str, message: Option<&str>) -> anyhow::Result<()> {
    sqlx::query("UPDATE notion_destinations SET last_status = ?, last_message = ? WHERE id = ?")
        .bind(state)
        .bind(message)
        .bind(dest_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;

    const DS: &str = "0123456789abcdef0123456789abcdef";
    const DB: &str = "fedcba9876543210fedcba9876543210";

    fn dest(id: &str, name: &str) -> Destination {
        Destination {
            id: id.into(),
            name: name.into(),
            colour: 1,
            position: 0,
            database_id: DB.into(),
            data_source_id: Some(DS.into()),
            mapping: Mapping { title_prop: "Name".into(), due_prop: None, meeting_prop: None, fixed: vec![], emoji: None, ai_fill: true },
            last_status: None,
            last_message: None,
        }
    }

    fn dest_on(name: &str, n: u32) -> Destination {
        Destination { database_id: format!("{n:032x}"), ..dest("", name) }
    }

    #[tokio::test]
    async fn delete_clears_dest_on_removed_tasks_too() {
        let pool = test_pool().await;
        let d = save_destination(&pool, dest("", "Work")).await.unwrap();
        sqlx::query("INSERT INTO tasks (id, meeting_id, text, owner, status, source, created_at, review, dest_id) VALUES ('t', NULL, 'x', 'me', 'dismissed', 'auto', 't', 'drop', ?)")
            .bind(&d.id)
            .execute(&pool)
            .await
            .unwrap();
        delete_destination(&pool, &d.id).await.unwrap();
        let (x,): (Option<String>,) = sqlx::query_as("SELECT dest_id FROM tasks WHERE id = 't'").fetch_one(&pool).await.unwrap();
        assert_eq!(x, None);
    }

    #[tokio::test]
    async fn save_refuses_a_database_already_listed() {
        let pool = test_pool().await;
        let a = save_destination(&pool, dest("", "Work")).await.unwrap();
        assert_eq!(save_destination(&pool, dest("", "Again")).await.unwrap_err(), "That Notion database is already in Settings.");
        let b = save_destination(&pool, dest_on("Other", 7)).await.unwrap();
        let mut e = dest(&b.id, "Other");
        assert!(save_destination(&pool, e.clone()).await.is_err());
        e.database_id = format!("{:032x}", 8);
        save_destination(&pool, e).await.unwrap();
        save_destination(&pool, dest(&a.id, "Work2")).await.unwrap();
    }

    #[tokio::test]
    async fn save_inserts_normalises_and_updates() {
        let pool = test_pool().await;
        let a = save_destination(&pool, dest("", "  Work ")).await.unwrap();
        let b = save_destination(&pool, dest_on("Personal", 1)).await.unwrap();
        assert_eq!(uuid::Uuid::parse_str(&a.id).is_ok(), true);
        assert_eq!((a.name.as_str(), a.position, b.position), ("Work", 1, 2));
        assert_eq!(a.database_id, "fedcba98-7654-3210-fedc-ba9876543210");
        assert_eq!(a.data_source_id.as_deref(), Some("01234567-89ab-cdef-0123-456789abcdef"));
        let mut e = a.clone();
        e.name = "Office".into();
        let e = save_destination(&pool, e).await.unwrap();
        assert_eq!((e.name.as_str(), e.position), ("Office", 1));
        let names: Vec<_> = list_destinations(&pool).await.unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["Office", "Personal"]);
        assert_eq!(
            save_destination(&pool, Destination { id: "nope".into(), ..dest_on("X", 55) }).await.unwrap_err(),
            "That Notion database is no longer in Settings."
        );
    }

    #[tokio::test]
    async fn save_resets_status_and_ignores_incoming_status() {
        let pool = test_pool().await;
        let a = save_destination(&pool, dest("", "Work")).await.unwrap();
        set_dest_status(&pool, &a.id, "mapping_broken", Some("x")).await.unwrap();
        let l = &list_destinations(&pool).await.unwrap()[0];
        assert_eq!((l.last_status.as_deref(), l.last_message.as_deref()), (Some("mapping_broken"), Some("x")));
        let mut e = a.clone();
        e.last_status = Some("connected".into());
        let e = save_destination(&pool, e).await.unwrap();
        assert_eq!((e.last_status, e.last_message), (None, None));
    }

    #[tokio::test]
    async fn save_refuses_bad_input_and_a_tenth() {
        let pool = test_pool().await;
        let mut d = dest("", "Work");
        d.colour = 10;
        assert_eq!(save_destination(&pool, d).await.unwrap_err(), "Pick a colour.");
        assert_eq!(save_destination(&pool, dest("", "  ")).await.unwrap_err(), "Give the database a name.");
        let mut d = dest("", "Work");
        d.data_source_id = None;
        assert_eq!(save_destination(&pool, d).await.unwrap_err(), "Read the database's columns first.");
        let mut d = dest("", "Work");
        d.mapping.fixed = vec![FixedValue { prop: "Done".into(), kind: "checkbox".into(), value: "yes".into() }];
        assert!(save_destination(&pool, d).await.unwrap_err().contains("has a value Conclave can't send."));
        let mut d = dest("", "Work");
        d.mapping.due_prop = Some("Due".into());
        d.mapping.fixed = vec![FixedValue { prop: "Due".into(), kind: "date".into(), value: "today".into() }];
        assert!(save_destination(&pool, d).await.unwrap_err().contains("due date column"));
        let mut d = dest("", "Work");
        d.mapping.fixed = vec![FixedValue { prop: "Sent".into(), kind: "date".into(), value: "today".into() }];
        save_destination(&pool, d).await.unwrap();
        for i in 0..8 {
            save_destination(&pool, dest_on(&format!("D{i}"), i + 1)).await.unwrap();
        }
        assert_eq!(
            save_destination(&pool, dest_on("Tenth", 99)).await.unwrap_err(),
            "Conclave supports up to 9 Notion databases."
        );
        assert_eq!(list_destinations(&pool).await.unwrap().len(), 9);
    }

    #[test]
    fn old_mapping_json_defaults_ai_fill_on() {
        let m: Mapping = serde_json::from_str(r#"{"titleProp":"Name","dueProp":null,"meetingProp":null,"fixed":[]}"#).unwrap();
        assert!(m.ai_fill);
        assert_eq!(m.emoji, None);
        let m = Mapping { emoji: Some("👷".into()), ai_fill: false, ..m };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""emoji":"👷""#) && j.contains(r#""aiFill":false"#), "{j}");
    }

    #[tokio::test]
    async fn emoji_is_validated_and_saved() {
        let pool = test_pool().await;
        let with = |n: u32, e: Option<&str>| {
            let mut d = dest_on(&format!("E{n}"), n);
            d.mapping.emoji = e.map(Into::into);
            d
        };
        save_destination(&pool, with(1, Some("👷"))).await.unwrap();
        // ZWJ sequence: person + ZWJ + male sign + variation selector 16
        save_destination(&pool, with(2, Some("\u{1F477}\u{200D}\u{2642}\u{FE0F}"))).await.unwrap();
        save_destination(&pool, with(3, None)).await.unwrap();
        // flag, keycap, plain emoji with VS16
        for (n, e) in [(16, "\u{1F1E9}\u{1F1EA}"), (17, "1\u{FE0F}\u{20E3}"), (18, "\u{2764}\u{FE0F}"), (19, "\u{2B50}")] {
            save_destination(&pool, with(n, Some(e))).await.unwrap_or_else(|e| panic!("{n}: {e}"));
        }
        let emojis: Vec<_> = list_destinations(&pool).await.unwrap().into_iter().map(|d| d.mapping.emoji).take(3).collect();
        assert_eq!(emojis, [Some("👷".into()), Some("\u{1F477}\u{200D}\u{2642}\u{FE0F}".into()), None]);
        for (n, e) in [(4, "ab"), (5, "é"), (6, "ж"), (7, ""), (8, " 👷"), (9, "👷👷👷👷👷👷👷👷👷"), (10, "★"), (11, "→"), (12, "™"), (13, "№"), (14, "1"), (15, "✓")] {
            assert_eq!(save_destination(&pool, with(n, Some(e))).await.unwrap_err(), BAD_EMOJI, "{e:?}");
        }
    }

    #[tokio::test]
    async fn category_default_fills_undecided_tasks_and_delete_clears_references() {
        let pool = test_pool().await;
        for (m, t) in [("m1", Some("standup")), ("m2", None)] {
            sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, template_id) VALUES (?, 'T', 't', 't', ?)")
                .bind(m)
                .bind(t)
                .execute(&pool)
                .await
                .unwrap();
        }
        for (id, review, m) in [("t1", "open", "m1"), ("t2", "keep", "m1"), ("t3", "sent", "m1"), ("t4", "open", "m2")] {
            sqlx::query("INSERT INTO tasks (id, meeting_id, text, owner, status, source, created_at, review) VALUES (?, ?, 'x', 'me', 'open', 'auto', 't', ?)")
                .bind(id)
                .bind(m)
                .bind(review)
                .execute(&pool)
                .await
                .unwrap();
        }
        let d = save_destination(&pool, dest("", "Work")).await.unwrap();
        assert!(set_category_default(&pool, "standup", Some("nope")).await.is_err());
        set_category_default(&pool, "standup", Some(&d.id)).await.unwrap();
        let dests = || async {
            sqlx::query_as::<_, (String, Option<String>)>("SELECT id, dest_id FROM tasks ORDER BY id").fetch_all(&pool).await.unwrap()
        };
        let want = Some(d.id.clone());
        assert_eq!(dests().await, [("t1".into(), want.clone()), ("t2".into(), want), ("t3".into(), None), ("t4".into(), None)]);
        assert_eq!(
            list_category_defaults(&pool).await.unwrap(),
            [CategoryDefault { template_key: "standup".into(), dest_id: Some(d.id.clone()) }]
        );
        delete_destination(&pool, &d.id).await.unwrap();
        assert!(dests().await.iter().all(|(_, x)| x.is_none()));
        assert_eq!(list_category_defaults(&pool).await.unwrap()[0].dest_id, None);
        assert!(list_destinations(&pool).await.unwrap().is_empty());
    }
}
