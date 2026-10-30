use crate::database::repositories::{
    meeting::MeetingsRepository, summary::SummaryProcessesRepository,
};
use crate::summary::llm_client::{generate_summary, LLMProvider, TokenUsage};
use crate::summary::language_choice::{resolve_summary_language, LanguageChoice};
use crate::summary::language_detection::detect_summary_language;
use crate::summary::llm_target::{resolve_llm_target, LlmTarget};
use crate::summary::metadata::{
    read_detected_summary_language_from_metadata, read_template_choice_from_metadata,
    write_template_choice_to_metadata,
};
use crate::summary::processor::{
    clean_llm_markdown_detailed, extract_meeting_name_from_markdown, generate_meeting_summary,
    chunk_prompt_overhead_tokens, require_visible_markdown, rough_token_count, MeetingInfo,
};
use crate::summary::template_choice::{
    auto_pick_answer_id, auto_pick_prompts, choice_to_store, choose_without_model, Choice,
    TemplateSource,
};
use crate::summary::templates;
use crate::usage::{self, Purpose};
use crate::ollama::metadata::ModelMetadataCache;
use chrono::{DateTime, Local, TimeZone, Utc};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

static METADATA_CACHE: LazyLock<ModelMetadataCache> =
    LazyLock::new(|| ModelMetadataCache::new(Duration::from_secs(300)));

#[derive(Clone)]
struct RegisteredCancellation {
    started_at: DateTime<Utc>,
    token: CancellationToken,
}

static CANCELLATION_REGISTRY: LazyLock<Arc<Mutex<HashMap<String, RegisteredCancellation>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Output tokens kept free in Ollama's context so a chunk's summary is not cut off.
const OLLAMA_REPLY_TOKENS: usize = 1024;

/// Chunk size for Ollama. Ollama's default runtime context is 4096 tokens and requests never send num_ctx to raise it.
/// shortcut: a fixed 4096 cap, not the server's OLLAMA_CONTEXT_LENGTH or per-request num_ctx; upgrade if those become configurable.
fn ollama_chunk_tokens(context_size: usize) -> usize {
    context_size
        .min(4096)
        .saturating_sub(chunk_prompt_overhead_tokens() + OLLAMA_REPLY_TOKENS)
}

/// Chunk size when the model's context cannot be fetched: Ollama's default runtime context.
fn ollama_fallback_chunk_tokens() -> usize {
    ollama_chunk_tokens(4096)
}

fn prep_material_budget(token_threshold: usize, transcript: &str) -> usize {
    token_threshold
        .saturating_sub(rough_token_count(transcript))
        .saturating_mul(4)
}

/// Strips the first `#` heading line; returns "" if no `#` is found.
fn strip_leading_title(markdown: &str) -> String {
    if let Some(hash_pos) = markdown.find('#') {
        let body_start = markdown[hash_pos..]
            .find('\n')
            .map_or(markdown.len(), |line_end| hash_pos + line_end);
        markdown[body_start..].trim_start().to_string()
    } else {
        String::new()
    }
}

/// Strips the leading H1 (`# Title\n...`) only when the markdown starts with one.
/// No-op on already-stripped values, values starting with `## Subheading`, or values
/// without any heading. Avoids the silent-empty-return case where `strip_leading_title`
/// returns "" for input lacking a leading `#`.
fn strip_title_if_present(markdown: &str) -> String {
    if markdown.trim_start().starts_with("# ") {
        strip_leading_title(markdown)
    } else {
        markdown.to_string()
    }
}

/// One row per summary call that got a response. Called after the summary, with no lock held.
async fn record_summary_usage(
    pool: &SqlitePool,
    provider: &LLMProvider,
    model: &str,
    meeting_id: &str,
    calls: &[TokenUsage],
) {
    for call in calls {
        usage::record(pool, provider.as_str(), model, Purpose::Summary, Some(meeting_id), None, Some(call)).await;
    }
}

