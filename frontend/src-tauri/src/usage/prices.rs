// Model prices in $ per million tokens. Built-in rows ship with the app; the user's overrides
// live in costs.json and win over a built-in row with the same provider and model. Claude prices
// refreshed from OpenRouter's public model list (at most weekly) sit between the two.

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
const FETCHED_KEY: &str = "fetched_prices";
const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
const REFRESH_AFTER_SECS: i64 = 7 * 24 * 3600;
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

/// Claude rows fetched from OpenRouter, with the unix time of the successful fetch.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct Fetched {
    at: i64,
    rows: Vec<PriceRow>,
}

static FETCHED: RwLock<Option<Fetched>> = RwLock::new(None);
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

/// Built-in rows with fetched rows laid over them (a fetched model that is not built in is added).
/// All rows come back flagged built_in: the user can only override them, not delete them.
fn base() -> Vec<PriceRow> {
    let fetched = FETCHED.read().unwrap_or_else(PoisonError::into_inner).clone();
    let mut rows = built_in();
    for f in fetched.into_iter().flat_map(|f| f.rows) {
        let f = PriceRow { built_in: true, ..f };
        match rows.iter_mut().find(|r| same_key(r, &f)) {
            Some(r) => *r = f,
            None => rows.push(f),
        }
    }
    rows
}

/// The override if there is one, else the fetched row, else the built-in row.
pub fn lookup(provider: &str, model: &str) -> Option<PriceRow> {
    lookup_exact(provider, model).or_else(|| {
        // A dated id (claude-sonnet-4-5-20250929) uses the undated row's price.
        let (undated, date) = model.rsplit_once('-')?;
        (date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()))
            .then(|| lookup_exact(provider, undated))?
    })
}

fn lookup_exact(provider: &str, model: &str) -> Option<PriceRow> {
    let overrides = CACHE.read().unwrap_or_else(PoisonError::into_inner);
    overrides
        .iter()
        .find(|r| r.provider == provider && r.model == model)
        .cloned()
        .or_else(|| {
            base()
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

/// The page sends back every row it shows, fetched ones included. A row equal to a fetched row
/// that is not already an override is taken as untouched and not stored, so later fetches still
/// update it. shortcut: typing a price equal to the fetched one is not stored either; it
/// follows the fetched price until the user changes it to something else.
fn without_untouched_fetched(rows: Vec<PriceRow>) -> Vec<PriceRow> {
    let fetched = FETCHED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|f| f.rows.clone())
        .unwrap_or_default();
    let current = CACHE.read().unwrap_or_else(PoisonError::into_inner).clone();
    let same = |o: &PriceRow, r: &PriceRow| {
        same_key(o, r) && PriceRow { built_in: false, ..o.clone() } == PriceRow { built_in: false, ..r.clone() }
    };
    rows.into_iter()
        .filter(|r| !fetched.iter().any(|o| same(o, r)) || current.iter().any(|o| same(o, r)))
        .collect()
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
    let built = base();
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
    let fetched = match app.store(STORE_FILE) {
        Ok(store) => store.get(FETCHED_KEY),
        Err(_) => None, // already logged above
    };
    set_fetched(sanitize_fetched(fetched));
    drop(_saving);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = refresh_if_stale(&app, MODELS_URL).await {
            warn!("usage: price refresh failed, last known prices kept: {e}");
        }
    });
}

