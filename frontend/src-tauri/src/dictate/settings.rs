// Dictate's settings. They live in dictate.json, not in a shared preferences object, so a save
// from another page cannot drop them. Startup reads them leniently; a save is checked strictly.

use dictate_input::hotkey::{Hotkey, MOD_CTRL, MOD_WIN};
use dictate_input::machine::{validate_bindings, Binding, Target};
use log::{info, warn};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::{LazyLock, Mutex, PoisonError, RwLock};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

pub const AS_SPOKEN_ID: &str = "as-spoken";
pub const CLEANED_UP_ID: &str = "cleaned-up";
pub const STARTER_ID: &str = "prompt-for-claude";

const STORE_FILE: &str = "dictate.json";
const KEY: &str = "settings";
const DEFAULT_MAIN: Hotkey = Hotkey {
    mods: MOD_CTRL | MOD_WIN,
    key: None,
};
const MAX_MODES: usize = 20;
const MAX_NAME_CHARS: usize = 60;
const MAX_INSTRUCTIONS_CHARS: usize = 4000;
const MAX_EXAMPLES: usize = 3;
const MAX_EXAMPLE_CHARS: usize = 2000;
const MAX_WORDS: usize = 200;
const MAX_WORD_CHARS: usize = 64;
const STARTER_INSTRUCTIONS: &str = "Rewrite the dictation as a clear, well-structured prompt for an AI coding assistant. Keep every requirement, constraint, file name and detail the speaker gave; add nothing they did not say. Use short paragraphs or a bulleted list when there are several points. Keep the speaker's language.";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModeExample {
    pub said: String,
    pub want: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CustomMode {
    pub id: String,
    pub name: String,
    pub instructions: String,
    pub examples: Vec<ModeExample>,
    pub hotkey: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DictateSettings {
    pub enabled: bool,
    pub main_hotkey: String,
    pub default_mode: String,
    /// None means the same provider and model as summaries.
    pub cleanup_provider: Option<String>,
    pub cleanup_model: Option<String>,
    /// Keys are AS_SPOKEN_ID or CLEANED_UP_ID.
    pub builtin_hotkeys: BTreeMap<String, String>,
    pub modes: Vec<CustomMode>,
    pub dictionary: Vec<String>,
}

fn starter_mode() -> CustomMode {
    CustomMode {
        id: STARTER_ID.to_string(),
        name: "Prompt for Claude".to_string(),
        instructions: STARTER_INSTRUCTIONS.to_string(),
        examples: Vec::new(),
        hotkey: None,
    }
}

impl Default for DictateSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            main_hotkey: DEFAULT_MAIN.display(),
            default_mode: CLEANED_UP_ID.to_string(),
            cleanup_provider: None,
            cleanup_model: None,
            builtin_hotkeys: BTreeMap::new(),
            modes: vec![starter_mode()],
            dictionary: Vec::new(),
        }
    }
}


static CACHE: LazyLock<RwLock<DictateSettings>> = LazyLock::new(|| RwLock::new(DictateSettings::default()));
/// Held across validate, store write and cache update, so concurrent saves cannot interleave.
static SAVE_LOCK: Mutex<()> = Mutex::new(());

/// Startup read. Never fails: a broken entry is dropped and logged, the rest is kept.
pub fn sanitize(raw: Option<Value>) -> DictateSettings {
    let obj = match raw {
        None | Some(Value::Null) => return DictateSettings::default(),
        Some(Value::Object(obj)) => obj,
        Some(_) => {
            warn!("dictate: settings are not an object, using defaults");
            return DictateSettings::default();
        }
    };
    let candidate = DictateSettings {
        enabled: field(&obj, "enabled", false),
        main_hotkey: field(&obj, "main_hotkey", DEFAULT_MAIN.display()),
        default_mode: field(&obj, "default_mode", CLEANED_UP_ID.to_string()),
        cleanup_provider: field(&obj, "cleanup_provider", None),
        cleanup_model: field(&obj, "cleanup_model", None),
        builtin_hotkeys: builtin_hotkeys(&obj),
        modes: modes(&obj),
        dictionary: list(&obj, "dictionary"),
    };
    normalise(candidate, false).unwrap_or_default()
}