fn build_summary_result_json(
    final_markdown: &str,
    reasoning_stripped: bool,
    language: &LanguageChoice,
    template_id: &str,
    template_name: &str,
    template_source: TemplateSource,
) -> Result<serde_json::Value, String> {
    let cleaned = clean_llm_markdown_detailed(final_markdown);
    require_visible_markdown("Final summary", &cleaned)?;
    let markdown = strip_title_if_present(&cleaned.markdown);
    if markdown.trim().is_empty() {
        return Err("Final summary contains no visible content after title removal".to_string());
    }
    Ok(serde_json::json!({
        "markdown": markdown,
        "generated_markdown": markdown,
        "reasoning_stripped": reasoning_stripped || cleaned.reasoning_stripped,
        "summary_language": language.code,
        "language_fallback": language.fallback,
        "template_id": template_id,
        "template_name": template_name,
        "template_source": template_source,
    }))
}

/// Summary service - handles all summary generation logic
pub struct SummaryService;

impl SummaryService {
    /// Registers a new cancellation token for a meeting.
    pub(crate) fn register_cancellation_token(
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> CancellationToken {
        let token = CancellationToken::new();
        let mut registry = CANCELLATION_REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(previous) = registry.insert(
            meeting_id.to_string(),
            RegisteredCancellation {
                started_at,
                token: token.clone(),
            },
        ) {
            previous.token.cancel();
        }
        info!("Registered cancellation token for meeting: {}", meeting_id);
        token
    }

    /// Cancels only the active generation identified by `started_at`.
    pub fn cancel_summary(meeting_id: &str, started_at: DateTime<Utc>) -> bool {
        let registry = CANCELLATION_REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = registry.get(meeting_id).filter(|entry| entry.started_at == started_at)
        {
            info!("Cancelling summary generation for meeting: {}", meeting_id);
            entry.token.cancel();
            true
        } else {
            warn!("No active summary generation found for meeting: {}", meeting_id);
            false
        }
    }

    /// Cleans up only the matching generation token after processing completes.
    fn cleanup_cancellation_token(meeting_id: &str, started_at: DateTime<Utc>) {
        let mut registry = CANCELLATION_REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if registry
            .get(meeting_id)
            .is_some_and(|entry| entry.started_at == started_at)
        {
            registry.remove(meeting_id);
            info!("Cleaned up cancellation token for meeting: {}", meeting_id);
        }
    }

    async fn read_detected_summary_language(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Option<String> {
        let meeting = match MeetingsRepository::get_meeting_metadata(pool, meeting_id).await {
            Ok(Some(meeting)) => meeting,
            Ok(None) => {
                warn!("Meeting not found while reading detected summary language: {}", meeting_id);
                return None;
            }
            Err(e) => {
                warn!(
                    "Failed to read meeting metadata for detected summary language (meeting_id={}): {}",
                    meeting_id, e
                );
                return None;
            }
        };

        let Some(folder_path) = meeting.folder_path.filter(|p| !p.trim().is_empty()) else {
            return None;
        };

        match read_detected_summary_language_from_metadata(Path::new(&folder_path)) {
            Ok(language) => language,
            Err(e) => {
                warn!(
                    "Failed to read detected summary language metadata for meeting_id={}: {}",
                    meeting_id, e
                );
                None
            }
        }
    }

    fn detect_summary_language_from_text(text: &str) -> Option<String> {
        let transcript_texts = [text.to_string()];
        let detection = detect_summary_language(&transcript_texts);
        match &detection.language {
            Some(language) => {
                info!("Detected transcript summary language: {}", language);
            }
            None => {
                info!(
                    "Transcript summary language unknown: {:?}",
                    detection.reason
                );
            }
        }
        detection.language
    }

    /// Processes transcript in the background and generates summary
    ///
    /// This function is designed to be spawned as an async task and does not block
    /// the main thread. It updates the database with progress and results.
    ///
    /// # Arguments
    /// * `_app` - Tauri app handle (for future use)
    /// * `pool` - SQLx connection pool
    /// * `meeting_id` - Unique identifier for the meeting
    /// * `text` - Full transcript text
    /// * `model_provider` - LLM provider name (e.g., "ollama", "openai")
    /// * `model_name` - Specific model (e.g., "gpt-4", "llama3.2:latest")
    /// * `custom_prompt` - Optional user-provided context
    /// * `manual_template_id` - Template the user picked for this run; None lets Conclave choose
    pub async fn process_transcript_background<R: tauri::Runtime>(
        _app: AppHandle<R>,
        pool: SqlitePool,
        meeting_id: String,
        started_at: DateTime<Utc>,
        cancellation_token: CancellationToken,
        text: String,
        model_provider: String,
        model_name: String,
        custom_prompt: String,
        manual_template_id: Option<String>,
        summary_language: Option<String>,
    ) {
        let start_time = Instant::now();
        info!(
            "Starting background processing for meeting_id: {}",
            meeting_id
        );

        // Provider, key and endpoints from the saved settings
        let LlmTarget {
            provider,
            api_key: final_api_key,
            ollama_endpoint,
            custom_openai_endpoint,
            max_tokens: custom_openai_max_tokens,
            temperature: custom_openai_temperature,
            top_p: custom_openai_top_p,
            ..
        } = match resolve_llm_target(&pool, &model_provider, &model_name).await {
            Ok(target) => target,
            Err(err) => {
                Self::fail_and_cleanup(&pool, &meeting_id, started_at, &err).await;
                return;
            }
        };

        // Dynamically fetch context size based on provider and model
        let token_threshold = if provider == LLMProvider::Ollama {
            match METADATA_CACHE.get_or_fetch(&model_name, ollama_endpoint.as_deref()).await {
                Ok(metadata) => {
                    let optimal = ollama_chunk_tokens(metadata.context_size);
                    info!(
                        "✓ Using dynamic context for {}: {} tokens (chunk size: {})",
                        model_name, metadata.context_size, optimal
                    );
                    optimal
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch context for {}: {}. Using default 4000",
                        model_name, e
                    );
                    ollama_fallback_chunk_tokens()
                }
            }
        } else if provider == LLMProvider::BuiltInAI {
            // Get model's context size from registry
            use crate::summary::summary_engine::models;
            let model = models::get_model_by_name(&model_name)
                .ok_or_else(|| format!("Unknown model: {}", model_name));

            match model {
                Ok(model_def) => {
                    // Reserve 300 tokens for prompt overhead
                    let optimal = model_def.context_size.saturating_sub(300).max(1) as usize;
                    info!(
                        "✓ Using BuiltInAI context size: {} tokens (chunk size: {})",
                        model_def.context_size, optimal
                    );
                    optimal
                }
                Err(e) => {
                    warn!("{}, using default 2048", e);
                    1748  // 2048 - 300 for overhead
                }
            }
        } else {
            // Cloud providers (OpenAI, Claude, Groq, CustomOpenAI) handle large contexts automatically
            100000  // Effectively unlimited for single-pass processing
        };

        // Get app data directory for BuiltInAI provider
        let app_data_dir = _app.path().app_data_dir().ok();

        if let Some(code) = &summary_language {
            info!("📝 Summary language preference: {}", code);
        }

        let detected_summary_language =
            Self::read_detected_summary_language(&pool, &meeting_id)
                .await
                .or_else(|| Self::detect_summary_language_from_text(&text));

        if let Some(code) = &detected_summary_language {
            info!("📝 Detected transcript summary language: {}", code);
        }

        let client = reqwest::Client::new();
        let meeting = match MeetingsRepository::get_meeting_metadata(&pool, &meeting_id).await {
            Ok(meeting) => meeting,
            Err(_) => {
                warn!("Could not read meeting {}", meeting_id);
                None
            }
        };
        let title_source = MeetingsRepository::get_title_source(&pool, &meeting_id)
            .await
            .unwrap_or_else(|e| {
                warn!("Could not read title source for meeting {}: {}", meeting_id, e);
                None
            });
        let user_title_cutoff = MeetingsRepository::title_source_installed_on(&pool).await.ok().flatten();
        if user_title_cutoff.is_none() {
            warn!("Could not read the title migration time for meeting {}", meeting_id);
        }
        let meeting_info = meeting.as_ref().map(|m| {
            meeting_info_for_prompt(&m.title, title_source.as_deref(), m.created_at.0, user_title_cutoff)
        });
        let title = meeting.as_ref().map(|m| m.title.clone()).unwrap_or_default();
        let folder = meeting.and_then(|m| m.folder_path).filter(|p| !p.trim().is_empty()).map(PathBuf::from);
        let stored = folder.as_deref().and_then(|f| read_template_choice_from_metadata(f).ok().flatten());
        let prep = crate::prep::store::for_meeting(&pool, &meeting_id).await.unwrap_or_else(|_| {
            warn!("Could not read prep for meeting {}", meeting_id);
            None
        });
        let listing = templates::list_templates();
        let (template_id, source) = match choose_without_model(manual_template_id.as_deref(), stored, prep.as_ref().and_then(|p| p.template_id.as_deref()), &title, &listing.templates) {
            Choice::Decided(id, source) => (id, source),
            Choice::AskModel => {
                let (system, user) = auto_pick_prompts(&text, &listing.templates);
                // Only a custom endpoint has ever received this temperature; other providers keep the default.
                let temperature = if provider == LLMProvider::CustomOpenAI { Some(0.0) } else { None };
                let answer = tokio::time::timeout(
                    Duration::from_secs(30),
                    generate_summary(&client, &provider, &model_name, &final_api_key, &system, &user,
                        ollama_endpoint.as_deref(), custom_openai_endpoint.as_deref(), custom_openai_max_tokens,
                        temperature, custom_openai_top_p, app_data_dir.as_ref(), Some(&cancellation_token)),
                ).await;
                if let Ok(Ok(c)) = &answer {
                    usage::record(&pool, provider.as_str(), &model_name, Purpose::TemplatePick, Some(&meeting_id), None, c.usage.as_ref()).await;
                }
                match answer {
                    Ok(Ok(c)) if c.truncated => {
                        warn!("Auto-pick answer was cut off at the output limit");
                        (templates::STANDARD_TEMPLATE_ID.to_string(), TemplateSource::Fallback)
                    }
                    Ok(Ok(c)) => match auto_pick_answer_id(&c.content, &listing.templates) {
                        Some(id) => (id, TemplateSource::Auto),
                        None => {
                            warn!("Auto-pick answer is not a template id: {} chars", c.content.chars().count());
                            (templates::STANDARD_TEMPLATE_ID.to_string(), TemplateSource::Fallback)
                        }
                    },
                    Ok(Err(e)) => {
                        warn!("Auto-pick failed: {}", e);
                        (templates::STANDARD_TEMPLATE_ID.to_string(), TemplateSource::Fallback)
                    }
                    Err(_) => {
                        warn!("Auto-pick timed out");
                        (templates::STANDARD_TEMPLATE_ID.to_string(), TemplateSource::Fallback)
                    }
                }
            }
        };
        let (template_id, source, template) = match templates::get_template(&template_id) {
            Ok(template) => (template_id, source, template),
            Err(e) => {
                warn!("Template '{}' unusable ({}); using Standard", template_id, e);
                let standard = templates::get_template(templates::STANDARD_TEMPLATE_ID)
                    .expect("standard template has an embedded fallback");
                (templates::STANDARD_TEMPLATE_ID.to_string(), TemplateSource::Fallback, standard)
            }
        };
        if cancellation_token.is_cancelled() {
            Self::update_process_cancelled(&pool, &meeting_id, started_at).await;
            Self::cleanup_cancellation_token(&meeting_id, started_at);
            return;
        }
        if let Some(f) = folder.as_deref().filter(|_| choice_to_store(source, cancellation_token.is_cancelled())) {
            if let Err(e) = write_template_choice_to_metadata(f, Some((&template_id, source))) {
                warn!("Could not store the template choice: {}", e);
            }
        }

        let language = resolve_summary_language(
            summary_language.as_deref(),
            template.fixed_language(),
            detected_summary_language.as_deref(),
        );
        info!("📝 Summary language: {} ({}){}", language.name, language.code, if language.fallback { ", fallback" } else { "" });

        let notes = crate::database::repositories::meeting_notes::MeetingNotesRepository::get(&pool, &meeting_id)
            .await
            .unwrap_or_else(|e| {
                warn!("Could not read notes for {}: {}", meeting_id, e);
                Vec::new()
            });
        let notes_block = crate::summary::user_notes::render_notes_block(&notes);
        let prep_block = prep.as_ref().and_then(|p| {
            crate::summary::user_notes::render_prep_block(p, prep_material_budget(token_threshold, &text))
        });
        let dictionary = crate::dictate::settings::current().dictionary;

        let mut calls = Vec::new();
        let result = generate_meeting_summary(
            &client,
            &provider,
            &model_name,
            &final_api_key,
            &text,
            &custom_prompt,
            notes_block.as_deref(),
            prep_block.as_deref(),
            meeting_info.as_ref(),
            &dictionary,
            &template,
            language.name,
            token_threshold,
            ollama_endpoint.as_deref(),
            custom_openai_endpoint.as_deref(),
            custom_openai_max_tokens,
            custom_openai_temperature,
            custom_openai_top_p,
            app_data_dir.as_ref(),
            Some(&cancellation_token),
            &mut calls,
        )
        .await;
        record_summary_usage(&pool, &provider, &model_name, &meeting_id, &calls).await;

        let duration = start_time.elapsed().as_secs_f64();

        match result {
            Ok(generated) => {
                info!(
                    "✓ Successfully processed {} chunks for meeting_id: {}. Duration: {:.2}s",
                    generated.successful_chunk_count, meeting_id, duration
                );
                let result_json = match build_summary_result_json(
                    &generated.final_markdown,
                    generated.reasoning_stripped,
                    &language,
                    &template_id,
                    &template.name,
                    source,
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        Self::update_process_failed(&pool, &meeting_id, started_at, &error).await;
                        Self::cleanup_cancellation_token(&meeting_id, started_at);
                        return;
                    }
                };

                // The name renames the meeting in the completion's transaction: see update_process_completed.
                let summary_name = extract_meeting_name_from_markdown(&generated.final_markdown)
                    .filter(|name| !name.is_empty());
                match SummaryProcessesRepository::update_process_completed(
                    &pool,
                    &meeting_id,
                    started_at,
                    result_json,
                    generated.successful_chunk_count,
                    duration,
                    summary_name.as_deref(),
                )
                .await
                {
                    Ok(true) => {
                        if crate::knowledge::settings::current().extract_enabled {
                            crate::knowledge::extract::enqueue(&pool, &meeting_id).await;
                        }
                        info!("Summary saved successfully for meeting_id: {}", meeting_id);
                    }
                    Ok(false) => warn!("Skipped stale summary completion for meeting_id: {}", meeting_id),
                    Err(error) => error!(
                        "Failed to save completed process for {}: {}",
                        meeting_id, error
                    ),
                }
            }
            Err(error) if cancellation_token.is_cancelled() => {
                Self::update_process_cancelled(&pool, &meeting_id, started_at).await;
            }
            Err(error) => {
                Self::update_process_failed(&pool, &meeting_id, started_at, &error).await;
            }
        }
        Self::cleanup_cancellation_token(&meeting_id, started_at);
    }

