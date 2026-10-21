// Copilot's settings. They live in copilot.json, not in a shared preferences object, so a save
// from another page cannot drop them. Startup reads them leniently; a save is checked strictly.

use super::{MAX_ALERT_WORDS, MAX_ALERT_WORD_CHARS};
use crate::summary::processor::language_name_from_code;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex, PoisonError, RwLock};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "copilot.json";
const KEY: &str = "settings";
const PROVIDERS: [&str; 6] = [
    "claude",
    "openai",
    "groq",
    "openrouter",
    "ollama",
    "custom-openai",
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct CopilotSettings {
    /// None means the same provider and model as summaries. Both are set, or neither.
    pub provider: Option<String>,
    pub model: Option<String>,
    /// None means the frontend default (the pinned summary language, else "ru").
    pub answer_language: Option<String>,
    pub alert_words: Vec<String>,
}

static CACHE: LazyLock<RwLock<CopilotSettings>> =
    LazyLock::new(|| RwLock::new(CopilotSettings::default()));
/// Held across validate, store write and cache update, so concurrent saves cannot interleave.
static SAVE_LOCK: Mutex<()> = Mutex::new(());

/// Startup read. Never fails: a broken entry is dropped and logged, the rest is kept.
pub fn sanitize(raw: Option<Value>) -> CopilotSettings {
    let obj = match raw {
        None | Some(Value::Null) => return CopilotSettings::default(),
        Some(Value::Object(obj)) => obj,
        Some(_) => {
            warn!("copilot: settings are not an object, using defaults");
            return CopilotSettings::default();
        }
    };
    let candidate = CopilotSettings {
        provider: text_field(&obj, "provider"),
        model: text_field(&obj, "model"),
        answer_language: text_field(&obj, "answer_language"),
        alert_words: alert_words(&obj),
    };
    normalise(candidate, false).unwrap_or_default()
}

/// A wrong-typed value is dropped. Its content is never logged.
fn text_field(obj: &Map<String, Value>, key: &str) -> Option<String> {
    match obj.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => {
            warn!("copilot: setting '{key}' is not text, ignored");
            None
        }
    }
}

/// Each entry is read on its own, so one wrong-typed word keeps the others.
fn alert_words(obj: &Map<String, Value>) -> Vec<String> {
    match obj.get("alert_words") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item.as_str() {
                Some(word) => Some(word.to_string()),
                None => {
                    warn!("copilot: dropped an alert word that is not text");
                    None
                }
            })
            .collect(),
        Some(_) => {
            warn!("copilot: 'alert_words' is not a list, ignored");
            Vec::new()
        }
    }
}

/// The one rule set. Strict (save) stops at the first problem. Lenient (startup) drops the broken
/// entry, logs it without its content, and falls back.
fn normalise(s: CopilotSettings, strict: bool) -> Result<CopilotSettings, String> {
    let (provider, model) = match (optional(s.provider), optional(s.model)) {
        (None, None) => (None, None),
        (Some(p), Some(m)) if PROVIDERS.contains(&p.as_str()) => (Some(p), Some(m)),
        _ => {
            reject(
                strict,
                "copilot provider and model must both be set, with a supported provider"
                    .to_string(),
            )?;
            (None, None)
        }
    };

    let answer_language = match optional(s.answer_language) {
        Some(code) if language_name_from_code(&code).is_none() => {
            reject(
                strict,
                "answer language is not a supported language code".to_string(),
            )?;
            None
        }
        other => other,
    };

    let mut alert_words: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for word in &s.alert_words {
        let word = word.trim();
        if word.is_empty() || !seen.insert(word.to_lowercase()) {
            continue;
        }
        if !word.chars().all(char::is_alphanumeric) {
            reject(
                strict,
                "each alert word must be one word, letters and digits only".to_string(),
            )?;
        } else if word.chars().count() > MAX_ALERT_WORD_CHARS {
            reject(
                strict,
                format!("each alert word must be at most {MAX_ALERT_WORD_CHARS} characters"),
            )?;
        } else if alert_words.len() == MAX_ALERT_WORDS {
            reject(strict, format!("at most {MAX_ALERT_WORDS} alert words"))?;
        } else {
            alert_words.push(word.to_string());
        }
    }

    Ok(CopilotSettings {
        provider,
        model,
        answer_language,
        alert_words,
    })
}

fn reject(strict: bool, problem: String) -> Result<(), String> {
    if strict {
        return Err(problem);
    }
    warn!("copilot: dropped a setting: {problem}");
    Ok(())
}

