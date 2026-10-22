// Model prices in $ per million tokens. Built-in rows ship with the app; the user's overrides
// live in costs.json and win over a built-in row with the same provider and model.

use crate::summary::llm_client::TokenUsage;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex, PoisonError, RwLock};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "costs.json";
const KEY: &str = "prices";
const MAX_PRICE: f64 = 1000.0;
/// Local models cost nothing, so they need no price row.
const FREE_PROVIDERS: [&str; 2] = ["ollama", "builtin-ai"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PriceRow {
    pub provider: String,
    pub model: String,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    #[serde(default)]
    pub built_in: bool,
}

static CACHE: LazyLock<RwLock<Vec<PriceRow>>> = LazyLock::new(|| RwLock::new(Vec::new()));
/// Held across validate, store write and cache update, so concurrent saves cannot interleave.
static SAVE_LOCK: Mutex<()> = Mutex::new(());
#[cfg(test)]
pub(crate) static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn row(
    provider: &str,
    model: &str,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
) -> PriceRow {
    PriceRow {
        provider: provider.to_string(),
        model: model.to_string(),
        input,
        output,
        cache_read,
        cache_write,
        built_in: true,
    }
}

pub fn built_in() -> Vec<PriceRow> {
    vec![
        row("claude", "claude-haiku-5-5", 0.10, 0.50, 0.01, 0.125),
        row("claude", "claude-sonnet-5-5", 2.00, 10.00, 0.20, 2.50),
        row("claude", "claude-opus-5-5", 4.00, 20.00, 0.20, 5.00),
        row("claude", "claude-fable-5-1", 10.00, 50.00, 0.25, 12.50),
    ]
}

fn same_key(a: &PriceRow, b: &PriceRow) -> bool {
    a.provider == b.provider && a.model == b.model
}

/// The override if there is one, else the built-in row.
pub fn lookup(provider: &str, model: &str) -> Option<PriceRow> {
    let overrides = CACHE.read().unwrap_or_else(PoisonError::into_inner);
    overrides
        .iter()
        .find(|r| r.provider == provider && r.model == model)
        .cloned()
        .or_else(|| {
            built_in()
                .into_iter()
                .find(|r| r.provider == provider && r.model == model)
        })
}

/// None when there is no price, or when the call's input or output tokens are unknown.
pub fn cost(provider: &str, model: &str, u: &TokenUsage) -> Option<f64> {
    match lookup(provider, model) {
        Some(p) => {
            let input = u.input?;
            let output = u.output?;
            let tokens = |n: Option<u64>| n.unwrap_or(0) as f64;
            Some(
                (input as f64 * p.input
                    + output as f64 * p.output
                    + tokens(u.cache_read) * p.cache_read
                    + tokens(u.cache_write) * p.cache_write)
                    / 1e6,
            )
        }
        None if FREE_PROVIDERS.contains(&provider) => Some(0.0),
        None => None,
    }
}

fn check(r: &PriceRow) -> Result<(), String> {
    if r.provider.is_empty() || r.model.is_empty() {
        return Err("each price row needs a provider and a model".to_string());
    }
    let prices = [r.input, r.output, r.cache_read, r.cache_write];
    if prices
        .iter()
        .any(|p| !p.is_finite() || *p < 0.0 || *p > MAX_PRICE)
    {
        return Err(format!(
            "each price must be from 0 to {MAX_PRICE} $ per million tokens"
        ));
    }
    Ok(())
}

/// The one rule set. Strict (save) stops at the first problem. Lenient (startup) drops the broken
/// row and logs it without its content. Rows equal to a built-in row are not stored, so a later
/// app release can still update that price.
fn normalise(rows: Vec<PriceRow>, strict: bool) -> Result<Vec<PriceRow>, String> {
    let built = built_in();
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for mut r in rows {
        r.provider = r.provider.trim().to_string();
        r.model = r.model.trim().to_string();
        let problem = check(&r).err().or_else(|| {
            (!seen.insert((r.provider.clone(), r.model.clone())))
                .then(|| "duplicate provider and model".to_string())
        });
        if let Some(problem) = problem {
            if strict {
                return Err(problem);
            }
            warn!("usage: dropped a price row: {problem}");
            continue;
        }
        if built.contains(&PriceRow {
            built_in: true,
            ..r.clone()
        }) {
            continue;
        }
        kept.push(PriceRow {
            built_in: false,
            ..r
        });
    }
    Ok(kept)
}

fn set_cache(rows: Vec<PriceRow>) {
    *CACHE.write().unwrap_or_else(PoisonError::into_inner) = rows;
}

#[cfg(test)]
pub(crate) fn set_cache_for_test(rows: Vec<PriceRow>) {
    set_cache(rows);
}

/// Built-in rows, with an override in place where the user has one. Then the user's own rows.
fn merged() -> Vec<PriceRow> {
    let overrides = CACHE.read().unwrap_or_else(PoisonError::into_inner).clone();
    let built = built_in();
    let mut rows: Vec<PriceRow> = built
        .iter()
        .map(|b| match overrides.iter().find(|o| same_key(o, b)) {
            Some(o) => PriceRow {
                built_in: true,
                ..o.clone()
            },
            None => b.clone(),
        })
        .collect();
    rows.extend(
        overrides
            .into_iter()
            .filter(|o| !built.iter().any(|b| same_key(b, o))),
    );
    rows
}

/// Startup read. Never fails: a broken row is dropped and logged, the rest is kept.
fn sanitize(raw: Option<Value>) -> Vec<PriceRow> {
    let items = match raw {
        None | Some(Value::Null) => return Vec::new(),
        Some(Value::Array(items)) => items,
        Some(_) => {
            warn!("usage: price overrides are not a list, ignored");
            return Vec::new();
        }
    };
    let rows = items
        .into_iter()
        .filter_map(|item| match serde_json::from_value::<PriceRow>(item) {
            Ok(r) => Some(r),
            Err(_) => {
                warn!("usage: dropped a price row that is malformed");
                None
            }
        })
        .collect();
    normalise(rows, false).unwrap_or_default()
}

pub fn load_at_startup<R: Runtime>(app: &AppHandle<R>) {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let raw = match app.store(STORE_FILE) {
        Ok(store) => store.get(KEY),
        Err(e) => {
            warn!("usage: price store unreadable, built-in prices used: {e}");
            None
        }
    };
    let rows = sanitize(raw);
    info!("usage: {} price overrides loaded", rows.len());
    set_cache(rows);
}

#[tauri::command]
pub async fn costs_get_prices() -> Vec<PriceRow> {
    merged()
}

#[tauri::command]
pub async fn costs_save_prices<R: Runtime>(
    app: AppHandle<R>,
    rows: Vec<PriceRow>,
) -> Result<Vec<PriceRow>, String> {
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let overrides = normalise(rows, true)?;
    let value = serde_json::to_value(&overrides).map_err(|e| e.to_string())?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())?;
    set_cache(overrides);
    Ok(merged())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        CACHE_TEST_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn usage(input: Option<u64>, output: Option<u64>, cache_read: Option<u64>) -> TokenUsage {
        TokenUsage {
            input,
            output,
            cache_read,
            cache_write: None,
            reported_cost_usd: None,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn built_in_prices() {
        let b = built_in();
        let find = |m: &str| {
            b.iter()
                .find(|r| r.provider == "claude" && r.model == m)
                .unwrap()
                .clone()
        };
        assert_eq!(b.len(), 4);
        assert_eq!(
            find("claude-haiku-5-5"),
            row("claude", "claude-haiku-5-5", 0.10, 0.50, 0.01, 0.125)
        );
        assert_eq!(
            find("claude-sonnet-5-5"),
            row("claude", "claude-sonnet-5-5", 2.00, 10.00, 0.20, 2.50)
        );
        assert_eq!(
            find("claude-opus-5-5"),
            row("claude", "claude-opus-5-5", 4.00, 20.00, 0.20, 5.00)
        );
        assert_eq!(
            find("claude-fable-5-1"),
            row("claude", "claude-fable-5-1", 10.00, 50.00, 0.25, 12.50)
        );
    }

    #[test]
    fn cost_sonnet() {
        let _g = guard();
        set_cache_for_test(Vec::new());
        let c = cost(
            "claude",
            "claude-sonnet-5-5",
            &usage(Some(1_000_000), Some(100_000), None),
        );
        assert!(close(c.unwrap(), 3.0));
    }

    #[test]
    fn cost_with_cache() {
        let _g = guard();
        set_cache_for_test(Vec::new());
        let c = cost(
            "claude",
            "claude-haiku-5-5",
            &usage(Some(1000), Some(1000), Some(1_000_000)),
        );
        assert!(close(c.unwrap(), 0.0001 + 0.0005 + 0.01));
    }

    #[test]
    fn cost_unknown_model_none() {
        let _g = guard();
        set_cache_for_test(Vec::new());
        assert_eq!(
            cost("claude", "claude-nope", &usage(Some(1), Some(1), None)),
            None
        );
        // Unknown input or output tokens cannot be priced.
        assert_eq!(
            cost("claude", "claude-sonnet-5-5", &usage(None, Some(1), None)),
            None
        );
        assert_eq!(
            cost("claude", "claude-sonnet-5-5", &usage(Some(1), None, None)),
            None
        );
    }

    #[test]
    fn cost_ollama_zero() {
        let _g = guard();
        set_cache_for_test(Vec::new());
        assert_eq!(
            cost("ollama", "llama3", &usage(Some(5), Some(5), None)),
            Some(0.0)
        );
        assert_eq!(cost("builtin-ai", "x", &usage(None, None, None)), Some(0.0));
    }

    #[test]
    fn override_wins() {
        let _g = guard();
        set_cache_for_test(Vec::new());
        let saved = normalise(
            vec![row("claude", "claude-sonnet-5-5", 3.0, 10.0, 0.20, 2.50)],
            true,
        )
        .unwrap();
        set_cache_for_test(saved);

        assert_eq!(lookup("claude", "claude-sonnet-5-5").unwrap().input, 3.0);
        let listed = merged();
        assert_eq!(
            listed
                .iter()
                .filter(|r| r.model == "claude-sonnet-5-5")
                .count(),
            1
        );
        assert_eq!(listed.len(), 4);
        set_cache_for_test(Vec::new());
    }

    #[test]
    fn save_rejects_bad_rows() {
        let good = || row("claude", "m", 1.0, 1.0, 0.0, 0.0);
        let bad = |f: fn(&mut PriceRow)| {
            let mut r = good();
            f(&mut r);
            normalise(vec![r], true).is_err()
        };
        assert!(bad(|r| r.input = -1.0));
        assert!(bad(|r| r.output = f64::NAN));
        assert!(bad(|r| r.cache_write = f64::INFINITY));
        assert!(bad(|r| r.cache_read = 1000.5));
        assert!(bad(|r| r.provider = "  ".to_string()));
        assert!(bad(|r| r.model = String::new()));
        assert!(normalise(vec![good(), good()], true).is_err());
        assert!(normalise(vec![good(), row("claude", "m2", 1.0, 1.0, 0.0, 0.0)], true).is_ok());
        // Lenient load keeps the good row and drops the bad one.
        let kept = normalise(vec![good(), row("claude", "m", 2.0, 2.0, 0.0, 0.0)], false).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].input, 1.0);
    }

    #[test]
    fn built_in_equal_rows_are_not_stored() {
        let same = normalise(
            vec![row("claude", "claude-haiku-5-5", 0.10, 0.50, 0.01, 0.125)],
            true,
        )
        .unwrap();
        assert!(same.is_empty());
    }

    #[test]
    fn sanitize_skips_malformed_rows() {
        let raw = serde_json::json!([
            {"provider": "claude", "model": "m", "input": 1.0, "output": 1.0, "cache_read": 0.0, "cache_write": 0.0},
            {"provider": "claude", "model": "bad", "input": "x"},
            "junk"
        ]);
        let rows = sanitize(Some(raw));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "m");
        assert!(sanitize(Some(serde_json::json!("junk"))).is_empty());
    }

    #[tokio::test]
    async fn get_prices_lists_built_ins_and_overrides_once() {
        let _g = guard();
        set_cache_for_test(
            normalise(
                vec![row("claude", "claude-sonnet-5-5", 3.0, 10.0, 0.20, 2.50)],
                true,
            )
            .unwrap(),
        );
        let listed = costs_get_prices().await;
        assert_eq!(listed.len(), 4);
        let sonnet = listed
            .iter()
            .find(|r| r.model == "claude-sonnet-5-5")
            .unwrap();
        assert_eq!(sonnet.input, 3.0);
        assert!(sonnet.built_in);
        let haiku = listed
            .iter()
            .find(|r| r.model == "claude-haiku-5-5")
            .unwrap();
        assert!(haiku.built_in);
        set_cache_for_test(Vec::new());
    }
}