    /// Updates the summary process status to failed with error message
    async fn fail_and_cleanup(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        error_msg: &str,
    ) {
        Self::update_process_failed(pool, meeting_id, started_at, error_msg).await;
        Self::cleanup_cancellation_token(meeting_id, started_at);
    }

    async fn update_process_cancelled(pool: &SqlitePool, meeting_id: &str, started_at: DateTime<Utc>) {
        match SummaryProcessesRepository::update_process_cancelled(pool, meeting_id, started_at).await {
            Ok(false) => warn!("Skipped stale summary cancellation for meeting_id: {}", meeting_id),
            Ok(true) => info!("Summary generation was cancelled for meeting_id: {}", meeting_id),
            Err(db_error) => error!(
                "Failed to update DB status to cancelled for {}: {}",
                meeting_id, db_error
            ),
        }
    }

    async fn update_process_failed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        error_msg: &str,
    ) {
        error!(
            "Processing failed for meeting_id {}: {}",
            meeting_id, error_msg
        );
        match SummaryProcessesRepository::update_process_failed(
            pool,
            meeting_id,
            started_at,
            error_msg,
        )
        .await
        {
            Ok(false) => warn!("Skipped stale summary failure for meeting_id: {}", meeting_id),
            Ok(true) => {}
            Err(e) => error!(
                "Failed to update DB status to failed for {}: {}",
                meeting_id, e
            ),
        }
    }


}

