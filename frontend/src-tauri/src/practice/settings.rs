// Speaking Practice's model settings. They live in practice.json, not in a shared preferences object, like
// Dictate's in dictate.json. Startup reads them leniently; a save is checked strictly.

use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{LazyLock, Mutex, PoisonError, RwLock};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "practice.json";
const KEY: &str = "settings";
const CHAT_MODELS: [&str; 3] = ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"];
const PLAN_MODELS: [&str; 2] = ["claude-opus-5-5", "claude-sonnet-5-5"];
const EFFORTS: [&str; 3] = ["medium", "high", "xhigh"];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct PracticeSettings {
    pub chat_model: String,
    pub plan_model: String,
    pub plan_effort: String,
}

impl Default for PracticeSettings {
    fn default() -> Self {
        Self {
            chat_model: "claude-haiku-5-5".into(),
            plan_model: "claude-opus-5-5".into(),
            plan_effort: "high".into(),
        }
    }
}

static CACHE: LazyLock<RwLock<PracticeSettings>> =
    LazyLock::new(|| RwLock::new(PracticeSettings::default()));
/// Held across check, store write and cache update, so concurrent saves cannot interleave.
static SAVE_LOCK: Mutex<()> = Mutex::new(());

/// Unknown values fall back to the default, one field at a time.
pub fn sanitize(s: PracticeSettings) -> PracticeSettings {
    let d = PracticeSettings::default();
    PracticeSettings {
        chat_model: keep(s.chat_model, &CHAT_MODELS, d.chat_model),
        plan_model: keep(s.plan_model, &PLAN_MODELS, d.plan_model),
        plan_effort: keep(s.plan_effort, &EFFORTS, d.plan_effort),
    }
}

fn keep(value: String, allowed: &[&str], default: String) -> String {
    if allowed.contains(&value.as_str()) {
        value
    } else {
        default
    }
}

/// Save check: an unknown value is an error, not a silent fallback.
fn checked(s: PracticeSettings) -> Result<PracticeSettings, String> {
    if sanitize(s.clone()) == s {
        Ok(s)
    } else {
        Err("Unknown Practice model or effort".into())
    }
}

/// Startup read. A missing or unreadable entry means the defaults.
fn from_stored(raw: Option<Value>) -> PracticeSettings {
    raw.and_then(|v| serde_json::from_value(v).ok())
        .map_or_else(PracticeSettings::default, sanitize)
}

pub fn current() -> PracticeSettings {
    CACHE.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn set_cache(s: PracticeSettings) {
    *CACHE.write().unwrap_or_else(PoisonError::into_inner) = s;
}

pub fn load_at_startup<R: Runtime>(app: &AppHandle<R>) {
    let raw = match app.store(STORE_FILE) {
        Ok(store) => store.get(KEY),
        Err(e) => {
            warn!("practice: settings store unreadable, using defaults: {e}");
            None
        }
    };
    let s = from_stored(raw);
    info!(
        "practice: conversation {}, plan {} at {}",
        s.chat_model, s.plan_model, s.plan_effort
    );
    set_cache(s);
}

pub fn save<R: Runtime>(
    app: &AppHandle<R>,
    s: PracticeSettings,
) -> Result<PracticeSettings, String> {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let valid = checked(s)?;
    let value = serde_json::to_value(&valid).map_err(|e| e.to_string())?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())?;
    set_cache(valid.clone());
    Ok(valid)
}

#[tauri::command]
pub async fn practice_get_settings() -> PracticeSettings {
    current()
}

#[tauri::command]
pub async fn practice_save_settings<R: Runtime>(
    app: AppHandle<R>,
    settings: PracticeSettings,
) -> Result<PracticeSettings, String> {
    save(&app, settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_is_the_phase_1_models() {
        assert_eq!(
            PracticeSettings::default(),
            PracticeSettings {
                chat_model: "claude-haiku-5-5".into(),
                plan_model: "claude-opus-5-5".into(),
                plan_effort: "high".into(),
            }
        );
    }

    #[test]
    fn sanitize_replaces_unknown_values_field_by_field() {
        let raw = PracticeSettings {
            chat_model: "gpt-4".into(),
            plan_model: "claude-sonnet-5-5".into(),
            plan_effort: "turbo".into(),
        };
        assert_eq!(
            sanitize(raw),
            PracticeSettings {
                chat_model: "claude-haiku-5-5".into(),
                plan_model: "claude-sonnet-5-5".into(),
                plan_effort: "high".into(),
            }
        );
    }

    #[test]
    fn save_rejects_unknown_values() {
        let mut s = PracticeSettings::default();
        s.plan_model = "claude-haiku-5-5".into();
        assert!(checked(s).is_err());

        let mut s = PracticeSettings::default();
        s.plan_effort = "low".into();
        assert!(checked(s).is_err());

        let ok = PracticeSettings {
            chat_model: "claude-opus-5-5".into(),
            plan_model: "claude-sonnet-5-5".into(),
            plan_effort: "xhigh".into(),
        };
        assert_eq!(checked(ok.clone()), Ok(ok));
    }

    #[test]
    fn missing_or_partial_file_uses_defaults() {
        assert_eq!(from_stored(None), PracticeSettings::default());
        assert_eq!(
            from_stored(Some(json!("junk"))),
            PracticeSettings::default()
        );
        assert_eq!(
            from_stored(Some(json!({"chat_model": "claude-sonnet-5-5"}))),
            PracticeSettings {
                chat_model: "claude-sonnet-5-5".into(),
                ..PracticeSettings::default()
            }
        );
    }
}
