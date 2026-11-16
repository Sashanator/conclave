use crate::database::models::{Setting, TranscriptSetting};
use crate::summary::CustomOpenAIConfig;
use crate::secrets::{self, read_or_plain, transcript_name};
use sqlx::SqlitePool;

fn store_err(e: anyhow::Error) -> sqlx::Error {
    sqlx::Error::Protocol(e.to_string().into())
}

#[derive(serde::Deserialize, Debug)]
pub struct SaveModelConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

pub struct SettingsRepository;

// Transcript providers: localWhisper, deepgram, elevenLabs, groq, openai
// Summary providers: openai, claude, ollama, groq, added openrouter
// NOTE: Handle data exclusion in the higher layer as this is database abstraction layer(using SELECT *)

impl SettingsRepository {
    pub async fn get_model_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<Setting>, sqlx::Error> {
        let setting = sqlx::query_as::<_, Setting>("SELECT * FROM settings LIMIT 1")
            .fetch_optional(pool)
            .await?;
        Ok(setting)
    }

    pub async fn save_model_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
        whisper_model: &str,
        ollama_endpoint: Option<&str>,
    ) -> std::result::Result<(), sqlx::Error> {
        // Using id '1' for backward compatibility
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, ollamaEndpoint)
            VALUES ('1', $1, $2, $3, $4)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model,
                whisperModel = excluded.whisperModel,
                ollamaEndpoint = excluded.ollamaEndpoint
            "#,
        )
        .bind(provider)
        .bind(model)
        .bind(whisper_model)
        .bind(ollama_endpoint)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn save_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config (customOpenAIConfig) instead of a separate API key column
        if provider == "custom-openai" {
            return Err(sqlx::Error::Protocol(
                "custom-openai provider should use save_custom_openai_config() instead of save_api_key()".into(),
            ));
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "claude" => "anthropicApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(()), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        let query = format!(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, "{}")
            VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3', $1)
            ON CONFLICT(id) DO UPDATE SET
                "{}" = $1
            "#,
            api_key_column, api_key_column
        );
        // Store first: on failure the DB is untouched. The plain column is cleared.
        secrets::store().set(api_key_column, api_key).map_err(store_err)?;
        sqlx::query(&query).bind(None::<String>).execute(pool).await?;

        Ok(())
    }

    pub async fn get_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        // Custom OpenAI uses JSON config - extract API key from there
        if provider == "custom-openai" {
            let config = Self::get_custom_openai_config(pool).await?;
            return Ok(config.and_then(|c| c.api_key));
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "claude" => "anthropicApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(None), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        let query = format!(
            "SELECT {} FROM settings WHERE id = '1' LIMIT 1",
            api_key_column
        );
        let plain: Option<String> = sqlx::query_scalar(&query).fetch_optional(pool).await?;
        read_or_plain(secrets::store(), api_key_column, plain).map_err(store_err)
    }

    pub async fn get_transcript_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<TranscriptSetting>, sqlx::Error> {
        let setting =
            sqlx::query_as::<_, TranscriptSetting>("SELECT * FROM transcript_settings LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(setting)

    }

    pub async fn save_transcript_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO transcript_settings (id, provider, model)
            VALUES ('1', $1, $2)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model
            "#,
        )
        .bind(provider)
        .bind(model)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn save_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let api_key_column = match provider {
            "localWhisper" => "whisperApiKey",
            "parakeet" => return Ok(()), // Parakeet doesn't need an API key, return early
            "deepgram" => "deepgramApiKey",
            "elevenLabs" => "elevenLabsApiKey",
            "groq" => "groqApiKey",
            "openai" => "openaiApiKey",
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        let query = format!(
            r#"
            INSERT INTO transcript_settings (id, provider, model, "{}")
            VALUES ('1', '{}', '{}', $1)
            ON CONFLICT(id) DO UPDATE SET
                "{}" = $1
            "#,
            api_key_column,
            crate::config::DEFAULT_TRANSCRIPT_PROVIDER,
            crate::config::DEFAULT_WHISPER_MODEL,
            api_key_column
        );
        secrets::store()
            .set(&transcript_name(api_key_column), api_key)
            .map_err(store_err)?;
        sqlx::query(&query).bind(None::<String>).execute(pool).await?;

        Ok(())
    }

    pub async fn get_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        Self::get_transcript_api_key_in(secrets::store(), pool, provider).await
    }

    async fn get_transcript_api_key_in(
        store: &dyn secrets::SecretStore,
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let api_key_column = match provider {
            // Local engines use no key; skip the store so an unreadable store can't break the config.
            "localWhisper" | "parakeet" => return Ok(None),
            "deepgram" => "deepgramApiKey",
            "elevenLabs" => "elevenLabsApiKey",
            "groq" => "groqApiKey",
            "openai" => "openaiApiKey",
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        let query = format!(
            "SELECT {} FROM transcript_settings WHERE id = '1' LIMIT 1",
            api_key_column
        );
        let plain: Option<String> = sqlx::query_scalar(&query).fetch_optional(pool).await?;
        read_or_plain(store, &transcript_name(api_key_column), plain).map_err(store_err)
    }

    pub async fn delete_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        Self::delete_api_key_in(secrets::store(), pool, provider).await
    }

    async fn delete_api_key_in(
        store: &dyn secrets::SecretStore,
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config - clear the entire config
        if provider == "custom-openai" {
            sqlx::query("UPDATE settings SET customOpenAIConfig = NULL WHERE id = '1'")
                .execute(pool)
                .await?;
            return store.delete(secrets::CUSTOM_OPENAI_API_KEY).map_err(store_err);
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "claude" => "anthropicApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(()), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        // Clear the plain column first so a failed UPDATE can't bring an old plain value back.
        let query = format!(
            "UPDATE settings SET {} = NULL WHERE id = '1'",
            api_key_column
        );
        sqlx::query(&query).execute(pool).await?;
        store.delete(api_key_column).map_err(store_err)
    }

    // ===== CUSTOM OPENAI CONFIG METHODS =====

    /// Gets the custom OpenAI configuration from JSON
    ///
    /// # Returns
    /// * `Ok(Some(CustomOpenAIConfig))` - Config exists and is valid JSON
    /// * `Ok(None)` - No config stored
    /// * `Err(sqlx::Error)` - Database error
    pub async fn get_custom_openai_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<CustomOpenAIConfig>, sqlx::Error> {
        use sqlx::Row;

        let row = sqlx::query(
            r#"
            SELECT customOpenAIConfig
            FROM settings
            WHERE id = '1'
            LIMIT 1
            "#
        )
        .fetch_optional(pool)
        .await?;

        match row {
            Some(record) => {
                let config_json: Option<String> = record.get("customOpenAIConfig");

                if let Some(json) = config_json {
                    // Parse JSON into CustomOpenAIConfig
                    let mut config: CustomOpenAIConfig = serde_json::from_str(&json)
                        .map_err(|e| sqlx::Error::Protocol(
                            format!("Invalid JSON in customOpenAIConfig: {}", e).into()
                        ))?;

                    config.api_key = read_or_plain(
                        secrets::store(),
                        secrets::CUSTOM_OPENAI_API_KEY,
                        config.api_key.take(),
                    )
                    .map_err(store_err)?;

                    Ok(Some(config))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Saves the custom OpenAI configuration as JSON
    ///
    /// # Arguments
    /// * `pool` - Database connection pool
    /// * `config` - CustomOpenAIConfig to save (includes endpoint, apiKey, model, maxTokens, temperature, topP)
    ///
    /// # Returns
    /// * `Ok(())` - Config saved successfully
    /// * `Err(sqlx::Error)` - Database or JSON serialization error
    pub async fn save_custom_openai_config(
        pool: &SqlitePool,
        config: &CustomOpenAIConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        match &config.api_key {
            Some(k) => secrets::store().set(secrets::CUSTOM_OPENAI_API_KEY, k),
            None => secrets::store().delete(secrets::CUSTOM_OPENAI_API_KEY),
        }
        .map_err(store_err)?;

        // The key lives in the store, never in the JSON.
        let mut plain_config = config.clone();
        plain_config.api_key = None;
        let config_json = serde_json::to_string(&plain_config)
            .map_err(|e| sqlx::Error::Protocol(
                format!("Failed to serialize config to JSON: {}", e).into()
            ))?;

        // Upsert into settings table
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, customOpenAIConfig)
            VALUES ('1', 'custom-openai', $1, 'large-v3', $2)
            ON CONFLICT(id) DO UPDATE SET
                customOpenAIConfig = excluded.customOpenAIConfig
            "#,
        )
        .bind(&config.model)
        .bind(config_json)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod default_transcript_tests {
    use super::*;
    use crate::config::{DEFAULT_TRANSCRIPT_PROVIDER, DEFAULT_WHISPER_MODEL};

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE transcript_settings (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                whisperApiKey TEXT,
                deepgramApiKey TEXT,
                elevenLabsApiKey TEXT,
                groqApiKey TEXT,
                openaiApiKey TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn api_key_upsert_on_empty_db_creates_whisper_default() {
        let pool = test_pool().await;
        SettingsRepository::save_transcript_api_key(&pool, "groq", "k").await.unwrap();

        let config = SettingsRepository::get_transcript_config(&pool).await.unwrap().unwrap();
        assert_eq!(config.provider, DEFAULT_TRANSCRIPT_PROVIDER);
        assert_eq!(config.model, DEFAULT_WHISPER_MODEL);
    }

    #[tokio::test]
    async fn api_key_upsert_keeps_saved_provider() {
        let pool = test_pool().await;
        SettingsRepository::save_transcript_config(&pool, "parakeet", "parakeet-tdt-0.6b-v3-int8")
            .await
            .unwrap();
        SettingsRepository::save_transcript_api_key(&pool, "localWhisper", "k").await.unwrap();

        let config = SettingsRepository::get_transcript_config(&pool).await.unwrap().unwrap();
        assert_eq!(config.provider, "parakeet");
        assert_eq!(config.model, "parakeet-tdt-0.6b-v3-int8");
    }
}

#[cfg(test)]
mod secret_tests {
    use super::*;
    use crate::database::repositories::test_pool;

    async fn plain(pool: &SqlitePool, table: &str, col: &str) -> Option<String> {
        sqlx::query_scalar(&format!("SELECT {col} FROM {table} WHERE id = '1'"))
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn save_api_key_goes_to_store_not_column() {
        let pool = test_pool().await;
        SettingsRepository::save_api_key(&pool, "openai", "sk-1").await.unwrap();
        assert_eq!(plain(&pool, "settings", "openaiApiKey").await, None);
        assert_eq!(secrets::store().get("openaiApiKey").unwrap().as_deref(), Some("sk-1"));
        let got = SettingsRepository::get_api_key(&pool, "openai").await.unwrap();
        assert_eq!(got.as_deref(), Some("sk-1"));
    }

    #[tokio::test]
    async fn get_api_key_falls_back_to_plain_column() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO settings (id, provider, model, whisperModel, groqApiKey) VALUES ('1','openai','m','w','old')")
            .execute(&pool).await.unwrap();
        let got = SettingsRepository::get_api_key(&pool, "groq").await.unwrap();
        assert_eq!(got.as_deref(), Some("old"));
        SettingsRepository::save_api_key(&pool, "groq", "new").await.unwrap();
        let got = SettingsRepository::get_api_key(&pool, "groq").await.unwrap();
        assert_eq!(got.as_deref(), Some("new"));
    }

    #[tokio::test]
    async fn transcript_and_summary_keys_are_distinct() {
        let pool = test_pool().await;
        SettingsRepository::save_api_key(&pool, "groq", "sum").await.unwrap();
        SettingsRepository::save_transcript_api_key(&pool, "groq", "tr").await.unwrap();
        let s = SettingsRepository::get_api_key(&pool, "groq").await.unwrap();
        let t = SettingsRepository::get_transcript_api_key(&pool, "groq").await.unwrap();
        assert_eq!(s.as_deref(), Some("sum"));
        assert_eq!(t.as_deref(), Some("tr"));
        assert_eq!(plain(&pool, "transcript_settings", "groqApiKey").await, None);
    }

    #[tokio::test]
    async fn local_whisper_key_goes_to_store() {
        let pool = test_pool().await;
        SettingsRepository::save_transcript_api_key(&pool, "localWhisper", "w1").await.unwrap();
        let name = transcript_name("whisperApiKey");
        assert_eq!(name, "transcript.whisperApiKey");
        assert_eq!(secrets::store().get(&name).unwrap().as_deref(), Some("w1"));
        // Local Whisper uses no key, so reads skip the store.
        let got = SettingsRepository::get_transcript_api_key(&pool, "localWhisper").await.unwrap();
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn local_whisper_read_ignores_failing_store_but_cloud_errors() {
        let pool = test_pool().await;
        let f = &secrets::FailingStore;
        let got = SettingsRepository::get_transcript_api_key_in(f, &pool, "localWhisper").await;
        assert_eq!(got.unwrap(), None);
        assert!(SettingsRepository::get_transcript_api_key_in(f, &pool, "groq").await.is_err());
    }

    #[tokio::test]
    async fn delete_clears_column_even_when_store_fails() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO settings (id, provider, model, whisperModel, openaiApiKey) VALUES ('1','openai','m','w','plainkey')")
            .execute(&pool).await.unwrap();
        let r = SettingsRepository::delete_api_key_in(&secrets::FailingStore, &pool, "openai").await;
        assert!(r.is_err());
        assert_eq!(plain(&pool, "settings", "openaiApiKey").await, None);
    }

    #[tokio::test]
    async fn delete_api_key_removes_store_and_column() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO settings (id, provider, model, whisperModel, openaiApiKey) VALUES ('1','openai','m','w','plainkey')")
            .execute(&pool).await.unwrap();
        SettingsRepository::save_api_key(&pool, "openai", "k").await.unwrap();
        SettingsRepository::delete_api_key(&pool, "openai").await.unwrap();
        assert_eq!(secrets::store().get("openaiApiKey").unwrap(), None);
        assert_eq!(plain(&pool, "settings", "openaiApiKey").await, None);
        assert_eq!(SettingsRepository::get_api_key(&pool, "openai").await.unwrap(), None);
    }

    fn cfg(key: Option<&str>) -> CustomOpenAIConfig {
        CustomOpenAIConfig {
            endpoint: "http://x/v1".into(),
            api_key: key.map(Into::into),
            model: "m".into(),
            max_tokens: Some(5),
            temperature: Some(0.5),
            top_p: Some(0.9),
        }
    }

    #[tokio::test]
    async fn custom_openai_key_lives_in_store() {
        let pool = test_pool().await;
        SettingsRepository::save_custom_openai_config(&pool, &cfg(Some("c"))).await.unwrap();
        let raw = plain(&pool, "settings", "customOpenAIConfig").await.unwrap();
        assert!(!raw.contains("\"c\""));
        let got = SettingsRepository::get_custom_openai_config(&pool).await.unwrap().unwrap();
        assert_eq!(got.api_key.as_deref(), Some("c"));
        assert_eq!((got.endpoint.as_str(), got.model.as_str(), got.max_tokens), ("http://x/v1", "m", Some(5)));

        SettingsRepository::save_custom_openai_config(&pool, &cfg(None)).await.unwrap();
        assert_eq!(secrets::store().get(secrets::CUSTOM_OPENAI_API_KEY).unwrap(), None);

        SettingsRepository::save_custom_openai_config(&pool, &cfg(Some("c"))).await.unwrap();
        SettingsRepository::delete_api_key(&pool, "custom-openai").await.unwrap();
        assert_eq!(secrets::store().get(secrets::CUSTOM_OPENAI_API_KEY).unwrap(), None);
    }

    #[tokio::test]
    async fn over_limit_key_is_rejected_and_keeps_old() {
        let pool = test_pool().await;
        SettingsRepository::save_api_key(&pool, "openai", "k1").await.unwrap();
        let err = SettingsRepository::save_api_key(&pool, "openai", &"x".repeat(2000))
            .await
            .unwrap_err()
            .to_string();
        assert!(!err.contains("xxxx"));
        let got = SettingsRepository::get_api_key(&pool, "openai").await.unwrap();
        assert_eq!(got.as_deref(), Some("k1"));
    }
}
