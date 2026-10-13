use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;
use log::{info, warn, error};
use anyhow::Result;

use crate::state::AppState;
use crate::config::{DEFAULT_SUMMARY_MODEL, DEFAULT_SUMMARY_PROVIDER, DEFAULT_TRANSCRIPT_PROVIDER, DEFAULT_WHISPER_MODEL};
use crate::database::repositories::setting::SettingsRepository;


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OnboardingStatus {
    pub version: String,
    pub completed: bool,
    pub current_step: u8,
    pub model_status: ModelStatus,
    pub last_updated: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ModelStatus {
    // Stored name kept for compatibility with existing onboarding-status.json; means the transcription model.
    pub parakeet: String,  // "downloaded" | "not_downloaded" | "downloading"
    pub summary: String,   // Unused since v0.2.1 (no local summary model); kept for stored-status compatibility
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_summary_model: Option<String>,
}

impl Default for OnboardingStatus {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            completed: false,
            current_step: 1,
            model_status: ModelStatus {
                parakeet: "not_downloaded".to_string(),
                summary: "not_downloaded".to_string(),  // Changed from gemma
                selected_summary_model: None,
            },
            last_updated: chrono::Utc::now().to_rfc3339(),
        }
    }
}


/// Load onboarding status from store
pub async fn load_onboarding_status<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<OnboardingStatus> {
    // Try to load from Tauri store
    let store = match app.store("onboarding-status.json") {
        Ok(store) => store,
        Err(e) => {
            warn!("Failed to access onboarding store: {}, using defaults", e);
            return Ok(OnboardingStatus::default());
        }
    };

    // Try to get the status from store
    let status = if let Some(value) = store.get("status") {
        match serde_json::from_value::<OnboardingStatus>(value.clone()) {
            Ok(s) => {
                info!("Loaded onboarding status from store - Step: {}, Completed: {}",
                      s.current_step, s.completed);
                s
            }
            Err(e) => {
                warn!("Failed to deserialize onboarding status: {}, using defaults", e);
                OnboardingStatus::default()
            }
        }
    } else {
        info!("No stored onboarding status found, using defaults");
        OnboardingStatus::default()
    };

    Ok(status)
}

/// Save onboarding status to store
pub async fn save_onboarding_status<R: Runtime>(
    app: &AppHandle<R>,
    status: &OnboardingStatus,
) -> Result<()> {
    info!("Saving onboarding status: step={}, completed={}",
          status.current_step, status.completed);

    // Get or create store
    let store = app.store("onboarding-status.json")
        .map_err(|e| anyhow::anyhow!("Failed to access onboarding store: {}", e))?;

    // Update last_updated timestamp
    let mut status = status.clone();
    status.last_updated = chrono::Utc::now().to_rfc3339();

    // Serialize status to JSON value
    let status_value = serde_json::to_value(&status)
        .map_err(|e| anyhow::anyhow!("Failed to serialize onboarding status: {}", e))?;

    // Save to store
    store.set("status", status_value);

    // Persist to disk
    store.save()
        .map_err(|e| anyhow::anyhow!("Failed to save onboarding store to disk: {}", e))?;

    info!("Successfully persisted onboarding status to disk");
    Ok(())
}

/// Reset onboarding status (delete from store)
pub async fn reset_onboarding_status<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<()> {
    info!("Resetting onboarding status");

    let store = app.store("onboarding-status.json")
        .map_err(|e| anyhow::anyhow!("Failed to access onboarding store: {}", e))?;

    // Clear the status key
    store.delete("status");

    // Persist deletion to disk
    store.save()
        .map_err(|e| anyhow::anyhow!("Failed to save onboarding store after reset: {}", e))?;

    info!("Successfully reset onboarding status");
    Ok(())
}

/// Tauri commands for onboarding status
#[tauri::command]
pub async fn get_onboarding_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<OnboardingStatus>, String> {
    let status = load_onboarding_status(&app)
        .await
        .map_err(|e| format!("Failed to load onboarding status: {}", e))?;

    // Return None if it's the default (never saved before)
    // Check if we have any saved data by seeing if the store has the key
    let store = app.store("onboarding-status.json")
        .map_err(|e| format!("Failed to access store: {}", e))?;

    if store.get("status").is_none() {
        Ok(None)
    } else {
        Ok(Some(status))
    }
}

