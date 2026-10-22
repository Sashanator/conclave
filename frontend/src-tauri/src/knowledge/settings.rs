// Knowledge settings. They live in knowledge.json, not in a shared preferences object, so a save
// from another page cannot drop them. Startup reads them leniently; a save is checked strictly.

use super::{MAX_NAMES, MAX_NAME_CHARS};
use crate::copilot;
use crate::summary::llm_target::{target_with_override, LlmTarget};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex, PoisonError, RwLock};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "knowledge.json";
const KEY: &str = "settings";
/// The same allow-list as Copilot.
pub(crate) const PROVIDERS: [&str; 6] = [
    "claude",
    "openai",
    "groq",
    "openrouter",
    "ollama",
    "custom-openai",
];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct KnowledgeSettings {
    /// None means the summary model. Both are set, or neither.
    pub provider: Option<String>,
    pub model: Option<String>,
    pub my_names: Vec<String>,
    pub extract_enabled: bool,
    pub digest_enabled: bool,
    /// A credential. It is stored here and never returned to the frontend.
    pub calendar_url: Option<String>,
    /// Set once the names have been seeded from Copilot's alert words, so a cleared list stays empty.
    pub names_seeded: bool,
}

// Debug hides the calendar URL, so a logged or printed settings value cannot leak it.
impl std::fmt::Debug for KnowledgeSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KnowledgeSettings")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("my_names", &self.my_names)
            .field("extract_enabled", &self.extract_enabled)
            .field("digest_enabled", &self.digest_enabled)
            .field(
                "calendar_url",
                &if self.calendar_url.is_some() {
                    "<set>"
                } else {
                    "<unset>"
                },
            )
            .field("names_seeded", &self.names_seeded)
            .finish()
    }
}

impl Default for KnowledgeSettings {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            my_names: Vec::new(),
            extract_enabled: true,
            digest_enabled: true,
            calendar_url: None,
            names_seeded: false,
        }
    }
}

#[derive(Serialize, Debug, PartialEq)]
pub struct KnowledgeSettingsView {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub my_names: Vec<String>,
    pub extract_enabled: bool,
    pub digest_enabled: bool,
    /// The calendar host only. The path and query are secrets and are never sent.
    pub calendar_host: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct KnowledgeSettingsInput {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub my_names: Vec<String>,
    pub extract_enabled: bool,
    pub digest_enabled: bool,
}

static CACHE: LazyLock<RwLock<KnowledgeSettings>> =
    LazyLock::new(|| RwLock::new(KnowledgeSettings::default()));
/// Held across validate, store write and cache update, so concurrent saves cannot interleave.
static SAVE_LOCK: Mutex<()> = Mutex::new(());

/// Startup read. Never fails: a broken entry is dropped and logged, the rest is kept.
pub fn sanitize(raw: Option<Value>) -> KnowledgeSettings {
    let obj = match raw {
        None | Some(Value::Null) => return KnowledgeSettings::default(),
        Some(Value::Object(obj)) => obj,
        Some(_) => {
            warn!("knowledge: settings are not an object, using defaults");
            return KnowledgeSettings::default();
        }
    };
    let candidate = KnowledgeSettings {
        provider: text_field(&obj, "provider"),
        model: text_field(&obj, "model"),
        my_names: names(&obj),
        extract_enabled: flag(&obj, "extract_enabled", true),
        digest_enabled: flag(&obj, "digest_enabled", true),
        calendar_url: text_field(&obj, "calendar_url"),
        names_seeded: flag(&obj, "names_seeded", false),
    };
    normalise(candidate, false).unwrap_or_default()
}

/// A wrong-typed value is dropped. Its content is never logged, so the calendar URL stays out of logs.
fn text_field(obj: &Map<String, Value>, key: &str) -> Option<String> {
    match obj.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => {
            warn!("knowledge: setting '{key}' is not text, ignored");
            None
        }
    }
}