/// Each built-in hotkey is read on its own, so one wrong-typed value keeps the others.
fn builtin_hotkeys(obj: &Map<String, Value>) -> BTreeMap<String, String> {
    match obj.get("builtin_hotkeys") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(Value::Object(entries)) => entries
            .iter()
            .filter_map(|(id, value)| match value.as_str() {
                Some(text) => Some((id.clone(), text.to_string())),
                None => {
                    warn!("dictate: dropped a 'builtin_hotkeys' entry that is not text");
                    None
                }
            })
            .collect(),
        Some(_) => {
            warn!("dictate: 'builtin_hotkeys' is not an object, ignored");
            BTreeMap::new()
        }
    }
}

/// Each mode is read field by field, so a bad hotkey or example drops only that part.
fn modes(obj: &Map<String, Value>) -> Vec<CustomMode> {
    match obj.get("modes") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::Object(mode) => Some(CustomMode {
                    id: field(mode, "id", String::new()),
                    name: field(mode, "name", String::new()),
                    instructions: field(mode, "instructions", String::new()),
                    examples: list(mode, "examples"),
                    hotkey: field(mode, "hotkey", None),
                }),
                _ => {
                    warn!("dictate: dropped a 'modes' entry that is not an object");
                    None
                }
            })
            .collect(),
        Some(_) => {
            warn!("dictate: 'modes' is not a list, ignored");
            Vec::new()
        }
    }
}

/// Save check. Returns the normalised copy, or the first problem as a user-readable message.
pub fn validate_for_save(s: &DictateSettings) -> Result<DictateSettings, String> {
    normalise(s.clone(), true)
}

/// Hotkeys of the main binding, the built-in modes and the custom modes, in that order.
pub fn bindings(s: &DictateSettings) -> Vec<Binding> {
    let main = Hotkey::parse(&s.main_hotkey).ok().map(|hotkey| Binding {
        hotkey,
        target: Target::Main,
    });
    let builtins = s.builtin_hotkeys.iter().filter_map(|(id, text)| {
        Some(Binding {
            hotkey: Hotkey::parse(text).ok()?,
            target: Target::Mode(id.clone()),
        })
    });
    let modes = s.modes.iter().filter_map(|mode| {
        Some(Binding {
            hotkey: Hotkey::parse(mode.hotkey.as_deref()?).ok()?,
            target: Target::Mode(mode.id.clone()),
        })
    });
    main.into_iter().chain(builtins).chain(modes).collect()
}

pub fn current() -> DictateSettings {
    CACHE.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn set_cache(s: DictateSettings) {
    *CACHE.write().unwrap_or_else(PoisonError::into_inner) = s;
}

#[cfg(test)]
pub fn set_cache_for_test(s: DictateSettings) {
    set_cache(s);
}

pub fn load_at_startup<R: Runtime>(app: &AppHandle<R>) {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let raw = match app.store(STORE_FILE) {
        Ok(store) => store.get(KEY),
        Err(e) => {
            warn!("dictate: settings store unreadable, Dictate stays off: {e}");
            None
        }
    };
    let s = sanitize(raw);
    info!("dictate: {}", if s.enabled { "on" } else { "off" });
    set_cache(s);
}

pub fn save<R: Runtime>(app: &AppHandle<R>, s: DictateSettings) -> Result<DictateSettings, String> {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let valid = validate_for_save(&s)?;
    let value = serde_json::to_value(&valid).map_err(|e| e.to_string())?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())?;
    set_cache(valid.clone());
    Ok(valid)
}

#[tauri::command]
pub async fn dictate_get_settings() -> DictateSettings {
    current()
}

