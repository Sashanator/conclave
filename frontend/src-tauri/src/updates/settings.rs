//! `updates.json`: whether to check at start-up, and the outcome of the last check.

use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "updates.json";
const CHECK_ON_START: &str = "checkOnStart";
const LAST_CHECK: &str = "lastCheck";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCheck {
    pub at: String,
    pub result: String,
}

pub fn check_on_start_from(v: Option<Value>) -> bool {
    v.and_then(|v| v.as_bool()).unwrap_or(true)
}

pub fn last_check_from(v: Option<Value>) -> Option<LastCheck> {
    serde_json::from_value(v?).ok()
}

/// Only the kind is reported: the error text can quote the store file.
fn store_error_kind(e: &tauri_plugin_store::Error) -> String {
    use tauri_plugin_store::Error;
    match e {
        Error::Io(e) => format!("io {:?}", e.kind()),
        Error::Json(e) => format!("json {:?}", e.classify()),
        _ => "other".to_string(),
    }
}

/// A missing file is the normal first run; only an existing file that fails to load is a problem.
fn store_problem(exists: bool, reload: Result<(), tauri_plugin_store::Error>) -> Option<String> {
    match reload {
        Err(e) if exists => Some(store_error_kind(&e)),
        _ => None,
    }
}

/// Opens the store and reloads it: the plugin drops a broken file silently on open, and a later save would
/// overwrite it. An unreadable file is an error, never defaults (a user who turned the check off must not get one).
fn open<R: Runtime>(app: &AppHandle<R>) -> Result<std::sync::Arc<tauri_plugin_store::Store<R>>, String> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| format!("Couldn't read the update settings ({}).", store_error_kind(&e)))?;
    let exists = app
        .path()
        .resolve(STORE_FILE, BaseDirectory::AppData)
        .is_ok_and(|p| p.exists());
    match store_problem(exists, store.reload()) {
        Some(k) => Err(format!(
            "The update settings file is unreadable ({k}). Delete updates.json in the Conclave data folder to reset it."
        )),
        None => Ok(store),
    }
}

fn read<R: Runtime>(app: &AppHandle<R>, key: &str) -> Result<Option<Value>, String> {
    open(app).map(|s| s.get(key))
}

pub fn check_on_start<R: Runtime>(app: &AppHandle<R>) -> Result<bool, String> {
    read(app, CHECK_ON_START).map(check_on_start_from)
}

pub fn last_check<R: Runtime>(app: &AppHandle<R>) -> Result<Option<LastCheck>, String> {
    read(app, LAST_CHECK).map(last_check_from)
}

fn write<R: Runtime>(app: &AppHandle<R>, key: &str, v: Value) -> Result<(), String> {
    let store = open(app)?;
    store.set(key, v);
    store.save().map_err(|e| e.to_string())
}

pub fn set_check_on_start<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), String> {
    write(app, CHECK_ON_START, Value::Bool(enabled))
}

/// `result` must already be scrubbed. A store failure is logged only: the check itself already reported to the user.
pub fn record_check<R: Runtime>(app: &AppHandle<R>, result: &str) {
    let lc = LastCheck { at: chrono::Utc::now().to_rfc3339(), result: result.to_string() };
    let Ok(v) = serde_json::to_value(&lc) else { return };
    if let Err(e) = write(app, LAST_CHECK, v) {
        warn!("updates: couldn't record the last check: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn store_problem_only_for_an_existing_unreadable_file() {
        use std::io::{Error as IoError, ErrorKind};
        use tauri_plugin_store::Error as StoreError;
        let io = || StoreError::Io(IoError::from(ErrorKind::InvalidData));
        assert_eq!(store_problem(false, Err(io())), None);
        assert_eq!(store_problem(true, Ok(())), None);
        assert_eq!(store_problem(true, Err(io())), Some("io InvalidData".to_string()));
        let json = serde_json::from_str::<u8>("\"x\"").unwrap_err();
        assert_eq!(store_problem(true, Err(StoreError::Json(json))), Some("json Data".to_string()));
    }

    #[test]
    fn check_on_start_defaults_true() {
        assert!(check_on_start_from(None));
        assert!(check_on_start_from(Some(json!("no"))));
        assert!(check_on_start_from(Some(json!(true))));
        assert!(!check_on_start_from(Some(json!(false))));
    }

    #[test]
    fn last_check_round_trips_camel_case() {
        let lc = LastCheck { at: "2026-11-16T10:00:00+00:00".into(), result: "up_to_date".into() };
        let v = serde_json::to_value(&lc).unwrap();
        assert_eq!(v, json!({"at":"2026-11-16T10:00:00+00:00","result":"up_to_date"}));
        assert_eq!(last_check_from(Some(v)), Some(lc));
        assert_eq!(last_check_from(Some(json!({"at":1}))), None);
        assert_eq!(last_check_from(None), None);
    }
}