fn optional(value: Option<String>) -> Option<String> {
    value
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

pub fn current() -> CopilotSettings {
    CACHE.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The per-transcript-line check. Reads the cache in place, so nothing is cloned when no words are set.
pub fn has_alert_words() -> bool {
    !CACHE.read().unwrap_or_else(PoisonError::into_inner).alert_words.is_empty()
}

fn set_cache(s: CopilotSettings) {
    *CACHE.write().unwrap_or_else(PoisonError::into_inner) = s;
}

#[cfg(test)]
pub fn set_cache_for_test(s: CopilotSettings) {
    set_cache(s);
}

/// The reason to log a settings file that failed to load. A missing file is the normal first run.
/// The reason is the load error only; it never carries the file's content.
fn store_problem<E: std::fmt::Display>(exists: bool, reload: Result<(), E>) -> Option<String> {
    match reload {
        Err(e) if exists => Some(e.to_string()),
        _ => None,
    }
}

pub fn load_at_startup<R: Runtime>(app: &AppHandle<R>) {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let raw = match app.store(STORE_FILE) {
        Ok(store) => {
            // The plugin drops a broken file silently when it opens, so reload to see the error.
            let exists = app
                .path()
                .resolve(STORE_FILE, BaseDirectory::AppData)
                .is_ok_and(|path| path.exists());
            if let Some(problem) = store_problem(exists, store.reload()) {
                warn!("copilot: settings file unreadable, defaults used: {problem}");
            }
            store.get(KEY)
        }
        Err(e) => {
            warn!("copilot: settings store unreadable, defaults used: {e}");
            None
        }
    };
    let s = sanitize(raw);
    info!(
        "copilot: settings loaded, {} alert words",
        s.alert_words.len()
    );
    set_cache(s);
}

#[tauri::command]
pub async fn copilot_get_settings() -> CopilotSettings {
    current()
}

#[tauri::command]
pub async fn copilot_save_settings<R: Runtime>(
    app: AppHandle<R>,
    settings: CopilotSettings,
) -> Result<CopilotSettings, String> {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let valid = normalise(settings, true)?;
    let value = serde_json::to_value(&valid).map_err(|e| e.to_string())?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())?;
    set_cache(valid.clone());
    Ok(valid)
}

/// Tests that set the global cache hold this, so they do not see each other's settings.
#[cfg(test)]
static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with_words(words: &[&str]) -> CopilotSettings {
        CopilotSettings {
            alert_words: words.iter().map(|w| w.to_string()).collect(),
            ..Default::default()
        }
    }

    fn with_provider(provider: Option<&str>, model: Option<&str>) -> CopilotSettings {
        CopilotSettings {
            provider: provider.map(str::to_string),
            model: model.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn defaults_are_off() {
        assert_eq!(sanitize(None), CopilotSettings::default());
        assert_eq!(sanitize(Some(json!("junk"))), CopilotSettings::default());
    }

    #[test]
    fn alert_words_cleaned() {
        let s = with_words(&["  Sasha ", "sasha", "Саша", ""]);
        assert_eq!(normalise(s, true).unwrap().alert_words, ["Sasha", "Саша"]);
    }

    #[test]
    fn alert_word_must_be_one_word() {
        let err = normalise(with_words(&["Sasha K"]), true).unwrap_err();
        assert!(err.contains("one word"), "{err}");
        let err = normalise(with_words(&["o'neil"]), true).unwrap_err();
        assert!(err.contains("one word"), "{err}");
        let s = sanitize(Some(json!({"alert_words": ["Sasha K", "o'neil", "Sasha"]})));
        assert_eq!(s.alert_words, ["Sasha"]);
    }

    #[test]
    fn alert_word_limits() {
        let long = "a".repeat(41);
        let err = normalise(with_words(&[&long]), true).unwrap_err();
        assert!(err.contains("40"), "{err}");
        let ok = "a".repeat(40);
        assert!(normalise(with_words(&[&ok]), true).is_ok());

        let many: Vec<String> = (0..31).map(|i| format!("name{i}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(normalise(with_words(&refs), true).is_err());

        let s = sanitize(Some(json!({"alert_words": many})));
        assert_eq!(s.alert_words.len(), 30);
        assert_eq!(s.alert_words[0], "name0");
        assert_eq!(s.alert_words[29], "name29");
    }

    #[test]
    fn provider_rules() {
        let err = normalise(with_provider(Some("builtin-ai"), Some("m")), true).unwrap_err();
        assert!(!err.is_empty());
        assert!(normalise(with_provider(Some("groq"), None), true).is_err());
        assert!(normalise(with_provider(None, Some("m")), true).is_err());
        assert!(normalise(with_provider(Some("nope"), Some("m")), true).is_err());
        assert!(normalise(with_provider(None, None), true).is_ok());
        assert!(normalise(with_provider(Some("groq"), Some("llama")), true).is_ok());

        let s = sanitize(Some(json!({"provider": "builtin-ai", "model": "m"})));
        assert_eq!((s.provider, s.model), (None, None));
        let s = sanitize(Some(json!({"provider": "groq"})));
        assert_eq!((s.provider, s.model), (None, None));
    }

    #[test]
    fn language_rules() {
        let s = CopilotSettings {
            answer_language: Some("de".to_string()),
            ..Default::default()
        };
        assert_eq!(
            normalise(s, true).unwrap().answer_language.as_deref(),
            Some("de")
        );

        let s = CopilotSettings {
            answer_language: Some("xx".to_string()),
            ..Default::default()
        };
        assert!(normalise(s, true).is_err());
        assert_eq!(
            sanitize(Some(json!({"answer_language": "xx"}))).answer_language,
            None
        );
    }

    #[test]
    fn unreadable_file_is_reported_only_when_it_exists() {
        // A missing file is the normal first run, so only an existing file that fails to load is reported.
        assert_eq!(store_problem(false, Err::<(), _>("not found")), None);
        assert_eq!(store_problem(false, Ok::<(), String>(())), None);
        assert_eq!(store_problem(true, Ok::<(), String>(())), None);
        assert_eq!(
            store_problem(true, Err::<(), _>("expected value at line 1 column 1")),
            Some("expected value at line 1 column 1".to_string())
        );
    }

    #[test]
    fn cache_follows_set_cache_for_test() {
        let _guard = CACHE_TEST_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let s = with_words(&["Sasha"]);
        set_cache_for_test(s.clone());
        assert_eq!(current(), s);
        assert!(has_alert_words());
        set_cache_for_test(CopilotSettings::default());
        assert!(!has_alert_words());
    }
}
