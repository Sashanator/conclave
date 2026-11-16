//! One-time verified move of plain-text secrets into the secret store.
//! A value is cleared from its plain home only after the store returns it unchanged,
//! and only if the plain value is still what we copied. Nothing here logs a value.
use std::path::Path;

use anyhow::{anyhow, Result};
use log::{info, warn};
use sqlx::{Row, SqlitePool};

use super::{transcript_name, SecretStore, CALENDAR_URL, CUSTOM_OPENAI_API_KEY};
use crate::knowledge::settings::{calendar_path, read_calendar_at, write_calendar_at};

pub(crate) const MOVE_FAILED_NOTICE: &str =
    "Couldn't move your keys to Windows encryption; they still work. Details in the log.";
const SETTINGS_COLUMNS: [&str; 6] = [
    "openaiApiKey", "anthropicApiKey", "ollamaApiKey", "groqApiKey", "openRouterApiKey", "geminiApiKey",
];
const TRANSCRIPT_COLUMNS: [&str; 5] =
    ["whisperApiKey", "deepgramApiKey", "elevenLabsApiKey", "groqApiKey", "openaiApiKey"];

#[derive(Debug, Default)]
pub(crate) struct MoveReport {
    pub not_moved: Vec<String>,
    pub cleared: usize,
}

impl MoveReport {
    fn record(&mut self, name: &str, r: Result<usize>) {
        match r {
            Ok(n) => self.cleared += n,
            Err(e) => {
                warn!("secrets: {name} not moved, plain-text copy kept: {e}");
                self.not_moved.push(name.to_string());
            }
        }
    }
}

/// Store already has a value -> Ok (it wins). Else set, get, compare; mismatch -> Err.
fn copy_verified(store: &dyn SecretStore, name: &str, plain: &str) -> Result<()> {
    if store.get(name)?.is_some() {
        return Ok(());
    }
    store.set(name, plain)?;
    match store.get(name)? {
        Some(back) if back == plain => Ok(()),
        _ => {
            // Drop the bad copy, or the next start would treat it as the winner and clear the real plain value.
            let msg = match store.delete(name) {
                Ok(()) => String::new(),
                Err(e) => format!("; removing it also failed: {e}"),
            };
            Err(anyhow!("secret {name}: read-back did not match{msg}"))
        }
    }
}

/// Plain column -> store, then compare-and-clear. Returns 1 if cleared.
async fn move_column(
    pool: &SqlitePool,
    store: &dyn SecretStore,
    table: &str,
    col: &str,
    name: &str,
) -> Result<usize> {
    let plain: Option<String> =
        sqlx::query_scalar(&format!("SELECT \"{col}\" FROM {table} WHERE id = '1'"))
            .fetch_optional(pool)
            .await?
            .flatten();
    let Some(plain) = plain.filter(|p| !p.is_empty()) else { return Ok(0) };
    copy_verified(store, name, &plain)?;
    let done = sqlx::query(&format!(
        "UPDATE {table} SET \"{col}\" = NULL WHERE id = '1' AND \"{col}\" = $1"
    ))
    .bind(&plain)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() as usize)
}

async fn move_custom_config(pool: &SqlitePool, store: &dyn SecretStore) -> Result<usize> {
    let json: Option<String> =
        sqlx::query_scalar("SELECT customOpenAIConfig FROM settings WHERE id = '1'")
            .fetch_optional(pool)
            .await?
            .flatten();
    let Some(json) = json.filter(|j| !j.trim().is_empty()) else { return Ok(0) };
    let mut value: serde_json::Value =
        serde_json::from_str(&json).map_err(|_| anyhow!("config is not valid JSON"))?;
    let Some(key) = value.get("apiKey").and_then(|k| k.as_str()).filter(|k| !k.is_empty()) else {
        return Ok(0);
    };
    copy_verified(store, CUSTOM_OPENAI_API_KEY, key)?;
    value["apiKey"] = serde_json::Value::Null;
    let done = sqlx::query(
        "UPDATE settings SET customOpenAIConfig = $1 WHERE id = '1' AND customOpenAIConfig = $2",
    )
    .bind(value.to_string())
    .bind(&json)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() as usize)
}