pub(crate) fn meeting_info_for_prompt(
    title: &str,
    title_source: Option<&str>,
    created_at: DateTime<Utc>,
    user_title_cutoff: Option<DateTime<Utc>>,
) -> MeetingInfo {
    let title = title.trim();
    MeetingInfo {
        title: match title_source {
            Some("calendar") if !title.is_empty() => Some(title.to_string()),
            // Existing rows default to user when the migration ran, so an older user title is unknown.
            Some("user") if !title.is_empty()
                && user_title_cutoff.map_or(true, |cutoff| created_at >= cutoff) => Some(title.to_string()),
            _ => None,
        },
        date: created_at.with_timezone(&Local).format("%Y-%m-%d").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::{test_pool, transcript::TranscriptsRepository};
    use crate::summary::llm_client::TokenUsage;

    #[test]
    fn meeting_info_for_prompt_sends_only_calendar_and_user_titles() {
        let at = Local.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap().with_timezone(&Utc);
        for (title, source, expected) in [
            (" X ", Some("calendar"), Some("X")),
            (" X ", Some("user"), Some("X")),
            (" X ", Some("default"), None),
            (" X ", Some("summary"), None),
            (" X ", None, None),
            ("  ", Some("user"), None),
        ] {
            let info = meeting_info_for_prompt(title, source, at, None);
            assert_eq!(info.title.as_deref(), expected);
            assert_eq!(info.date, "2026-10-29");
        }
    }

    #[tokio::test]
    async fn user_title_before_the_title_migration_is_unknown() {
        let pool = test_pool().await;
        let cutoff = MeetingsRepository::title_source_installed_on(&pool).await.unwrap().unwrap();
        let old = cutoff - chrono::Duration::days(1);
        let new = cutoff + chrono::Duration::days(1);

        let info = meeting_info_for_prompt(" X ", Some("user"), old, Some(cutoff));
        assert_eq!(info.title, None);
        assert_eq!(info.date, old.with_timezone(&Local).format("%Y-%m-%d").to_string());
        assert_eq!(
            meeting_info_for_prompt(" X ", Some("user"), new, Some(cutoff)).title.as_deref(),
            Some("X")
        );
        assert_eq!(
            meeting_info_for_prompt(" X ", Some("calendar"), old, Some(cutoff)).title.as_deref(),
            Some("X")
        );
        assert_eq!(
            meeting_info_for_prompt(" X ", Some("user"), old, None).title.as_deref(),
            Some("X")
        );
    }

    #[test]
    fn prep_material_budget_uses_only_tokens_left_after_transcript() {
        assert_eq!(prep_material_budget(100, ""), 400);
        assert_eq!(prep_material_budget(100, &"я".repeat(100)), 260);
        assert_eq!(prep_material_budget(1, "я"), 0);
        assert_eq!(prep_material_budget(35, &"a".repeat(100)), 0);
        assert_eq!(prep_material_budget(10, &"a".repeat(100)), 0);
        assert_eq!(prep_material_budget(0, ""), 0);
    }

    #[test]
    fn ollama_fallback_chunks_leave_room_for_the_reply() {
        let chunk = ollama_fallback_chunk_tokens();
        assert!(chunk + chunk_prompt_overhead_tokens() + OLLAMA_REPLY_TOKENS <= 4096);
    }

    #[test]
    fn ollama_chunks_leave_room_for_the_reply_in_the_default_context() {
        // Prompt and reply share the context: chunk + instructions + 1024 reply tokens must fit in 4096.
        for context in [2_048, 4_096, 131_072] {
            let chunk = ollama_chunk_tokens(context);
            assert!(chunk + chunk_prompt_overhead_tokens() + 1024 <= context.min(4096));
        }
        assert_eq!(ollama_chunk_tokens(200), 0);
    }

    #[tokio::test]
    async fn summary_usage_is_recorded_for_each_call() {
        let pool = test_pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(&pool, "M", &[], None, &[], false, &crate::database::repositories::meeting::TitleOrigin::default())
            .await
            .unwrap();
        let calls = [
            TokenUsage { input: Some(1), output: Some(2), ..TokenUsage::default() },
            TokenUsage { output: Some(3), ..TokenUsage::default() },
        ];
        record_summary_usage(&pool, &LLMProvider::Claude, "claude-sonnet-5-5", &meeting_id, &calls).await;
        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT output_tokens FROM llm_usage WHERE meeting_id = ? AND purpose = 'summary' ORDER BY id",
        )
        .bind(&meeting_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows, vec![(2,), (3,)]);
    }

    #[test]
    fn stale_cleanup_keeps_the_replacement_cancellation_token() {
        let meeting_id = format!("summary-test-{}", uuid::Uuid::new_v4());
        let first_started_at = Utc::now();
        let second_started_at = first_started_at + chrono::Duration::nanoseconds(1);
        let first = SummaryService::register_cancellation_token(&meeting_id, first_started_at);
        let second = SummaryService::register_cancellation_token(&meeting_id, second_started_at);

        assert!(first.is_cancelled());
        SummaryService::cleanup_cancellation_token(&meeting_id, first_started_at);
        assert!(SummaryService::cancel_summary(&meeting_id, second_started_at));
        assert!(second.is_cancelled());
        SummaryService::cleanup_cancellation_token(&meeting_id, second_started_at);
    }

    #[test]
    fn test_strip_leading_title_with_body() {
        let input = "# Meeting Title\nThis is the body.\nMore content.";
        let result = strip_leading_title(input);
        assert_eq!(result, "This is the body.\nMore content.");
    }

    #[test]
    fn test_strip_leading_title_only() {
        let input = "# Meeting Title";
        let result = strip_leading_title(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_leading_title_no_heading() {
        let input = "No heading here.\nJust body.";
        let result = strip_leading_title(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_leading_title_multiline_body() {
        let input = "# Title\n## Subheading\nParagraph 1\n\nParagraph 2";
        let result = strip_leading_title(input);
        assert_eq!(result, "## Subheading\nParagraph 1\n\nParagraph 2");
    }

    #[test]
    fn test_strip_leading_title_empty_after_heading() {
        let input = "# Title\n";
        let result = strip_leading_title(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_leading_title_whitespace_after_heading() {
        let input = "# Title\n   \n Body with leading spaces";
        let result = strip_leading_title(input);
        assert_eq!(result, "Body with leading spaces");
    }

    #[test]
    fn test_strip_title_if_present_preserves_already_stripped() {
        assert_eq!(strip_title_if_present("## Action Items\nfoo"), "## Action Items\nfoo");
    }

    #[test]
    fn test_strip_title_if_present_strips_leading_h1() {
        assert_eq!(strip_title_if_present("# Meeting Title\n## Action Items\nfoo"), "## Action Items\nfoo");
    }

    #[test]
    fn test_strip_title_if_present_no_heading_preserved() {
        // Distinct from strip_leading_title which returns "" — this preserves input.
        assert_eq!(strip_title_if_present("Just body text"), "Just body text");
    }

    #[test]
    fn test_strip_title_if_present_hash_no_space_preserved() {
        // `#NoSpace` is not a markdown H1 — preserve.
        assert_eq!(strip_title_if_present("#NoSpace\nbody"), "#NoSpace\nbody");
    }

    #[test]
    fn test_strip_title_if_present_mid_document_h1_preserved() {
        // H1 after body content must NOT be stripped — guards the asymmetry where
        // extract_meeting_name_from_markdown scans every line for "# ".
        let input = "Some paragraph\n\n# H1 on line 3\n## Section\nbody";
        assert_eq!(strip_title_if_present(input), input);
    }

    #[test]
    fn test_strip_title_if_present_leading_whitespace_h1_stripped() {
        assert_eq!(
            strip_title_if_present("  # Title\n## Section\nbody"),
            "## Section\nbody"
        );
    }

    fn english_choice(fallback: bool) -> LanguageChoice {
        LanguageChoice { code: "en".into(), name: "English", fallback }
    }

    fn result_json(markdown: &str, reasoning_stripped: bool, language: &LanguageChoice) -> Result<serde_json::Value, String> {
        build_summary_result_json(markdown, reasoning_stripped, language, "standard_meeting", "Standard Meeting Notes", TemplateSource::Fallback)
    }

    #[test]
    fn test_result_json_strips_display_markdown_title() {
        let result = result_json("# Translated Title\n## Decisions\nDone", false, &english_choice(false)).unwrap();

        assert_eq!(result["markdown"], "## Decisions\nDone");
    }

    #[test]
    fn result_json_stores_generated_markdown() {
        let v = result_json("# T\n## A\nx", false, &english_choice(false)).unwrap();
        assert_eq!(v["generated_markdown"], v["markdown"]);
        assert_eq!(v["generated_markdown"], "## A\nx");
    }

    #[test]
    fn result_json_rejects_title_only_display_markdown() {
        assert_eq!(
            result_json("# Title", false, &english_choice(false)),
            Err("Final summary contains no visible content after title removal".to_string())
        );
    }

    #[test]
    fn result_json_never_persists_reasoning_text() {
        let result = result_json("# Title\nVisible", true, &english_choice(false)).unwrap();
        assert_eq!(result["reasoning_stripped"], true);
        assert!(result.get("reasoning").is_none());
    }

    #[test]
    fn result_json_carries_language_and_fallback_flag() {
        let lang = LanguageChoice { code: "en".into(), name: "English", fallback: true };
        let v = result_json("# Title\n\nBody", false, &lang).unwrap();
        assert_eq!(v["summary_language"], "en");
        assert_eq!(v["language_fallback"], true);
        assert!(v.get("english_cache").is_none() && v.get("normalization_fallback").is_none());
    }

    #[test]
    fn result_json_carries_template_id_name_and_source() {
        let v = build_summary_result_json(
            "# Title\n\nBody",
            false,
            &english_choice(false),
            "german_lessons",
            "German Lessons",
            TemplateSource::Keyword,
        )
        .unwrap();
        assert_eq!(v["template_id"], "german_lessons");
        assert_eq!(v["template_name"], "German Lessons");
        assert_eq!(v["template_source"], "keyword");
    }
}