/// A missing or wrong-typed scalar takes the default. Its content is never logged.
fn field<T: DeserializeOwned>(obj: &Map<String, Value>, key: &str, default: T) -> T {
    match obj.get(key) {
        None | Some(Value::Null) => default,
        Some(value) => serde_json::from_value(value.clone()).unwrap_or_else(|_| {
            warn!("dictate: setting '{key}' has the wrong type, using the default");
            default
        }),
    }
}

/// A list keeps each entry that reads, and logs each one that does not.
fn list<T: DeserializeOwned>(obj: &Map<String, Value>, key: &str) -> Vec<T> {
    match obj.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match serde_json::from_value(item.clone()) {
                Ok(entry) => Some(entry),
                Err(_) => {
                    warn!("dictate: dropped a '{key}' entry that could not be read");
                    None
                }
            })
            .collect(),
        Some(_) => {
            warn!("dictate: '{key}' is not a list, ignored");
            Vec::new()
        }
    }
}

/// The one rule set. Strict (save) stops at the first problem. Lenient (startup) drops the broken
/// entry, logs it without its content, and falls back.
fn normalise(s: DictateSettings, strict: bool) -> Result<DictateSettings, String> {
    let main = match check_hotkey(&s.main_hotkey, &[]) {
        Ok(hotkey) => hotkey,
        Err(e) => {
            reject_hotkey(strict, "main", e)?;
            DEFAULT_MAIN
        }
    };
    let mut bindings = vec![Binding {
        hotkey: main,
        target: Target::Main,
    }];

    let mut builtin_hotkeys = BTreeMap::new();
    for (id, text) in &s.builtin_hotkeys {
        if text.trim().is_empty() {
            // A blank value means "no hotkey", the same as a blank mode hotkey.
            continue;
        }
        let checked = if is_builtin(id) {
            check_hotkey(text, &bindings)
        } else {
            Err(HotkeyError::fixed("is for an unknown mode"))
        };
        match checked {
            Ok(hotkey) => {
                builtin_hotkeys.insert(id.clone(), hotkey.display());
                bindings.push(Binding {
                    hotkey,
                    target: Target::Mode(id.clone()),
                });
            }
            Err(e) => reject_hotkey(strict, "built-in", e)?,
        }
    }

    let mut modes: Vec<CustomMode> = Vec::new();
    for (i, mut mode) in s.modes.into_iter().enumerate() {
        let label = format!("mode {}", i + 1);
        if let Err(e) = check_mode_fields(&mode, &modes) {
            reject(strict, format!("{label} {e}"))?;
            continue;
        }
        let mut examples = Vec::new();
        for example in std::mem::take(&mut mode.examples) {
            if example.said.chars().count() > MAX_EXAMPLE_CHARS
                || example.want.chars().count() > MAX_EXAMPLE_CHARS
            {
                reject(
                    strict,
                    format!("{label} has an example longer than {MAX_EXAMPLE_CHARS} characters"),
                )?;
            } else if examples.len() == MAX_EXAMPLES {
                reject(
                    strict,
                    format!("{label} has more than {MAX_EXAMPLES} examples"),
                )?;
            } else {
                examples.push(example);
            }
        }
        mode.examples = examples;
        mode.name = mode.name.trim().to_string();
        mode.hotkey = match mode.hotkey.take() {
            Some(text) if !text.trim().is_empty() => match check_hotkey(&text, &bindings) {
                Ok(hotkey) => {
                    bindings.push(Binding {
                        hotkey,
                        target: Target::Mode(mode.id.clone()),
                    });
                    Some(hotkey.display())
                }
                Err(e) => {
                    reject_hotkey(strict, &label, e)?;
                    None
                }
            },
            _ => None,
        };
        modes.push(mode);
    }

    let mut dictionary: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for (i, word) in s.dictionary.iter().enumerate() {
        let word = word.trim();
        let len = word.chars().count();
        if !(1..=MAX_WORD_CHARS).contains(&len) {
            reject(
                strict,
                format!(
                    "dictionary word {} must be 1-{MAX_WORD_CHARS} characters",
                    i + 1
                ),
            )?;
        } else if !seen.insert(word.to_lowercase()) {
            reject(
                strict,
                format!("dictionary word {} is already in the dictionary", i + 1),
            )?;
        } else if dictionary.len() == MAX_WORDS {
            reject(
                strict,
                format!("dictionary has more than {MAX_WORDS} words"),
            )?;
        } else {
            dictionary.push(word.to_string());
        }
    }

    let known = is_builtin(&s.default_mode) || modes.iter().any(|m| m.id == s.default_mode);
    let default_mode = if known {
        s.default_mode
    } else {
        reject(strict, "default mode is not a known mode".to_string())?;
        CLEANED_UP_ID.to_string()
    };

    Ok(DictateSettings {
        enabled: s.enabled,
        main_hotkey: main.display(),
        default_mode,
        cleanup_provider: optional_text(s.cleanup_provider),
        cleanup_model: optional_text(s.cleanup_model),
        builtin_hotkeys,
        modes,
        dictionary,
    })
}