fn flag(obj: &Map<String, Value>, key: &str, default: bool) -> bool {
    match obj.get(key) {
        None | Some(Value::Null) => default,
        Some(Value::Bool(on)) => *on,
        Some(_) => {
            warn!("knowledge: setting '{key}' is not on or off, default used");
            default
        }
    }
}

/// Each entry is read on its own, so one wrong-typed name keeps the others.
fn names(obj: &Map<String, Value>) -> Vec<String> {
    match obj.get("my_names") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item.as_str() {
                Some(name) => Some(name.to_string()),
                None => {
                    warn!("knowledge: dropped a name that is not text");
                    None
                }
            })
            .collect(),
        Some(_) => {
            warn!("knowledge: 'my_names' is not a list, ignored");
            Vec::new()
        }
    }
}

/// The one rule set. Strict (save) stops at the first problem. Lenient (startup) drops the broken
/// entry, logs it without its content, and falls back.
fn normalise(s: KnowledgeSettings, strict: bool) -> Result<KnowledgeSettings, String> {
    let (provider, model) = match (optional(s.provider), optional(s.model)) {
        (None, None) => (None, None),
        (Some(p), Some(m)) if PROVIDERS.contains(&p.as_str()) => (Some(p), Some(m)),
        _ => {
            reject(
                strict,
                "knowledge provider and model must both be set, with a supported provider"
                    .to_string(),
            )?;
            (None, None)
        }
    };

    let mut my_names: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for name in &s.my_names {
        let name = name.trim();
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        if name.chars().count() > MAX_NAME_CHARS {
            reject(
                strict,
                format!("each name must be at most {MAX_NAME_CHARS} characters"),
            )?;
        } else if my_names.len() == MAX_NAMES {
            reject(strict, format!("at most {MAX_NAMES} names"))?;
        } else {
            my_names.push(name.to_string());
        }
    }

    Ok(KnowledgeSettings {
        provider,
        model,
        my_names,
        extract_enabled: s.extract_enabled,
        digest_enabled: s.digest_enabled,
        calendar_url: s.calendar_url,
        names_seeded: s.names_seeded,
    })
}

fn reject(strict: bool, problem: String) -> Result<(), String> {
    if strict {
        return Err(problem);
    }
    warn!("knowledge: dropped a setting: {problem}");
    Ok(())
}

fn optional(value: Option<String>) -> Option<String> {
    value
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Copies the save's fields. The calendar URL and the seeding flag are not in the form, so they are kept.
fn apply_input(stored: &KnowledgeSettings, input: KnowledgeSettingsInput) -> KnowledgeSettings {
    KnowledgeSettings {
        provider: input.provider,
        model: input.model,
        my_names: input.my_names,
        extract_enabled: input.extract_enabled,
        digest_enabled: input.digest_enabled,
        calendar_url: stored.calendar_url.clone(),
        names_seeded: stored.names_seeded,
    }
}

/// Fills the names from Copilot's alert words, once. After that, the user's list is never touched.
fn seed_names(s: KnowledgeSettings, alert_words: Vec<String>) -> KnowledgeSettings {
    if s.names_seeded {
        return s;
    }
    let seeded = normalise(
        KnowledgeSettings {
            my_names: alert_words,
            ..KnowledgeSettings::default()
        },
        false,
    )
    .unwrap_or_default();
    KnowledgeSettings {
        my_names: seeded.my_names,
        names_seeded: true,
        ..s
    }
}

fn with_calendar_url(s: KnowledgeSettings, url: Option<String>) -> KnowledgeSettings {
    KnowledgeSettings {
        calendar_url: optional(url),
        ..s
    }
}

/// Only the host is shown. A URL that does not parse has no host to show.
fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(str::to_string)
}

fn view(s: &KnowledgeSettings) -> KnowledgeSettingsView {
    KnowledgeSettingsView {
        provider: s.provider.clone(),
        model: s.model.clone(),
        my_names: s.my_names.clone(),
        extract_enabled: s.extract_enabled,
        digest_enabled: s.digest_enabled,
        calendar_host: s.calendar_url.as_deref().and_then(host_of),
    }
}

