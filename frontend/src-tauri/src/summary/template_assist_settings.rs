// The template assistant's model. Unset means the summary model. It lives in template_assist.json, like the
// other feature settings. Startup reads it leniently; a save is checked strictly.

use crate::copilot::settings::PROVIDERS;
use crate::summary::llm_target::{target_with_override, LlmTarget};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;
use std::sync::{LazyLock, Mutex, PoisonError, RwLock};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "template_assist.json";
const KEY: &str = "settings";

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct TemplateAssistSettings {
    /// None means the summary provider and model. Both are set, or neither.
    pub provider: Option<String>,
    pub model: Option<String>,
    pub include_transcript: bool,
    pub hide_edit_hint: bool,
}

static CACHE: LazyLock<RwLock<TemplateAssistSettings>> =
    LazyLock::new(|| RwLock::new(TemplateAssistSettings::default()));
/// Held across check, store write and cache update, so concurrent saves cannot interleave.
static SAVE_LOCK: Mutex<()> = Mutex::new(());

fn non_blank(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// Trims, and keeps the provider and model only when both are set and the provider is supported.
pub fn sanitize(s: TemplateAssistSettings) -> TemplateAssistSettings {
    let (provider, model) = match (non_blank(&s.provider), non_blank(&s.model)) {
        (Some(p), Some(m)) if PROVIDERS.contains(&p.as_str()) => (Some(p), Some(m)),
        _ => (None, None),
    };
    TemplateAssistSettings {
        provider,
        model,
        include_transcript: s.include_transcript,
        hide_edit_hint: s.hide_edit_hint,
    }
}

/// Save check: a non-blank provider or model that sanitize would drop is an error, not a silent fallback.
fn checked(s: TemplateAssistSettings) -> Result<TemplateAssistSettings, String> {
    let clean = sanitize(s.clone());
    let dropped = (non_blank(&s.provider).is_some() && clean.provider.is_none())
        || (non_blank(&s.model).is_some() && clean.model.is_none());
    if dropped {
        Err("Choose a supported provider and a model, or neither".into())
    } else {
        Ok(clean)
    }
}

/// Startup read. A missing or unreadable entry means the defaults.
fn from_stored(raw: Option<Value>) -> TemplateAssistSettings {
    raw.and_then(|v| serde_json::from_value(v).ok())
        .map_or_else(TemplateAssistSettings::default, sanitize)
}

pub fn current() -> TemplateAssistSettings {
    CACHE.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn set_cache(s: TemplateAssistSettings) {
    *CACHE.write().unwrap_or_else(PoisonError::into_inner) = s;
}

pub fn load_at_startup<R: Runtime>(app: &AppHandle<R>) {
    let raw = match app.store(STORE_FILE) {
        Ok(store) => store.get(KEY),
        Err(e) => {
            warn!("template_assist: settings store unreadable, using defaults: {e}");
            None
        }
    };
    let s = from_stored(raw);
    info!(
        "template_assist: provider {}, model {}",
        s.provider.as_deref().unwrap_or("summary default"),
        s.model.as_deref().unwrap_or("summary default")
    );
    set_cache(s);
}

pub fn save<R: Runtime>(
    app: &AppHandle<R>,
    s: TemplateAssistSettings,
) -> Result<TemplateAssistSettings, String> {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let valid = checked(s)?;
    let value = serde_json::to_value(&valid).map_err(|e| e.to_string())?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())?;
    set_cache(valid.clone());
    Ok(valid)
}

/// The LLM target for a template assist call, from the saved override or the summary model.
pub(crate) async fn target(pool: &SqlitePool) -> Result<LlmTarget, String> {
    let s = current();
    target_with_override(pool, s.provider, s.model).await
}

#[tauri::command]
pub async fn template_assist_get_settings() -> TemplateAssistSettings {
    current()
}

#[tauri::command]
pub async fn template_assist_save_settings<R: Runtime>(
    app: AppHandle<R>,
    settings: TemplateAssistSettings,
) -> Result<TemplateAssistSettings, String> {
    save(&app, settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_is_all_off() {
        assert_eq!(from_stored(None), TemplateAssistSettings::default());
    }

    #[test]
    fn camel_case_keys() {
        let v = serde_json::to_value(TemplateAssistSettings::default()).unwrap();
        assert!(v.get("includeTranscript").is_some());
        assert!(v.get("hideEditHint").is_some());
    }

    #[test]
    fn stored_values_kept() {
        let s = from_stored(Some(
            json!({"provider": "claude", "model": "m", "includeTranscript": true}),
        ));
        assert_eq!(s.provider.as_deref(), Some("claude"));
        assert_eq!(s.model.as_deref(), Some("m"));
        assert!(s.include_transcript);
    }

    #[test]
    fn half_or_unknown_provider_dropped() {
        let half = from_stored(Some(json!({"provider": "claude"})));
        assert_eq!((half.provider, half.model), (None, None));
        let builtin = from_stored(Some(json!({"provider": "builtin-ai", "model": "m"})));
        assert_eq!((builtin.provider, builtin.model), (None, None));
        let nope = from_stored(Some(json!({"provider": "nope", "model": "m"})));
        assert_eq!((nope.provider, nope.model), (None, None));
    }

    #[test]
    fn wrong_type_means_defaults() {
        assert_eq!(
            from_stored(Some(json!({"includeTranscript": "yes"}))),
            TemplateAssistSettings::default()
        );
    }

    #[test]
    fn save_check() {
        let provider_only = TemplateAssistSettings {
            provider: Some("claude".into()),
            ..Default::default()
        };
        assert!(checked(provider_only).is_err());
        let unknown = TemplateAssistSettings {
            provider: Some("nope".into()),
            model: Some("m".into()),
            ..Default::default()
        };
        assert!(checked(unknown).is_err());
        let blank = TemplateAssistSettings {
            provider: Some("  ".into()),
            model: Some("  ".into()),
            ..Default::default()
        };
        assert_eq!(checked(blank), Ok(TemplateAssistSettings::default()));
        assert_eq!(
            checked(TemplateAssistSettings::default()),
            Ok(TemplateAssistSettings::default())
        );
    }
}