fn set_fetched(f: Option<Fetched>) {
    *FETCHED.write().unwrap_or_else(PoisonError::into_inner) = f;
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A fetch time in the future (clock moved back) counts as stale too.
fn is_stale(fetched_at: Option<i64>, now: i64) -> bool {
    fetched_at.map_or(true, |at| at > now || now - at > REFRESH_AFTER_SECS)
}

const MAX_FETCHED_ROWS: usize = 50;

fn clean_fetched(rows: Vec<PriceRow>) -> Vec<PriceRow> {
    let mut seen = HashSet::new();
    rows.into_iter()
        .filter(|r| check(r).is_ok() && seen.insert((r.provider.clone(), r.model.clone())))
        .map(|r| PriceRow { built_in: false, ..r })
        .take(MAX_FETCHED_ROWS)
        .collect()
}

fn sanitize_fetched(raw: Option<Value>) -> Option<Fetched> {
    let raw = raw.filter(|v| !v.is_null())?;
    match serde_json::from_value::<Fetched>(raw) {
        Ok(f) => Some(Fetched { rows: clean_fetched(f.rows), ..f }),
        Err(_) => {
            warn!("usage: fetched prices are malformed, ignored");
            None
        }
    }
}

/// anthropic/claude-sonnet-5.5 -> claude-sonnet-5-5. Variants such as ":thinking" are skipped.
fn map_model_id(id: &str) -> Option<String> {
    let name = id.strip_prefix("anthropic/")?;
    (name.starts_with("claude-") && !name.contains(':')).then(|| name.replace('.', "-"))
}

/// OpenRouter prices are $ per token, as strings. Rounded to 1e-6 $ per million, which also
/// removes float noise such as 3.0000000000000004.
fn per_million(v: Option<&str>) -> Option<f64> {
    let p: f64 = v?.trim().parse().ok()?;
    Some((p * 1e12).round() / 1e6)
}

#[derive(Deserialize)]
struct ApiList {
    // Rows are parsed one by one, so one malformed row cannot fail the whole refresh.
    data: Vec<Value>,
}

#[derive(Deserialize)]
struct ApiModel {
    id: String,
    pricing: Option<ApiPricing>,
}

#[derive(Deserialize)]
struct ApiPricing {
    prompt: Option<String>,
    completion: Option<String>,
    input_cache_read: Option<String>,
    input_cache_write: Option<String>,
}

/// A model with a missing or invalid price field is skipped whole (its built-in price stays),
/// rather than priced at 0 for its cache tokens.
fn parse_models(list: ApiList) -> Vec<PriceRow> {
    let rows = list
        .data
        .into_iter()
        .filter_map(|v| serde_json::from_value::<ApiModel>(v).ok())
        .filter_map(|m| {
            let model = map_model_id(&m.id)?;
            let p = m.pricing?;
            Some(PriceRow {
                provider: "claude".to_string(),
                model,
                input: per_million(p.prompt.as_deref())?,
                output: per_million(p.completion.as_deref())?,
                cache_read: per_million(p.input_cache_read.as_deref())?,
                cache_write: per_million(p.input_cache_write.as_deref())?,
                built_in: false,
            })
        })
        .collect();
    clean_fetched(rows)
}

async fn fetch_rows(url: &str) -> Result<Vec<PriceRow>, String> {
    let response = reqwest::Client::new()
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status()));
    }
    let list: ApiList = response
        .json()
        .await
        .map_err(|e| format!("unexpected response: {e}"))?;
    let rows = parse_models(list);
    if rows.is_empty() {
        return Err("no usable Claude prices in the list".to_string());
    }
    Ok(rows)
}

async fn refresh_if_stale<R: Runtime>(app: &AppHandle<R>, url: &str) -> Result<(), String> {
    let at = FETCHED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|f| f.at);
    if !is_stale(at, now_secs()) {
        return Ok(());
    }
    let fetched = Fetched { at: now_secs(), rows: fetch_rows(url).await? };
    let _saving = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let value = serde_json::to_value(&fetched).map_err(|e| e.to_string())?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(FETCHED_KEY, value);
    store.save().map_err(|e| e.to_string())?;
    info!("usage: {} Claude prices updated from OpenRouter", fetched.rows.len());
    set_fetched(Some(fetched));
    Ok(())
}