async fn move_licensing(pool: &SqlitePool, store: &dyn SecretStore, report: &mut MoveReport) {
    let rows = match sqlx::query("SELECT license_key, encrypted_key FROM licensing LIMIT 2")
        .fetch_all(pool)
        .await
    {
        Ok(r) => r,
        Err(e) => return report.record("licensing", Err(e.into())),
    };
    if rows.len() > 1 {
        return report.record("licensing", Err(anyhow!("more than one licensing row")));
    }
    let Some(row) = rows.first() else { return };
    let (Ok(lk), Ok(ek)) = (
        row.try_get::<String, _>("license_key"),
        row.try_get::<String, _>("encrypted_key"),
    ) else {
        return report.record("licensing", Err(anyhow!("unreadable licensing row")));
    };
    let mut ok = true;
    for (name, v) in [("licensing.license_key", &lk), ("licensing.encrypted_key", &ek)] {
        if !v.is_empty() {
            let r = copy_verified(store, name, v).map(|_| 0);
            ok &= r.is_ok();
            report.record(name, r);
        }
    }
    if !ok || (lk.is_empty() && ek.is_empty()) {
        return;
    }
    let r = sqlx::query(
        "UPDATE licensing SET license_key = '', encrypted_key = '' WHERE license_key = $1",
    )
    .bind(&lk)
    .execute(pool)
    .await
    .map(|d| d.rows_affected() as usize)
    .map_err(Into::into);
    report.record("licensing", r);
}

pub(crate) async fn move_plain_secrets(
    pool: &SqlitePool,
    calendar_file: &Path,
    store: &dyn SecretStore,
) -> MoveReport {
    let mut report = MoveReport::default();
    for col in SETTINGS_COLUMNS {
        let r = move_column(pool, store, "settings", col, col).await;
        report.record(col, r);
    }
    for col in TRANSCRIPT_COLUMNS {
        let name = transcript_name(col);
        let r = move_column(pool, store, "transcript_settings", col, &name).await;
        report.record(&name, r);
    }
    let r = move_custom_config(pool, store).await;
    report.record(CUSTOM_OPENAI_API_KEY, r);
    move_licensing(pool, store, &mut report).await;
    let r = match read_calendar_at(calendar_file) {
        Ok(Some(url)) if !url.is_empty() => copy_verified(store, CALENDAR_URL, &url)
            .and_then(|_| write_calendar_at(calendar_file, None).map_err(|e| anyhow!(e)))
            .map(|_| 1),
        Ok(_) => Ok(0),
        Err(e) => Err(anyhow!(e)),
    };
    report.record(CALENDAR_URL, r);
    report
}

/// Checkpoint + VACUUM so cleared values leave the DB file; only when something was cleared.
pub(crate) async fn compact_if_cleared(pool: &SqlitePool, cleared: usize) -> Result<bool> {
    if cleared == 0 {
        return Ok(false);
    }
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(pool).await?;
    sqlx::query("VACUUM").execute(pool).await?;
    // In WAL mode VACUUM lands in the -wal file; fold it into the main file.
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(pool).await?;
    Ok(true)
}

