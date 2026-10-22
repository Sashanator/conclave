//! Resolves which LLM provider, model, key and endpoints the user configured.
//! Shared by summary generation and Dictate cleanup.

use crate::database::repositories::setting::SettingsRepository;
use crate::summary::llm_client::{generate_summary, LLMProvider, LlmCompletion};
use sqlx::SqlitePool;
use std::path::PathBuf;
use tracing::info;

// No Debug or Clone: api_key must never reach a log line or a copy.
pub(crate) struct LlmTarget {
    pub provider: LLMProvider,
    pub model: String,
    pub api_key: String,
    pub ollama_endpoint: Option<String>,
    pub custom_openai_endpoint: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

/// The provider and model for a feature: its own override when both are set, else the summary model.
pub(crate) async fn target_with_override(
    pool: &SqlitePool,
    provider: Option<String>,
    model: Option<String>,
) -> Result<LlmTarget, String> {
    let (provider, model) = match (provider, model) {
        (Some(p), Some(m)) => (p, m),
        _ => {
            let config = SettingsRepository::get_model_config(pool)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("no summary provider set")?;
            (config.provider, config.model)
        }
    };
    resolve_llm_target(pool, &provider, &model).await
}

/// Reads the saved provider settings. The error strings are user-facing and stay as summaries had them.
pub(crate) async fn resolve_llm_target(
    pool: &SqlitePool,
    provider: &str,
    model: &str,
) -> Result<LlmTarget, String> {
    let parsed = LLMProvider::from_str(provider)?;
    let mut target = LlmTarget {
        provider: parsed.clone(),
        model: model.to_string(),
        api_key: String::new(),
        ollama_endpoint: None,
        custom_openai_endpoint: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
    };

    match parsed {
        LLMProvider::Ollama => {
            target.ollama_endpoint = match SettingsRepository::get_model_config(pool).await {
                Ok(Some(config)) => config.ollama_endpoint,
                Ok(None) => None,
                Err(e) => {
                    info!("Failed to retrieve Ollama endpoint: {}, using default", e);
                    None
                }
            };
        }
        // Local model: no key and no endpoint.
        LLMProvider::BuiltInAI => {}
        LLMProvider::CustomOpenAI => match SettingsRepository::get_custom_openai_config(pool).await
        {
            Ok(Some(config)) => {
                info!("✓ Using custom OpenAI endpoint: {}", config.endpoint);
                target.custom_openai_endpoint = Some(config.endpoint);
                target.api_key = config.api_key.unwrap_or_default();
                target.max_tokens = config.max_tokens.map(|t| t as u32);
                target.temperature = config.temperature;
                target.top_p = config.top_p;
            }
            Ok(None) => {
                return Err(
                    "Custom OpenAI provider selected but no configuration found".to_string()
                );
            }
            Err(e) => {
                return Err(format!("Failed to retrieve custom OpenAI config: {}", e));
            }
        },
        _ => {
            // The raw string, not the parsed name, is the settings key, as before.
            target.api_key = match SettingsRepository::get_api_key(pool, provider).await {
                Ok(Some(key)) if !key.is_empty() => key,
                Ok(None) | Ok(Some(_)) => {
                    return Err(format!("API key not found for {}", provider));
                }
                Err(e) => {
                    return Err(format!(
                        "Failed to retrieve API key for {}: {}",
                        provider, e
                    ));
                }
            };
        }
    }

    Ok(target)
}

/// One non-streaming completion against the target, with a fresh HTTP client and no cancellation.
pub(crate) async fn complete(
    target: &LlmTarget,
    system: &str,
    user: &str,
    temperature: Option<f32>,
    app_data_dir: Option<&PathBuf>,
) -> Result<LlmCompletion, String> {
    let client = reqwest::Client::new();
    generate_summary(
        &client,
        &target.provider,
        &target.model,
        &target.api_key,
        system,
        user,
        target.ollama_endpoint.as_deref(),
        target.custom_openai_endpoint.as_deref(),
        target.max_tokens,
        temperature,
        target.top_p,
        app_data_dir,
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::setting::SettingsRepository;
    use crate::database::repositories::test_pool;
    use crate::summary::CustomOpenAIConfig;

    #[tokio::test]
    async fn missing_key_errors_like_before() {
        let pool = test_pool().await;
        let err = resolve_llm_target(&pool, "groq", "llama-3").await.err();
        assert_eq!(err.as_deref(), Some("API key not found for groq"));
    }

    #[tokio::test]
    async fn groq_key_resolves() {
        let pool = test_pool().await;
        SettingsRepository::save_api_key(&pool, "groq", "k")
            .await
            .unwrap();
        let target = resolve_llm_target(&pool, "groq", "llama-3").await.unwrap();
        assert_eq!(target.api_key, "k");
        assert_eq!(target.provider, LLMProvider::Groq);
        assert_eq!(target.model, "llama-3");
    }

    #[tokio::test]
    async fn custom_openai_uses_config() {
        let pool = test_pool().await;
        let config = CustomOpenAIConfig {
            endpoint: "http://x/v1".to_string(),
            api_key: Some("c".to_string()),
            model: "m".to_string(),
            max_tokens: Some(100),
            temperature: None,
            top_p: None,
        };
        SettingsRepository::save_custom_openai_config(&pool, &config)
            .await
            .unwrap();
        let target = resolve_llm_target(&pool, "custom-openai", "m")
            .await
            .unwrap();
        assert_eq!(
            target.custom_openai_endpoint.as_deref(),
            Some("http://x/v1")
        );
        assert_eq!(target.api_key, "c");
        assert_eq!(target.max_tokens, Some(100));
    }

    #[tokio::test]
    async fn unknown_provider_errors() {
        let pool = test_pool().await;
        assert!(resolve_llm_target(&pool, "nope", "m").await.is_err());
    }

    #[tokio::test]
    async fn ollama_reads_the_saved_endpoint() {
        let pool = test_pool().await;
        SettingsRepository::save_model_config(&pool, "ollama", "llama3", "", Some("http://h:1"))
            .await
            .unwrap();
        let target = resolve_llm_target(&pool, "ollama", "llama3").await.unwrap();
        assert_eq!(target.provider, LLMProvider::Ollama);
        assert_eq!(target.model, "llama3");
        assert_eq!(target.ollama_endpoint.as_deref(), Some("http://h:1"));
        assert!(target.api_key.is_empty());
        assert!(target.custom_openai_endpoint.is_none());
    }

    #[tokio::test]
    async fn ollama_without_saved_config_uses_the_default_endpoint() {
        let pool = test_pool().await;
        let target = resolve_llm_target(&pool, "ollama", "llama3").await.unwrap();
        assert!(target.ollama_endpoint.is_none());
    }

    #[tokio::test]
    async fn builtin_ai_needs_no_key_or_endpoint() {
        let pool = test_pool().await;
        let target = resolve_llm_target(&pool, "builtin-ai", "gemma").await.unwrap();
        assert_eq!(target.provider, LLMProvider::BuiltInAI);
        assert!(target.api_key.is_empty());
        assert!(target.ollama_endpoint.is_none());
        assert!(target.custom_openai_endpoint.is_none());
    }

    #[tokio::test]
    async fn custom_openai_without_config_errors_with_the_user_facing_text() {
        let pool = test_pool().await;
        let err = resolve_llm_target(&pool, "custom-openai", "m").await.err();
        assert_eq!(
            err.as_deref(),
            Some("Custom OpenAI provider selected but no configuration found")
        );
    }
}