#[tauri::command]
pub async fn save_onboarding_status_cmd<R: Runtime>(
    app: AppHandle<R>,
    status: OnboardingStatus,
) -> Result<(), String> {
    save_onboarding_status(&app, &status)
        .await
        .map_err(|e| format!("Failed to save onboarding status: {}", e))
}

#[tauri::command]
pub async fn reset_onboarding_status_cmd<R: Runtime>(
    app: AppHandle<R>,
) -> Result<(), String> {
    reset_onboarding_status(&app)
        .await
        .map_err(|e| format!("Failed to reset onboarding status: {}", e))
}

/// Saves first-run defaults without overwriting anything the user already configured.
pub(crate) async fn save_onboarding_defaults(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    SettingsRepository::save_transcript_config(
        pool,
        DEFAULT_TRANSCRIPT_PROVIDER,
        DEFAULT_WHISPER_MODEL,
    )
    .await?;
    info!("Saved transcription defaults: provider={}, model={}", DEFAULT_TRANSCRIPT_PROVIDER, DEFAULT_WHISPER_MODEL);

    if SettingsRepository::get_model_config(pool).await?.is_none() {
        SettingsRepository::save_model_config(
            pool,
            DEFAULT_SUMMARY_PROVIDER,
            DEFAULT_SUMMARY_MODEL,
            DEFAULT_WHISPER_MODEL,
            None,
        )
        .await?;
        info!("Saved default summary config: provider={}, model={}", DEFAULT_SUMMARY_PROVIDER, DEFAULT_SUMMARY_MODEL);
    }
    Ok(())
}

#[tauri::command]
pub async fn complete_onboarding<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    info!("Completing onboarding");

    // Step 1: Save defaults to SQLite FIRST
    save_onboarding_defaults(state.db_manager.pool())
        .await
        .map_err(|e| {
            error!("Failed to save onboarding defaults: {}", e);
            format!("Failed to save onboarding defaults: {}", e)
        })?;

    // Step 2: Only NOW mark onboarding as complete (after DB operations succeed)
    let mut status = load_onboarding_status(&app)
        .await
        .map_err(|e| format!("Failed to load onboarding status: {}", e))?;

    status.completed = true;
    status.current_step = 4; // Max step (4 on macOS with permissions, 3 on other platforms)
    // Stored field name kept for compatibility; it means the transcription model.
    status.model_status.parakeet = "downloaded".to_string();
    status.model_status.summary = "not_used".to_string();
    status.model_status.selected_summary_model = None;

    save_onboarding_status(&app, &status)
        .await
        .map_err(|e| format!("Failed to save completed onboarding status: {}", e))?;

    info!("Onboarding completed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn empty_db_gets_local_whisper_and_claude_haiku() {
        let pool = test_pool().await;
        save_onboarding_defaults(&pool).await.unwrap();

        let transcript = SettingsRepository::get_transcript_config(&pool).await.unwrap().unwrap();
        assert_eq!(transcript.provider, "localWhisper");
        assert_eq!(transcript.model, "large-v3-turbo");

        let model = SettingsRepository::get_model_config(&pool).await.unwrap().unwrap();
        assert_eq!(model.provider, "claude");
        assert_eq!(model.model, "claude-haiku-5-5");
    }

    #[tokio::test]
    async fn saved_summary_config_and_api_key_are_kept() {
        let pool = test_pool().await;
        SettingsRepository::save_model_config(&pool, "openai", "gpt-x", "large-v3", None).await.unwrap();
        SettingsRepository::save_api_key(&pool, "claude", "sk-test").await.unwrap();

        save_onboarding_defaults(&pool).await.unwrap();

        let model = SettingsRepository::get_model_config(&pool).await.unwrap().unwrap();
        assert_eq!(model.provider, "openai");
        assert_eq!(model.model, "gpt-x");
        assert_eq!(
            SettingsRepository::get_api_key(&pool, "claude").await.unwrap().as_deref(),
            Some("sk-test")
        );
    }

    #[test]
    fn onboarding_status_deserializes_without_selected_summary_model() {
        let status: OnboardingStatus = serde_json::from_str(
            r#"{
                "version": "1.0",
                "completed": true,
                "current_step": 4,
                "model_status": {
                    "parakeet": "downloaded",
                    "summary": "downloaded"
                },
                "last_updated": "2026-05-30T00:00:00Z"
            }"#,
        )
        .expect("old onboarding status should remain compatible");

        assert_eq!(status.model_status.selected_summary_model, None);
    }
}
