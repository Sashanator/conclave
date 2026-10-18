// The speaker-identification switch. It lives in its own store file, not in RecordingPreferences,
// because the Recording page saves that whole object and would drop an unknown field.

use log::{info, warn};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "speakers.json";
const KEY: &str = "enabled";

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Cheap: read on every recording start and model load.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Only a real `true` turns the feature on. A missing, unreadable or hand-edited value stays off.
fn parse_enabled(value: Option<Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

pub fn load_at_startup<R: Runtime>(app: &AppHandle<R>) {
    let value = match app.store(STORE_FILE) {
        Ok(store) => store.get(KEY),
        Err(e) => {
            warn!("speakers: settings store unreadable, feature stays off: {e}");
            None
        }
    };
    let enabled = parse_enabled(value);
    ENABLED.store(enabled, Ordering::Relaxed);
    info!("speakers: identification is {}", if enabled { "on" } else { "off" });
}

pub fn save_enabled<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(KEY, enabled);
    store.save().map_err(|e| e.to_string())?;
    ENABLED.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn speaker_setting_defaults_off() {
        assert!(!parse_enabled(None));
        assert!(!parse_enabled(Some(json!("yes"))));
        assert!(!parse_enabled(Some(json!(1))));
        assert!(parse_enabled(Some(json!(true))));
        assert!(!is_enabled());
    }
}