pub async fn run_at_startup(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Some(state) = app.try_state::<crate::state::AppState>() else { return };
    let pool = state.db_manager.pool();
    let mut report = match calendar_path(app) {
        Ok(path) => move_plain_secrets(pool, &path, super::store()).await,
        Err(e) => {
            warn!("secrets: calendar path unavailable, {CALENDAR_URL} not moved: {e}");
            let mut r = move_plain_secrets(pool, Path::new(""), super::store()).await;
            r.not_moved.retain(|n| n != CALENDAR_URL);
            r.not_moved.push(CALENDAR_URL.to_string());
            r
        }
    };
    info!("secrets: move checked, {} not moved, {} cleared", report.not_moved.len(), report.cleared);
    if let Err(e) = compact_if_cleared(pool, report.cleared).await {
        warn!("secrets: compaction failed, cleared values may remain in free pages: {e}");
    }
    if !report.not_moved.is_empty() {
        // The notice can be suppressed by the user's notification settings, so name the secrets in the log too.
        warn!("secrets: not moved to Credential Manager: {}", report.not_moved.join(", "));
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            for _ in 0..60 {
                if let Some(st) = app.try_state::<crate::notifications::commands::NotificationManagerState<tauri::Wry>>() {
                    if st.read().await.is_some() {
                        if let Err(e) = crate::notifications::commands::show_system_error_notification(
                            &st, MOVE_FAILED_NOTICE.into(),
                        ).await {
                            warn!("secrets: move-failed notice not shown: {e}");
                        }
                        return;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            warn!("secrets: notification manager never became ready, move-failed notice not shown");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::manager::DatabaseManager;
    use crate::secrets::{FailingStore, MemoryStore};

    // get() reports empty until set() is called, then always returns "other".
    struct LyingStore(std::sync::Mutex<std::collections::HashSet<String>>);
    impl SecretStore for LyingStore {
        fn get(&self, n: &str) -> Result<Option<String>> {
            Ok(if self.0.lock().unwrap().contains(n) { Some("other".into()) } else { None })
        }
        fn set(&self, n: &str, _: &str) -> Result<()> {
            self.0.lock().unwrap().insert(n.to_string());
            Ok(())
        }
        fn delete(&self, n: &str) -> Result<()> {
            self.0.lock().unwrap().remove(n);
            Ok(())
        }
    }

    const SENTINEL: &str = "sentinel-not-a-real-key";

    async fn db(dir: &Path) -> DatabaseManager {
        let p = |n: &str| dir.join(n).to_string_lossy().to_string();
        DatabaseManager::new(&p("meeting_minutes.sqlite"), &p("meeting_minutes.db")).await.unwrap()
    }

    async fn exec(pool: &SqlitePool, sql: &str) {
        sqlx::query(sql).execute(pool).await.unwrap();
    }

    async fn val(pool: &SqlitePool, sql: &str) -> Option<String> {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    async fn seed(pool: &SqlitePool) {
        exec(pool, "INSERT INTO settings (id, provider, model, whisperModel, openaiApiKey, anthropicApiKey, geminiApiKey, customOpenAIConfig) \
            VALUES ('1','p','m','w','o','a','g','{\"endpoint\":\"http://x/v1\",\"apiKey\":\"c\",\"model\":\"m\"}')").await;
        exec(pool, "INSERT INTO transcript_settings (id, provider, model, groqApiKey, whisperApiKey) VALUES ('1','p','m','tg','tw')").await;
        exec(pool, "INSERT INTO licensing (license_key, encrypted_key, signature_hash, activation_date, expiry_date, soft_expiry_date, max_activation_time, duration, generated_on) \
            VALUES ('lk','ek','s','a','e','se','m',1,'g')").await;
    }

    fn cal(dir: &Path, url: Option<&str>) -> std::path::PathBuf {
        let p = dir.join("cal.json");
        write_calendar_at(&p, url).unwrap();
        p
    }

    #[tokio::test]
    async fn move_success_clears_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let m = db(dir.path()).await;
        let pool = m.pool();
        seed(pool).await;
        let cp = cal(dir.path(), Some("http://cal/x"));
        let store = MemoryStore::default();
        let r = move_plain_secrets(pool, &cp, &store).await;
        assert!(r.not_moved.is_empty(), "{:?}", r.not_moved);
        assert_eq!(r.cleared, 8);
        for (n, v) in [
            ("openaiApiKey", "o"), ("anthropicApiKey", "a"), ("geminiApiKey", "g"),
            (CUSTOM_OPENAI_API_KEY, "c"), ("transcript.groqApiKey", "tg"),
            ("transcript.whisperApiKey", "tw"), ("licensing.license_key", "lk"),
            ("licensing.encrypted_key", "ek"), (CALENDAR_URL, "http://cal/x"),
        ] {
            assert_eq!(store.get(n).unwrap().as_deref(), Some(v), "{n}");
        }
        assert_eq!(val(pool, "SELECT openaiApiKey FROM settings").await, None);
        assert_eq!(val(pool, "SELECT groqApiKey FROM transcript_settings").await, None);
        assert_eq!(val(pool, "SELECT license_key FROM licensing").await.as_deref(), Some(""));
        assert_eq!(val(pool, "SELECT encrypted_key FROM licensing").await.as_deref(), Some(""));
        let cfg: serde_json::Value =
            serde_json::from_str(&val(pool, "SELECT customOpenAIConfig FROM settings").await.unwrap()).unwrap();
        assert_eq!(cfg["endpoint"], "http://x/v1");
        assert!(cfg["apiKey"].is_null());
        assert_eq!(read_calendar_at(&cp).unwrap(), None);
        // second run changes nothing
        let r2 = move_plain_secrets(pool, &cp, &store).await;
        assert!(r2.not_moved.is_empty());
        assert_eq!(r2.cleared, 0);
    }

    #[tokio::test]
    async fn read_back_mismatch_keeps_plain() {
        let dir = tempfile::tempdir().unwrap();
        let m = db(dir.path()).await;
        seed(m.pool()).await;
        let cp = cal(dir.path(), None);
        let store = LyingStore(Default::default());
        let r = move_plain_secrets(m.pool(), &cp, &store).await;
        assert_eq!(r.cleared, 0);
        assert!(r.not_moved.contains(&"openaiApiKey".to_string()));
        assert_eq!(val(m.pool(), "SELECT openaiApiKey FROM settings").await.as_deref(), Some("o"));
        assert_eq!(store.get("openaiApiKey").unwrap(), None);
        // a second start must not mistake the bad copy for the winner
        let r2 = move_plain_secrets(m.pool(), &cp, &store).await;
        assert_eq!(r2.cleared, 0);
        assert_eq!(val(m.pool(), "SELECT openaiApiKey FROM settings").await.as_deref(), Some("o"));
        assert_eq!(store.get("openaiApiKey").unwrap(), None);
    }

    #[tokio::test]
    async fn store_error_keeps_plain_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let m = db(dir.path()).await;
        let pool = m.pool();
        seed(pool).await;
        let cp = cal(dir.path(), Some("http://cal/x"));
        let r = move_plain_secrets(pool, &cp, &FailingStore).await;
        assert_eq!(r.cleared, 0);
        for n in ["openaiApiKey", "anthropicApiKey", "geminiApiKey", "transcript.groqApiKey",
            "transcript.whisperApiKey", CUSTOM_OPENAI_API_KEY, "licensing.license_key",
            "licensing.encrypted_key", CALENDAR_URL] {
            assert!(r.not_moved.contains(&n.to_string()), "{n} missing in {:?}", r.not_moved);
        }
        assert_eq!(val(pool, "SELECT openaiApiKey FROM settings").await.as_deref(), Some("o"));
        assert_eq!(val(pool, "SELECT license_key FROM licensing").await.as_deref(), Some("lk"));
        assert_eq!(read_calendar_at(&cp).unwrap().as_deref(), Some("http://cal/x"));
        assert!(val(pool, "SELECT customOpenAIConfig FROM settings").await.unwrap().contains("\"c\""));
    }

    #[tokio::test]
    async fn existing_store_value_wins() {
        let dir = tempfile::tempdir().unwrap();
        let m = db(dir.path()).await;
        exec(m.pool(), "INSERT INTO settings (id, provider, model, whisperModel, openaiApiKey) VALUES ('1','p','m','w','old')").await;
        let store = MemoryStore::default();
        store.set("openaiApiKey", "new").unwrap();
        let r = move_plain_secrets(m.pool(), &cal(dir.path(), None), &store).await;
        assert!(r.not_moved.is_empty());
        assert_eq!(store.get("openaiApiKey").unwrap().as_deref(), Some("new"));
        assert_eq!(val(m.pool(), "SELECT openaiApiKey FROM settings").await, None);
    }

    #[tokio::test]
    async fn empty_values_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let m = db(dir.path()).await;
        exec(m.pool(), "INSERT INTO settings (id, provider, model, whisperModel, openaiApiKey) VALUES ('1','p','m','w','')").await;
        let store = MemoryStore::default();
        let r = move_plain_secrets(m.pool(), &cal(dir.path(), None), &store).await;
        assert!(r.not_moved.is_empty());
        assert_eq!(r.cleared, 0);
        assert_eq!(store.get("openaiApiKey").unwrap(), None);
    }

    #[tokio::test]
    async fn two_licensing_rows_are_reported_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let m = db(dir.path()).await;
        for k in ["a", "b"] {
            exec(m.pool(), &format!("INSERT INTO licensing (license_key, encrypted_key, signature_hash, activation_date, expiry_date, soft_expiry_date, max_activation_time, duration, generated_on) VALUES ('{k}','e','s','a','e','se','m',1,'g')")).await;
        }
        let store = MemoryStore::default();
        let r = move_plain_secrets(m.pool(), &cal(dir.path(), None), &store).await;
        assert_eq!(r.not_moved, vec!["licensing".to_string()]);
        assert_eq!(r.cleared, 0);
        assert_eq!(store.get("licensing.license_key").unwrap(), None);
    }

    #[tokio::test]
    async fn bad_custom_config_json_is_reported_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let m = db(dir.path()).await;
        exec(m.pool(), "INSERT INTO settings (id, provider, model, whisperModel, customOpenAIConfig) VALUES ('1','p','m','w','{nope')").await;
        let store = MemoryStore::default();
        let r = move_plain_secrets(m.pool(), &cal(dir.path(), None), &store).await;
        assert_eq!(r.not_moved, vec![CUSTOM_OPENAI_API_KEY.to_string()]);
        assert_eq!(val(m.pool(), "SELECT customOpenAIConfig FROM settings").await.as_deref(), Some("{nope"));
    }

    #[tokio::test]
    async fn compaction_runs_only_when_something_was_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let m = db(dir.path()).await;
        assert!(!compact_if_cleared(m.pool(), 0).await.unwrap());
    }

    #[tokio::test]
    async fn cleared_values_leave_the_db_file() {
        let dir = tempfile::tempdir().unwrap();
        let m = db(dir.path()).await;
        exec(m.pool(), &format!("INSERT INTO settings (id, provider, model, whisperModel, anthropicApiKey) VALUES ('1','p','m','w','{SENTINEL}')")).await;
        let store = MemoryStore::default();
        let r = move_plain_secrets(m.pool(), &cal(dir.path(), None), &store).await;
        assert_eq!(r.cleared, 1);
        assert!(compact_if_cleared(m.pool(), r.cleared).await.unwrap());
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let p = entry.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.starts_with("meeting_minutes.sqlite") {
                let bytes = std::fs::read(&p).unwrap();
                assert!(!bytes.windows(SENTINEL.len()).any(|w| w == SENTINEL.as_bytes()), "{name}");
            }
        }
    }
}