/// Unix seconds of the last successful price fetch, None if there never was one.
#[tauri::command]
pub async fn costs_prices_fetched_at() -> Option<i64> {
    FETCHED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|f| f.at)
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
    let overrides = normalise(without_untouched_fetched(rows), true)?;
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

    fn fetched_row(model: &str, input: f64) -> PriceRow {
        PriceRow { built_in: false, ..row("claude", model, input, 9.0, 0.5, 1.0) }
    }

    #[test]
    fn openrouter_ids_map_to_conclave_ids() {
        assert_eq!(map_model_id("anthropic/claude-sonnet-5.5").as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(map_model_id("anthropic/claude-fable-5.1").as_deref(), Some("claude-fable-5-1"));
        assert_eq!(map_model_id("anthropic/claude-3.7-sonnet:thinking"), None);
        assert_eq!(map_model_id("openai/gpt-5"), None);
        assert_eq!(map_model_id("anthropic/other"), None);
    }

    #[test]
    fn parse_converts_to_per_million_and_skips_bad_rows() {
        let list: ApiList = serde_json::from_value(serde_json::json!({"data": [
            {"id": "anthropic/claude-sonnet-5.5", "pricing": {"prompt": "0.000003", "completion": "0.000015",
              "input_cache_read": "0.0000003", "input_cache_write": "0.00000375"}},
            {"id": "anthropic/claude-opus-5.5", "pricing": {"prompt": "-1", "completion": "0.00002",
              "input_cache_read": "0", "input_cache_write": "0"}},
            {"id": "anthropic/claude-haiku-5.5", "pricing": {"prompt": "0.000001", "completion": "0.000005"}},
            {"id": "anthropic/claude-x", "pricing": {"prompt": "1", "completion": "1",
              "input_cache_read": "1", "input_cache_write": "1"}},
            {"id": "anthropic/claude-y", "pricing": {"prompt": "abc", "completion": "1",
              "input_cache_read": "1", "input_cache_write": "1"}},
            {"id": "openai/gpt-5", "pricing": {"prompt": "0.000001", "completion": "0.000005",
              "input_cache_read": "0", "input_cache_write": "0"}}
        ]})).unwrap();
        let rows = parse_models(list);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "claude-sonnet-5-5");
        assert_eq!((rows[0].input, rows[0].output), (3.0, 15.0));
        assert_eq!((rows[0].cache_read, rows[0].cache_write), (0.3, 3.75));
        assert!(!rows[0].built_in);
    }

    #[test]
    fn staleness_is_seven_days() {
        let now = 10_000_000;
        assert!(is_stale(None, now));
        assert!(!is_stale(Some(now - REFRESH_AFTER_SECS), now));
        assert!(is_stale(Some(now - REFRESH_AFTER_SECS - 1), now));
        assert!(is_stale(Some(now + 1), now));
    }

    #[test]
    fn priority_is_override_then_fetched_then_built_in() {
        let _g = guard();
        let sonnet = || lookup("claude", "claude-sonnet-5-5").unwrap().input;
        assert_eq!(sonnet(), 2.0);
        set_fetched(Some(Fetched { at: 1, rows: vec![fetched_row("claude-sonnet-5-5", 3.0), fetched_row("claude-new-9", 7.0)] }));
        assert_eq!(sonnet(), 3.0);
        assert_eq!(lookup("claude", "claude-new-9").unwrap().input, 7.0);
        set_cache_for_test(normalise(vec![row("claude", "claude-sonnet-5-5", 4.0, 1.0, 0.0, 0.0)], true).unwrap());
        assert_eq!(sonnet(), 4.0);
        // Saving the merged list back must not freeze fetched rows as overrides.
        set_cache_for_test(Vec::new());
        let merged_rows = merged();
        assert_eq!(merged_rows.iter().find(|r| r.model == "claude-new-9").unwrap().input, 7.0);
        assert!(normalise(without_untouched_fetched(merged_rows), true).unwrap().is_empty());
        set_fetched(None);
        set_cache_for_test(Vec::new());
        assert_eq!(sonnet(), 2.0);
    }

    #[test]
    fn stored_fetched_rows_are_validated() {
        let raw = serde_json::json!({"at": 5, "rows": [
            {"provider": "claude", "model": "ok", "input": 1.0, "output": 1.0, "cache_read": 0.0, "cache_write": 0.0},
            {"provider": "claude", "model": "neg", "input": -1.0, "output": 1.0, "cache_read": 0.0, "cache_write": 0.0}
        ]});
        let f = sanitize_fetched(Some(raw)).unwrap();
        assert_eq!((f.at, f.rows.len()), (5, 1));
        assert!(sanitize_fetched(Some(serde_json::json!("junk"))).is_none());
        assert!(sanitize_fetched(None).is_none());
    }

    async fn serve_once(status: &'static str, body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        url
    }

    #[tokio::test]
    async fn fetch_rows_reads_the_list_and_fails_loudly() {
        let ok = serve_once("200 OK", r#"{"data":[{"id":"anthropic/claude-sonnet-5.5","pricing":{"prompt":"0.000003","completion":"0.000015","input_cache_read":"0.0000003","input_cache_write":"0.00000375"}}]}"#).await;
        assert_eq!(fetch_rows(&ok).await.unwrap()[0].model, "claude-sonnet-5-5");
        let server_error = serve_once("500 Internal Server Error", "{}").await;
        assert!(fetch_rows(&server_error).await.unwrap_err().contains("500"));
        let junk = serve_once("200 OK", "not json").await;
        assert!(fetch_rows(&junk).await.is_err());
        let empty = serve_once("200 OK", r#"{"data":[]}"#).await;
        assert!(fetch_rows(&empty).await.is_err());
    }

    #[test]
    fn dated_ids_use_the_undated_price() {
        let _g = guard();
        set_cache_for_test(Vec::new());
        assert_eq!(lookup("claude", "claude-sonnet-5-5-20260101").unwrap().input, 2.0);
        assert!(lookup("claude", "claude-sonnet-5-5-2026").is_none());
        assert!(lookup("claude", "claude-nope-20260101").is_none());
    }

    #[test]
    fn one_bad_row_does_not_fail_the_list() {
        let list: ApiList = serde_json::from_value(serde_json::json!({"data": [
            {"id": 5, "pricing": "junk"},
            {"id": "anthropic/claude-sonnet-5.5", "pricing": {"prompt": "0.000003", "completion": "0.000015",
              "input_cache_read": "0.0000003", "input_cache_write": "0.00000375"}}
        ]})).unwrap();
        assert_eq!(parse_models(list).len(), 1);
    }

    #[test]
    fn user_rows_equal_to_fetched_survive_and_fetched_rows_are_capped() {
        let _g = guard();
        set_fetched(Some(Fetched { at: 1, rows: vec![fetched_row("claude-sonnet-5-5", 3.0)] }));
        // An existing override equal to the fetched price stays; an untouched fetched row is not stored.
        set_cache_for_test(normalise(vec![row("claude", "claude-sonnet-5-5", 3.0, 9.0, 0.5, 1.0)], true).unwrap());
        let kept = normalise(without_untouched_fetched(merged()), true).unwrap();
        assert_eq!(kept.len(), 1);
        set_fetched(None);
        set_cache_for_test(Vec::new());
        let many = (0..80).map(|i| fetched_row(&format!("claude-m-{i}"), 1.0)).collect();
        assert_eq!(clean_fetched(many).len(), MAX_FETCHED_ROWS);
    }
}