pub fn current() -> KnowledgeSettings {
    CACHE.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn set_cache(s: KnowledgeSettings) {
    *CACHE.write().unwrap_or_else(PoisonError::into_inner) = s;
}

fn persist<R: Runtime>(app: &AppHandle<R>, s: &KnowledgeSettings) -> Result<(), String> {
    let value = serde_json::to_value(s).map_err(|e| e.to_string())?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())
}

/// Knowledge model: its own provider and model if both are set, else the summary model.
/// Resolves exactly as copilot::ask::copilot_target does.
pub(crate) async fn target(pool: &SqlitePool) -> Result<LlmTarget, String> {
    let s = current();
    target_with_override(pool, s.provider, s.model).await
}

/// Used by the calendar task. The URL is a credential: it is stored, never logged.
pub(crate) fn set_calendar_url<R: Runtime>(
    app: &AppHandle<R>,
    url: Option<String>,
) -> Result<(), String> {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let s = with_calendar_url(current(), url);
    persist(app, &s)?;
    set_cache(s);
    Ok(())
}

pub fn load_at_startup<R: Runtime>(app: &AppHandle<R>) {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let raw = match app.store(STORE_FILE) {
        Ok(store) => store.get(KEY),
        Err(e) => {
            warn!("knowledge: settings store unreadable, defaults used: {e}");
            None
        }
    };
    let mut s = sanitize(raw);
    if !s.names_seeded {
        s = seed_names(s, copilot::settings::current().alert_words);
        if let Err(e) = persist(app, &s) {
            warn!("knowledge: could not save the seeded names: {e}");
        }
    }
    info!("knowledge: settings loaded, {} names", s.my_names.len());
    set_cache(s);
}

#[tauri::command]
pub async fn knowledge_get_settings() -> KnowledgeSettingsView {
    view(&current())
}

