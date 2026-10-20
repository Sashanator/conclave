//! Speaking Practice storage: setups, their materials and plans, sessions and turns.

use crate::practice::MATERIAL_MAX_BYTES;
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct SetupRow {
    pub id: String,
    pub name: String,
    pub character: String,
    pub situation: String,
    pub voice: String,
    pub seeded: bool,
    pub plan: Option<String>,
    pub plan_fingerprint: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaterialMeta {
    pub id: String,
    pub setup_id: String,
    pub kind: MaterialKind,
    pub name: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialKind {
    Pdf,
    Text,
}

#[derive(Debug, Clone)]
pub struct Material {
    pub meta: MaterialMeta,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct SetupInput {
    pub id: Option<String>,
    pub name: String,
    pub character: String,
    pub situation: String,
    pub voice: String,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct SessionRow {
    pub id: String,
    pub setup_id: Option<String>,
    pub setup_name: String,
    pub character: String,
    pub voice: String,
    pub started_at: String,
    pub duration_secs: f64,
    pub folder_path: Option<String>,
    pub plan: Option<String>,
    pub status: String,
    pub end_reason: Option<String>,
    pub report_json: Option<String>,
    pub report_status: String,
    pub report_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct TurnRow {
    pub session_id: String,
    pub idx: i64,
    pub speaker: String,
    pub live_text: String,
    pub verbatim_text: Option<String>,
    pub at_secs: f64,
    pub wav_offset: Option<i64>,
    pub wav_len: Option<i64>,
}

pub const VOICES: [(&str, &str); 10] = [
    ("M1", "Male voice 1"),
    ("M2", "Male voice 2"),
    ("M3", "Male voice 3"),
    ("M4", "Male voice 4"),
    ("M5", "Male voice 5"),
    ("F1", "Female voice 1"),
    ("F2", "Female voice 2"),
    ("F3", "Female voice 3"),
    ("F4", "Female voice 4"),
    ("F5", "Female voice 5"),
];

const NAME_MAX_CHARS: usize = 200;
const SITUATION_MAX_CHARS: usize = 4000;

fn db_err(e: sqlx::Error) -> String {
    e.to_string()
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

impl MaterialKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Text => "text",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "pdf" => Ok(Self::Pdf),
            "text" => Ok(Self::Text),
            other => Err(format!("Unknown material kind: {other}")),
        }
    }
}

#[derive(sqlx::FromRow)]
struct MaterialRow {
    id: String,
    setup_id: String,
    kind: String,
    name: String,
    bytes: i64,
}

impl MaterialRow {
    fn into_meta(self) -> Result<MaterialMeta, String> {
        Ok(MaterialMeta {
            id: self.id,
            setup_id: self.setup_id,
            kind: MaterialKind::parse(&self.kind)?,
            name: self.name,
            bytes: self.bytes,
        })
    }
}

#[derive(sqlx::FromRow)]
struct MaterialWithContent {
    #[sqlx(flatten)]
    meta: MaterialRow,
    content: Vec<u8>,
}

pub async fn list_setups(pool: &SqlitePool) -> Result<Vec<SetupRow>, String> {
    sqlx::query_as("SELECT * FROM practice_setups ORDER BY updated_at DESC")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn get_setup(pool: &SqlitePool, id: &str) -> Result<Option<SetupRow>, String> {
    sqlx::query_as("SELECT * FROM practice_setups WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn save_setup(pool: &SqlitePool, input: SetupInput) -> Result<SetupRow, String> {
    let name = input.name.trim().to_string();
    let character = input.character.trim().to_string();
    let situation = input.situation.trim().to_string();
    if name.is_empty() || name.chars().count() > NAME_MAX_CHARS {
        return Err(format!("Name must be 1 to {NAME_MAX_CHARS} characters"));
    }
    if character.is_empty() || character.chars().count() > NAME_MAX_CHARS {
        return Err(format!(
            "Character must be 1 to {NAME_MAX_CHARS} characters"
        ));
    }
    if situation.chars().count() > SITUATION_MAX_CHARS {
        return Err(format!(
            "Situation must be at most {SITUATION_MAX_CHARS} characters"
        ));
    }
    if !VOICES.iter().any(|(code, _)| *code == input.voice) {
        return Err(format!("Unknown voice: {}", input.voice));
    }
    let now = now();
    let id = match input.id {
        Some(id) => {
            let done = sqlx::query(
                "UPDATE practice_setups SET name = ?, character = ?, situation = ?, voice = ?, updated_at = ? WHERE id = ?",
            )
            .bind(&name)
            .bind(&character)
            .bind(&situation)
            .bind(&input.voice)
            .bind(&now)
            .bind(&id)
            .execute(pool)
            .await
            .map_err(db_err)?;
            if done.rows_affected() == 0 {
                return Err("Setup not found".to_string());
            }
            id
        }
        None => {
            let id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO practice_setups (id, name, character, situation, voice, seeded, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
            )
            .bind(&id)
            .bind(&name)
            .bind(&character)
            .bind(&situation)
            .bind(&input.voice)
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await
            .map_err(db_err)?;
            id
        }
    };
    get_setup(pool, &id)
        .await?
        .ok_or_else(|| "Setup not found".to_string())
}

/// Sessions keep their snapshot: `practice_sessions.setup_id` becomes NULL.
pub async fn delete_setup(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM practice_setups WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn list_materials(
    pool: &SqlitePool,
    setup_id: &str,
) -> Result<Vec<MaterialMeta>, String> {
    let rows: Vec<MaterialRow> = sqlx::query_as(
        "SELECT id, setup_id, kind, name, bytes FROM practice_materials WHERE setup_id = ? ORDER BY created_at, id",
    )
    .bind(setup_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    rows.into_iter().map(MaterialRow::into_meta).collect()
}

pub async fn load_materials(pool: &SqlitePool, setup_id: &str) -> Result<Vec<Material>, String> {
    let rows: Vec<MaterialWithContent> = sqlx::query_as(
        "SELECT id, setup_id, kind, name, bytes, content FROM practice_materials WHERE setup_id = ? ORDER BY created_at, id",
    )
    .bind(setup_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    rows.into_iter()
        .map(|r| {
            Ok(Material {
                meta: r.meta.into_meta()?,
                content: r.content,
            })
        })
        .collect()
}

/// The size check and the insert share one transaction, so two adds cannot both pass the limit.
pub async fn add_material(
    pool: &SqlitePool,
    setup_id: &str,
    kind: MaterialKind,
    name: &str,
    content: Vec<u8>,
) -> Result<MaterialMeta, String> {
    let bytes = content.len() as i64;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let used: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(bytes), 0) FROM practice_materials WHERE setup_id = ?",
    )
    .bind(setup_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if used + bytes > MATERIAL_MAX_BYTES {
        return Err("Material is limited to 20 MB per setup".to_string());
    }
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO practice_materials (id, setup_id, kind, name, content, bytes, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(setup_id)
    .bind(kind.as_str())
    .bind(name)
    .bind(content)
    .bind(bytes)
    .bind(now())
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(MaterialMeta {
        id,
        setup_id: setup_id.to_string(),
        kind,
        name: name.to_string(),
        bytes,
    })
}

pub async fn delete_material(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM practice_materials WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Hashes the situation and each material's (id, content), in id order, so the plan is stale
/// whenever either changes.
pub fn plan_fingerprint(situation: &str, materials: &[Material]) -> String {
    let mut sorted: Vec<&Material> = materials.iter().collect();
    sorted.sort_by(|a, b| a.meta.id.cmp(&b.meta.id));
    let mut hasher = Sha256::new();
    hasher.update(situation.as_bytes());
    hasher.update([0]);
    for m in sorted {
        hasher.update(m.meta.id.as_bytes());
        hasher.update([0]);
        hasher.update(&m.content);
    }
    format!("{:x}", hasher.finalize())
}

pub async fn set_plan(
    pool: &SqlitePool,
    setup_id: &str,
    plan: Option<&str>,
    fingerprint: Option<&str>,
) -> Result<(), String> {
    sqlx::query("UPDATE practice_setups SET plan = ?, plan_fingerprint = ? WHERE id = ?")
        .bind(plan)
        .bind(fingerprint)
        .bind(setup_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// The session stores a snapshot of the setup, so it still reads correctly after the setup is edited or deleted.
pub async fn create_session(
    pool: &SqlitePool,
    setup: &SetupRow,
    folder_path: &str,
    plan: Option<&str>,
) -> Result<SessionRow, String> {
    let row = SessionRow {
        id: Uuid::new_v4().to_string(),
        setup_id: Some(setup.id.clone()),
        setup_name: setup.name.clone(),
        character: setup.character.clone(),
        voice: setup.voice.clone(),
        started_at: now(),
        duration_secs: 0.0,
        folder_path: Some(folder_path.to_string()),
        plan: plan.map(str::to_string),
        status: "running".to_string(),
        end_reason: None,
        report_json: None,
        report_status: "pending".to_string(),
        report_error: None,
    };
    sqlx::query(
        "INSERT INTO practice_sessions (id, setup_id, setup_name, character, voice, started_at, duration_secs, folder_path, plan, status, report_status) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.setup_id)
    .bind(&row.setup_name)
    .bind(&row.character)
    .bind(&row.voice)
    .bind(&row.started_at)
    .bind(row.duration_secs)
    .bind(&row.folder_path)
    .bind(&row.plan)
    .bind(&row.status)
    .bind(&row.report_status)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(row)
}

pub async fn add_turn(pool: &SqlitePool, turn: &TurnRow) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO practice_turns (session_id, idx, speaker, live_text, verbatim_text, at_secs, wav_offset, wav_len) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&turn.session_id)
    .bind(turn.idx)
    .bind(&turn.speaker)
    .bind(&turn.live_text)
    .bind(turn.verbatim_text.as_deref())
    .bind(turn.at_secs)
    .bind(turn.wav_offset)
    .bind(turn.wav_len)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn end_session(
    pool: &SqlitePool,
    id: &str,
    duration_secs: f64,
    reason: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE practice_sessions SET status = 'ended', duration_secs = ?, end_reason = ? WHERE id = ?")
        .bind(duration_secs)
        .bind(reason)
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn set_report(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    json: Option<&str>,
    error: Option<&str>,
) -> Result<(), String> {
    sqlx::query("UPDATE practice_sessions SET report_status = ?, report_json = ?, report_error = ? WHERE id = ?")
        .bind(status)
        .bind(json)
        .bind(error)
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn set_verbatim(
    pool: &SqlitePool,
    session_id: &str,
    idx: i64,
    text: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE practice_turns SET verbatim_text = ? WHERE session_id = ? AND idx = ?")
        .bind(text)
        .bind(session_id)
        .bind(idx)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn list_sessions(pool: &SqlitePool) -> Result<Vec<SessionRow>, String> {
    sqlx::query_as("SELECT * FROM practice_sessions ORDER BY started_at DESC")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn get_session(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<(SessionRow, Vec<TurnRow>)>, String> {
    let Some(session) =
        sqlx::query_as::<_, SessionRow>("SELECT * FROM practice_sessions WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
    else {
        return Ok(None);
    };
    let turns = sqlx::query_as("SELECT * FROM practice_turns WHERE session_id = ? ORDER BY idx")
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok(Some((session, turns)))
}

/// Removes the session's rows only; its turns go with it through the foreign key.
pub async fn delete_session_rows(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM practice_sessions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Run at startup: a session or report left running by a crash can never finish.
pub async fn recover_interrupted(pool: &SqlitePool) -> Result<u32, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let ended = sqlx::query("UPDATE practice_sessions SET status = 'ended', end_reason = 'interrupted' WHERE status = 'running'")
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
    let failed = sqlx::query(
        "UPDATE practice_sessions SET report_status = 'failed', report_error = 'Interrupted — Retry' WHERE report_status IN ('running', 'waiting')",
    )
    .execute(&mut *tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    tx.commit().await.map_err(db_err)?;
    Ok((ended + failed) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::{hold_model, holds_model};

    const MB: usize = 1024 * 1024;

    async fn test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn input(id: Option<&str>, name: &str) -> SetupInput {
        SetupInput {
            id: id.map(str::to_string),
            name: name.to_string(),
            character: "Mike, a friendly car mechanic".to_string(),
            situation: "My car makes a strange noise.".to_string(),
            voice: "M3".to_string(),
        }
    }

    async fn new_setup(pool: &sqlx::SqlitePool, name: &str) -> SetupRow {
        save_setup(pool, input(None, name)).await.unwrap()
    }

    #[tokio::test]
    async fn seeds_are_three_editable_setups() {
        let pool = test_pool().await;
        assert_eq!(list_setups(&pool).await.unwrap().len(), 3);
        for (id, voice) in [
            ("seed-interview", "F2"),
            ("seed-thesis", "M1"),
            ("seed-casual", "M3"),
        ] {
            let setup = get_setup(&pool, id).await.unwrap().expect(id);
            assert_eq!(setup.voice, voice);
            assert!(setup.seeded);
        }
        let edited = save_setup(&pool, input(Some("seed-casual"), "Casual chat v2"))
            .await
            .unwrap();
        assert_eq!(edited.name, "Casual chat v2");
        assert!(edited.seeded);
        delete_setup(&pool, "seed-thesis").await.unwrap();
        assert!(get_setup(&pool, "seed-thesis").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_setup_validates_and_updates() {
        let pool = test_pool().await;
        assert!(save_setup(&pool, input(None, "  ")).await.is_err());
        assert!(save_setup(&pool, input(None, &"n".repeat(201)))
            .await
            .is_err());
        let mut long = input(None, "ok");
        long.situation = "s".repeat(4001);
        assert!(save_setup(&pool, long).await.is_err());
        let mut bad_voice = input(None, "ok");
        bad_voice.voice = "X9".to_string();
        assert!(save_setup(&pool, bad_voice).await.is_err());
        assert!(save_setup(&pool, input(Some("missing"), "ok"))
            .await
            .is_err());

        let created = new_setup(&pool, "Dentist").await;
        assert!(uuid::Uuid::parse_str(&created.id).is_ok());
        assert!(!created.seeded);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let updated = save_setup(&pool, input(Some(&created.id), "Dentist 2"))
            .await
            .unwrap();
        assert_eq!(updated.created_at, created.created_at);
        assert!(updated.updated_at > created.updated_at);
    }

    #[tokio::test]
    async fn material_limit_is_per_setup() {
        let pool = test_pool().await;
        let a = new_setup(&pool, "A").await;
        let b = new_setup(&pool, "B").await;
        add_material(&pool, &a.id, MaterialKind::Pdf, "a.pdf", vec![0; 19 * MB])
            .await
            .unwrap();
        let err = add_material(&pool, &a.id, MaterialKind::Pdf, "b.pdf", vec![0; 2 * MB])
            .await
            .unwrap_err();
        assert_eq!(err, "Material is limited to 20 MB per setup");
        add_material(&pool, &b.id, MaterialKind::Text, "b.txt", vec![0; 2 * MB])
            .await
            .unwrap();
        assert_eq!(list_materials(&pool, &a.id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deleting_a_setup_keeps_its_sessions() {
        let pool = test_pool().await;
        let setup = new_setup(&pool, "Interview").await;
        add_material(
            &pool,
            &setup.id,
            MaterialKind::Text,
            "cv.txt",
            b"cv".to_vec(),
        )
        .await
        .unwrap();
        let session = create_session(&pool, &setup, "C:/Conclave/practice/x", None)
            .await
            .unwrap();
        delete_setup(&pool, &setup.id).await.unwrap();
        let (kept, _) = get_session(&pool, &session.id).await.unwrap().unwrap();
        assert_eq!(kept.setup_id, None);
        assert_eq!(kept.setup_name, "Interview");
        assert!(list_materials(&pool, &setup.id).await.unwrap().is_empty());
    }

    #[test]
    fn fingerprint_changes_with_situation_or_material() {
        let meta = |id: &str| MaterialMeta {
            id: id.to_string(),
            setup_id: "s".to_string(),
            kind: MaterialKind::Text,
            name: "n".to_string(),
            bytes: 3,
        };
        let m1 = Material {
            meta: meta("m1"),
            content: b"abc".to_vec(),
        };
        let m2 = Material {
            meta: meta("m2"),
            content: b"def".to_vec(),
        };
        let base = plan_fingerprint("situation", &[m1.clone()]);
        assert_eq!(base, plan_fingerprint("situation", &[m1.clone()]));
        assert_eq!(base.len(), 64);
        assert_ne!(base, plan_fingerprint("situation 2", &[m1.clone()]));
        let both = plan_fingerprint("situation", &[m1.clone(), m2.clone()]);
        assert_ne!(base, both);
        assert_eq!(both, plan_fingerprint("situation", &[m2, m1]));
    }

    #[tokio::test]
    async fn deleting_a_session_removes_its_turns() {
        let pool = test_pool().await;
        let setup = new_setup(&pool, "Chat").await;
        let session = create_session(&pool, &setup, "C:/x", None).await.unwrap();
        for idx in 0..2 {
            let turn = TurnRow {
                session_id: session.id.clone(),
                idx,
                speaker: "user".to_string(),
                live_text: "hello".to_string(),
                verbatim_text: None,
                at_secs: 1.0,
                wav_offset: None,
                wav_len: None,
            };
            add_turn(&pool, &turn).await.unwrap();
        }
        delete_session_rows(&pool, &session.id).await.unwrap();
        assert!(get_session(&pool, &session.id).await.unwrap().is_none());
        let turns: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM practice_turns")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(turns, 0);
    }

    #[tokio::test]
    async fn recover_interrupted_ends_running_sessions_and_fails_running_reports() {
        let pool = test_pool().await;
        let setup = new_setup(&pool, "Chat").await;
        let running = create_session(&pool, &setup, "C:/a", None).await.unwrap();
        let waiting = create_session(&pool, &setup, "C:/b", None).await.unwrap();
        end_session(&pool, &waiting.id, 60.0, "stopped")
            .await
            .unwrap();
        set_report(&pool, &waiting.id, "waiting", None, None)
            .await
            .unwrap();
        let ready = create_session(&pool, &setup, "C:/c", None).await.unwrap();
        end_session(&pool, &ready.id, 60.0, "stopped")
            .await
            .unwrap();
        set_report(&pool, &ready.id, "ready", Some("{}"), None)
            .await
            .unwrap();

        assert_eq!(recover_interrupted(&pool).await.unwrap(), 2);

        let (run, _) = get_session(&pool, &running.id).await.unwrap().unwrap();
        assert_eq!(run.status, "ended");
        assert_eq!(run.end_reason.as_deref(), Some("interrupted"));
        let (wait, _) = get_session(&pool, &waiting.id).await.unwrap().unwrap();
        assert_eq!(wait.report_status, "failed");
        assert_eq!(wait.report_error.as_deref(), Some("Interrupted — Retry"));
        let (done, _) = get_session(&pool, &ready.id).await.unwrap().unwrap();
        assert_eq!(done.report_status, "ready");
        assert_eq!(done.report_json.as_deref(), Some("{}"));
    }

    #[test]
    fn model_holds_overlap() {
        let first = hold_model();
        let second = hold_model();
        assert!(holds_model());
        drop(first);
        assert!(holds_model());
        drop(second);
        assert!(!holds_model());
    }
}