fn reject(strict: bool, problem: String) -> Result<(), String> {
    if strict {
        return Err(problem);
    }
    warn!("dictate: dropped a setting: {problem}");
    Ok(())
}

/// A hotkey problem. `detail` may echo the user's text, so it goes to strict errors only.
#[derive(Debug)]
struct HotkeyError {
    reason: &'static str,
    detail: String,
}

impl HotkeyError {
    fn fixed(reason: &'static str) -> Self {
        Self {
            reason,
            detail: reason.to_string(),
        }
    }
}

fn reject_hotkey(strict: bool, label: &str, e: HotkeyError) -> Result<(), String> {
    if strict {
        return Err(format!("{label} hotkey {}", e.detail));
    }
    warn!("dictate: dropped a setting: {label} hotkey {}", e.reason);
    Ok(())
}

/// Parses the hotkey and checks it against the bindings already accepted, in order.
fn check_hotkey(text: &str, accepted: &[Binding]) -> Result<Hotkey, HotkeyError> {
    let hotkey = Hotkey::parse(text).map_err(|_| HotkeyError::fixed("is not a valid hotkey"))?;
    validate_bindings(&[Binding {
        hotkey,
        target: Target::Main,
    }])
    .map_err(|detail| HotkeyError {
        reason: "is not an allowed combination",
        detail,
    })?;
    if accepted.iter().any(|b| b.hotkey == hotkey) {
        return Err(HotkeyError {
            reason: "is already used by another hotkey",
            detail: format!("'{}' is already used by another hotkey", hotkey.display()),
        });
    }
    Ok(hotkey)
}

/// Checks a custom mode's own fields against the modes already kept. Its hotkey is checked separately.
fn check_mode_fields(mode: &CustomMode, kept: &[CustomMode]) -> Result<(), String> {
    if kept.len() == MAX_MODES {
        return Err(format!("is over the limit of {MAX_MODES} custom modes"));
    }
    if !valid_id(&mode.id) {
        return Err("id must be 1-40 characters: a-z, 0-9 or -".to_string());
    }
    if is_builtin(&mode.id) {
        return Err("id is a built-in mode id".to_string());
    }
    if kept.iter().any(|k| k.id == mode.id) {
        return Err("id is already used by another mode".to_string());
    }
    let name_len = mode.name.trim().chars().count();
    if !(1..=MAX_NAME_CHARS).contains(&name_len) {
        return Err(format!("name must be 1-{MAX_NAME_CHARS} characters"));
    }
    if mode.instructions.chars().count() > MAX_INSTRUCTIONS_CHARS {
        return Err(format!(
            "instructions are longer than {MAX_INSTRUCTIONS_CHARS} characters"
        ));
    }
    Ok(())
}

fn is_builtin(id: &str) -> bool {
    id == AS_SPOKEN_ID || id == CLEANED_UP_ID
}

fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Tests that set the global cache hold this, so they do not see each other's settings.
#[cfg(test)]
pub(crate) static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mode_json(id: &str, name: &str) -> Value {
        json!({"id": id, "name": name, "instructions": "Do it.", "examples": [], "hotkey": null})
    }

    #[test]
    fn defaults_are_off_with_starter_mode() {
        let d = sanitize(None);
        assert!(!d.enabled);
        assert_eq!(d.main_hotkey, "Ctrl+Win");
        assert_eq!(d.default_mode, "cleaned-up");
        assert_eq!(d.modes[0].id, "prompt-for-claude");
        assert_eq!(d.modes[0].name, "Prompt for Claude");
        assert_eq!(
            d.modes[0].instructions,
            "Rewrite the dictation as a clear, well-structured prompt for an AI coding assistant. Keep every requirement, constraint, file name and detail the speaker gave; add nothing they did not say. Use short paragraphs or a bulleted list when there are several points. Keep the speaker's language."
        );
        assert!(d.modes[0].examples.is_empty() && d.modes[0].hotkey.is_none());
        assert_eq!(d, DictateSettings::default());
        assert!(validate_for_save(&d).is_ok());
    }

    #[test]
    fn missing_store_value_vs_deleted_starter() {
        assert!(sanitize(Some(json!({"enabled": true, "modes": []})))
            .modes
            .is_empty());
    }

    #[test]
    fn sanitize_drops_broken_entries() {
        let examples: Vec<Value> = (0..5).map(|_| json!({"said": "s", "want": "w"})).collect();
        let raw = json!({
            "enabled": true,
            "modes": [
                mode_json("x", "X"),
                mode_json("y", "   "),
                mode_json("x", "Again"),
                mode_json("as-spoken", "AS"),
                {"id": "z", "name": "Z", "instructions": "", "examples": examples, "hotkey": null},
            ],
            "dictionary": ["Conclave", 7, "a".repeat(65), "conclave", "Tauri"],
        });
        let s = sanitize(Some(raw));
        let ids: Vec<&str> = s.modes.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["x", "z"]);
        assert_eq!(s.modes[1].examples.len(), 3);
        assert_eq!(s.dictionary, ["Conclave", "Tauri"]);
        assert_eq!(validate_for_save(&s).unwrap(), s);
    }

    #[test]
    fn sanitize_hotkey_fallbacks() {
        let s = sanitize(Some(json!({
            "main_hotkey": "Ctrl",
            "default_mode": "nope",
            "modes": [
                {"id": "a", "name": "A", "instructions": "", "examples": [], "hotkey": "ctrl+win"},
                {"id": "b", "name": "B", "instructions": "", "examples": [], "hotkey": "alt+p"},
                {"id": "c", "name": "C", "instructions": "", "examples": [], "hotkey": "Ctrl+Tab"},
            ],
        })));
        assert_eq!(s.main_hotkey, "Ctrl+Win");
        assert_eq!(s.default_mode, "cleaned-up");
        assert_eq!(s.modes[0].hotkey, None);
        assert_eq!(s.modes[1].hotkey.as_deref(), Some("Alt+P"));
        assert_eq!(s.modes[2].hotkey, None);
    }

    #[test]
    fn sanitize_never_panics_on_garbage() {
        for raw in [
            json!(42),
            json!("x"),
            json!([1, 2]),
            json!(true),
            json!(null),
        ] {
            assert_eq!(sanitize(Some(raw)), DictateSettings::default());
        }
        let s = sanitize(Some(json!({"modes": "x", "dictionary": {}})));
        assert!(s.modes.is_empty() && s.dictionary.is_empty());
        let s = sanitize(Some(json!({
            "enabled": "yes",
            "main_hotkey": 5,
            "builtin_hotkeys": 3,
            "cleanup_model": 7,
            "modes": [42, null, {"id": 1}],
        })));
        assert!(!s.enabled && s.modes.is_empty());
        assert_eq!(s.main_hotkey, "Ctrl+Win");
        assert_eq!(s.cleanup_model, None);
    }

    #[test]
    fn validate_for_save_reports_conflict() {
        let mut s = DictateSettings::default();
        s.modes.push(CustomMode {
            id: "m".into(),
            name: "M".into(),
            instructions: String::new(),
            examples: vec![],
            hotkey: Some("Ctrl+Win".into()),
        });
        let err = validate_for_save(&s).unwrap_err();
        assert!(err.contains("already used"), "{err}");
    }

    #[test]
    fn validate_for_save_normalises_and_enforces_limits() {
        let mut s = DictateSettings::default();
        s.main_hotkey = "win+ctrl".into();
        assert_eq!(validate_for_save(&s).unwrap().main_hotkey, "Ctrl+Win");

        let mut s = DictateSettings::default();
        s.default_mode = "nope".into();
        assert!(validate_for_save(&s).is_err());

        let mut s = DictateSettings::default();
        s.dictionary = (0..201).map(|i| format!("word{i}")).collect();
        assert!(validate_for_save(&s).is_err());

        let mut s = DictateSettings::default();
        s.dictionary = vec!["Conclave".into(), "conclave".into()];
        assert!(validate_for_save(&s).is_err());

        let mut s = DictateSettings::default();
        s.modes[0].examples = (0..4)
            .map(|_| ModeExample {
                said: "s".into(),
                want: "w".into(),
            })
            .collect();
        assert!(validate_for_save(&s).is_err());

        let mut s = DictateSettings::default();
        s.modes = (0..21)
            .map(|i| CustomMode {
                id: format!("m{i}"),
                ..starter_mode()
            })
            .collect();
        assert!(validate_for_save(&s).is_err());
    }

    #[test]
    fn bindings_map_targets() {
        let mut s = DictateSettings::default();
        s.builtin_hotkeys
            .insert(CLEANED_UP_ID.to_string(), "Alt+P".to_string());
        let b = bindings(&s);
        assert_eq!(
            b[0],
            Binding {
                hotkey: Hotkey::parse("Ctrl+Win").unwrap(),
                target: Target::Main
            }
        );
        assert!(b.contains(&Binding {
            hotkey: Hotkey::parse("Alt+P").unwrap(),
            target: Target::Mode("cleaned-up".to_string()),
        }));
    }

    #[test]
    fn cache_follows_set_cache_for_test() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let mut s = DictateSettings::default();
        s.dictionary = vec!["Conclave".into(), "Tauri".into()];
        set_cache_for_test(s.clone());
        assert_eq!(current(), s);
    }

    #[test]
    fn sanitize_keeps_mode_with_bad_parts() {
        let s = sanitize(Some(json!({
            "modes": [{
                "id": "m",
                "name": "M",
                "instructions": "Do it.",
                "hotkey": 42,
                "examples": [{"said": 5, "want": "w"}, {"said": "s", "want": "w"}],
            }],
        })));
        assert_eq!(s.modes.len(), 1);
        assert_eq!(s.modes[0].hotkey, None);
        assert_eq!(s.modes[0].examples.len(), 1);
        assert_eq!(s.modes[0].examples[0].said, "s");
        assert_eq!(validate_for_save(&s).unwrap(), s);
    }

    #[test]
    fn sanitize_reads_builtin_hotkeys_entry_by_entry() {
        let s = sanitize(Some(json!({
            "builtin_hotkeys": {"as-spoken": 3, "cleaned-up": "Alt+P"},
        })));
        assert_eq!(s.builtin_hotkeys.len(), 1);
        assert_eq!(s.builtin_hotkeys["cleaned-up"], "Alt+P");
    }

    #[test]
    fn blank_builtin_hotkey_removes_the_entry() {
        let mut s = DictateSettings::default();
        s.builtin_hotkeys
            .insert(AS_SPOKEN_ID.to_string(), "  ".to_string());
        assert!(validate_for_save(&s).unwrap().builtin_hotkeys.is_empty());
    }

    #[test]
    fn dictionary_word_length_counts_chars_not_bytes() {
        let ok = "ж".repeat(64);
        let too_long = "ж".repeat(65);
        let s = sanitize(Some(json!({"dictionary": [ok, too_long]})));
        assert_eq!(s.dictionary, ["ж".repeat(64)]);
    }
}