#[tauri::command]
pub async fn knowledge_save_settings<R: Runtime>(
    app: AppHandle<R>,
    input: KnowledgeSettingsInput,
) -> Result<KnowledgeSettingsView, String> {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let valid = normalise(apply_input(&current(), input), true)?;
    persist(&app, &valid)?;
    set_cache(valid.clone());
    Ok(view(&valid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with_names(names: &[&str]) -> KnowledgeSettings {
        KnowledgeSettings {
            my_names: names.iter().map(|n| n.to_string()).collect(),
            ..Default::default()
        }
    }

    fn with_provider(provider: Option<&str>, model: Option<&str>) -> KnowledgeSettings {
        KnowledgeSettings {
            provider: provider.map(str::to_string),
            model: model.map(str::to_string),
            ..Default::default()
        }
    }

    fn input_with_names(names: &[&str]) -> KnowledgeSettingsInput {
        KnowledgeSettingsInput {
            provider: None,
            model: None,
            my_names: names.iter().map(|n| n.to_string()).collect(),
            extract_enabled: true,
            digest_enabled: true,
        }
    }

    #[test]
    fn defaults_extract_and_digest_on() {
        let d = KnowledgeSettings::default();
        assert!(d.extract_enabled && d.digest_enabled);
        assert!(!d.names_seeded);
        assert_eq!(sanitize(None), d);
        assert_eq!(sanitize(Some(json!("junk"))), d);
    }

    #[test]
    fn sanitize_drops_bad_fields() {
        let s = sanitize(Some(json!({
            "provider": "builtin-ai",
            "model": "m",
            "my_names": ["Sasha", 5, ""],
            "extract_enabled": "yes",
            "digest_enabled": false,
            "calendar_url": 7,
            "names_seeded": true
        })));
        assert_eq!((s.provider, s.model), (None, None));
        assert_eq!(s.my_names, ["Sasha"]);
        assert!(s.extract_enabled);
        assert!(!s.digest_enabled);
        assert_eq!(s.calendar_url, None);
        assert!(s.names_seeded);
    }

    #[test]
    fn names_normalised_and_capped() {
        let s = normalise(with_names(&["  Sasha ", "sasha", "Саша", "", "Иван"]), true).unwrap();
        assert_eq!(s.my_names, ["Sasha", "Саша", "Иван"]);

        let many: Vec<String> = (0..21).map(|i| format!("name{i}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(normalise(with_names(&refs), true).is_err());
        let s = sanitize(Some(json!({ "my_names": many })));
        assert_eq!(s.my_names.len(), MAX_NAMES);
        assert_eq!(s.my_names[19], "name19");

        let long = "a".repeat(41);
        let err = normalise(with_names(&[&long]), true).unwrap_err();
        assert!(err.contains("40"), "{err}");
        let ok = "a".repeat(40);
        assert!(normalise(with_names(&[&ok]), true).is_ok());
    }

    #[test]
    fn seed_once_from_alert_words() {
        let words = vec!["Sasha".to_string(), "sasha".to_string(), "Саша".to_string()];
        let first = seed_names(KnowledgeSettings::default(), words.clone());
        assert_eq!(first.my_names, ["Sasha", "Саша"]);
        assert!(first.names_seeded);

        // The user clears the names and saves. The save keeps names_seeded.
        let cleared = normalise(apply_input(&first, input_with_names(&[])), true).unwrap();
        assert!(cleared.my_names.is_empty());
        assert!(cleared.names_seeded);

        // The next startup does not re-seed.
        assert_eq!(seed_names(cleared.clone(), words), cleared);
    }

    #[test]
    fn save_keeps_calendar_url() {
        let stored = KnowledgeSettings {
            calendar_url: Some("https://calendar.example.com/private/abc?token=t".to_string()),
            names_seeded: true,
            ..Default::default()
        };
        let saved = apply_input(&stored, input_with_names(&["Sasha"]));
        assert_eq!(saved.calendar_url, stored.calendar_url);
        assert!(saved.names_seeded);
        assert_eq!(saved.my_names, ["Sasha"]);
    }

    #[test]
    fn calendar_url_blank_is_cleared() {
        let s = with_calendar_url(KnowledgeSettings::default(), Some("  ".to_string()));
        assert_eq!(s.calendar_url, None);
        let s = with_calendar_url(s, Some(" https://a.example/x ".to_string()));
        assert_eq!(s.calendar_url.as_deref(), Some("https://a.example/x"));
    }

    #[test]
    fn view_never_contains_url() {
        let s = KnowledgeSettings {
            calendar_url: Some(
                "https://calendar.example.com/private/abc123/basic.ics?token=secret".to_string(),
            ),
            ..Default::default()
        };
        let json = serde_json::to_string(&view(&s)).unwrap();
        assert!(json.contains("calendar.example.com"), "{json}");
        for secret in ["/private", "abc123", ".ics", "token", "secret"] {
            assert!(!json.contains(secret), "{secret} leaked: {json}");
        }
        assert_eq!(view(&KnowledgeSettings::default()).calendar_host, None);
    }

    #[test]
    fn debug_never_contains_url() {
        let s = KnowledgeSettings {
            calendar_url: Some(
                "https://calendar.example.com/private/abc123?token=secret".to_string(),
            ),
            ..Default::default()
        };
        let dbg = format!("{:?}", s);
        for secret in [
            "calendar.example.com",
            "/private",
            "abc123",
            "token",
            "secret",
        ] {
            assert!(!dbg.contains(secret), "{secret} leaked: {dbg}");
        }
        assert!(dbg.contains("<set>"), "{dbg}");
        assert!(format!("{:?}", KnowledgeSettings::default()).contains("<unset>"));
    }

    #[test]
    fn provider_without_model_rejected() {
        assert!(normalise(with_provider(Some("groq"), None), true).is_err());
        assert!(normalise(with_provider(None, Some("m")), true).is_err());
        assert!(normalise(with_provider(Some("builtin-ai"), Some("m")), true).is_err());
        assert!(normalise(with_provider(Some("nope"), Some("m")), true).is_err());
        assert!(normalise(with_provider(None, None), true).is_ok());
        assert!(normalise(with_provider(Some("groq"), Some("llama")), true).is_ok());

        let s = sanitize(Some(json!({"provider": "groq"})));
        assert_eq!((s.provider, s.model), (None, None));
    }
}
