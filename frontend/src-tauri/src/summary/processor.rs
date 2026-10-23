use crate::summary::llm_client::{generate_summary, reject_truncated, LLMProvider, TokenUsage};
use crate::summary::templates::Template;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

static THINK_ENVELOPE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<think(?:ing)?(?:\s+[^>]*)?>.*?</think(?:ing)?\s*>").unwrap()
});
static THINK_MARKER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?is)</?think(?:ing)?(?:\s+[^>]*)?>").unwrap());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanedLlmMarkdown {
    pub markdown: String,
    pub reasoning_stripped: bool,
}

pub fn clean_llm_markdown_detailed(raw: &str) -> CleanedLlmMarkdown {
    let visible = THINK_ENVELOPE_REGEX.replace_all(raw, "");
    let reasoning_stripped = visible.as_ref() != raw;
    let trimmed = visible.trim();
    const PREFIXES: &[&str] = &["```markdown\n", "```\n", "```markdown\r\n", "```\r\n"];
    const SUFFIX: &str = "```";
    let markdown = PREFIXES
        .iter()
        .find_map(|prefix| {
            (trimmed.starts_with(prefix) && trimmed.ends_with(SUFFIX))
                .then(|| trimmed[prefix.len()..trimmed.len() - SUFFIX.len()].trim())
        })
        .unwrap_or(trimmed)
        .to_string();

    if THINK_MARKER_REGEX.is_match(&markdown) {
        warn!(
            raw_len = raw.len(),
            sanitized_len = markdown.len(),
            "LLM output contains an unterminated reasoning marker"
        );
    }

    CleanedLlmMarkdown {
        markdown,
        reasoning_stripped,
    }
}

pub(crate) fn contains_reasoning_marker(markdown: &str) -> bool {
    THINK_MARKER_REGEX.is_match(markdown)
}

pub fn require_visible_markdown(stage: &str, cleaned: &CleanedLlmMarkdown) -> Result<(), String> {
    if contains_reasoning_marker(&cleaned.markdown) {
        Err(format!(
            "{stage} contained an unterminated reasoning marker"
        ))
    } else if cleaned.markdown.is_empty() {
        Err(format!(
            "{stage} returned no visible summary content after reasoning removal"
        ))
    } else {
        Ok(())
    }
}

const MAX_CHUNK_ATTEMPTS: usize = 2;

fn should_retry_chunk_failure(
    attempt: usize,
    cancellation_token: Option<&CancellationToken>,
) -> bool {
    attempt < MAX_CHUNK_ATTEMPTS
        && !cancellation_token.is_some_and(CancellationToken::is_cancelled)
}

/// Maps a BCP-47 tag to the English language name used inside LLM prompts.
///
/// LLMs respond far more reliably to "in Spanish" than to "in es". Regional
/// tags (`pt-BR`, `en_GB`) are normalised to their base language; Chinese
/// variants are disambiguated. Unknown codes return None so the caller falls
/// back to English rather than injecting a literal ISO code into the prompt.
pub(crate) fn language_name_from_code(code: &str) -> Option<&'static str> {
    let normalised = code.to_ascii_lowercase().replace('_', "-");
    let lookup: &str = match normalised.as_str() {
        "zh-cn" => "zh",
        "zh-tw" => return Some("Traditional Chinese"),
        other => other.split('-').next().unwrap_or(other),
    };
    match lookup {
        "en" => Some("English"),
        "zh" => Some("Chinese"),
        "de" => Some("German"),
        "es" => Some("Spanish"),
        "ru" => Some("Russian"),
        "ko" => Some("Korean"),
        "fr" => Some("French"),
        "ja" => Some("Japanese"),
        "pt" => Some("Portuguese"),
        "it" => Some("Italian"),
        "nl" => Some("Dutch"),
        "pl" => Some("Polish"),
        "ar" => Some("Arabic"),
        "hi" => Some("Hindi"),
        "ta" => Some("Tamil"),
        "tr" => Some("Turkish"),
        "vi" => Some("Vietnamese"),
        "th" => Some("Thai"),
        "id" => Some("Indonesian"),
        "sv" => Some("Swedish"),
        "cs" => Some("Czech"),
        "da" => Some("Danish"),
        "fi" => Some("Finnish"),
        "el" => Some("Greek"),
        "he" => Some("Hebrew"),
        "hu" => Some("Hungarian"),
        "no" => Some("Norwegian"),
        "ro" => Some("Romanian"),
        "uk" => Some("Ukrainian"),
        _ => None,
    }
}

fn build_chunk_summary_user_prompt(chunk: &str, language_name: &str) -> String {
    format!(
        "Write in {language_name}.\n\nProvide a concise but comprehensive summary of the following transcript chunk. Capture all key points, decisions, action items, and mentioned individuals. Do not include reasoning, self-correction, or meta-commentary — output only the summary content.\n\n<transcript_chunk>\n{chunk}\n</transcript_chunk>"
    )
}

/// System prompt of the per-chunk call.
pub(crate) const CHUNK_SYSTEM_PROMPT: &str = "You are an expert meeting summarizer.";

/// Rough tokens the per-chunk call spends on instructions before the transcript, with the longest language name.
pub(crate) fn chunk_prompt_overhead_tokens() -> usize {
    rough_token_count(CHUNK_SYSTEM_PROMPT)
        + rough_token_count(&build_chunk_summary_user_prompt("", "Traditional Chinese"))
}

fn build_combine_summary_user_prompt(combined_text: &str, language_name: &str) -> String {
    format!(
        "Write in {language_name}.\n\nThe following are consecutive summaries of a meeting. Combine them into a single, coherent, and detailed narrative summary that retains all important details, organized logically. Do not include reasoning, self-correction, or meta-commentary — output only the summary content.\n\n<summaries>\n{combined_text}\n</summaries>"
    )
}

/// Rough token count estimation using character count
pub fn rough_token_count(s: &str) -> usize {
    let char_count = s.chars().count();
    (char_count as f64 * 0.35).ceil() as usize
}

/// Chunks text into overlapping segments based on token count
/// Uses character-based chunking for proper Unicode support
///
/// # Arguments
/// * `text` - The text to chunk
/// * `chunk_size_tokens` - Maximum tokens per chunk
/// * `overlap_tokens` - Number of overlapping tokens between chunks
///
/// # Returns
/// Vector of text chunks with smart word-boundary splitting
pub fn chunk_text(text: &str, chunk_size_tokens: usize, overlap_tokens: usize) -> Vec<String> {
    info!(
        "Chunking text with token-based chunk_size: {} and overlap: {}",
        chunk_size_tokens, overlap_tokens
    );

    if text.is_empty() || chunk_size_tokens == 0 {
        return vec![];
    }

    // Convert token-based sizes to character-based sizes
    // Using ~2.85 chars per token (inverse of 0.35 tokens per char from rough_token_count)
    let chars_per_token = 1.0 / 0.35;
    let chunk_size_chars = (chunk_size_tokens as f64 * chars_per_token).ceil() as usize;
    let overlap_chars = (overlap_tokens as f64 * chars_per_token).ceil() as usize;

    // Collect characters for indexing (needed for proper Unicode support)
    let chars: Vec<char> = text.chars().collect();
    let total_chars = chars.len();

    if total_chars <= chunk_size_chars {
        info!("Text is shorter than chunk size, returning as a single chunk.");
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start_char = 0;

    while start_char < total_chars {
        let end_char = (start_char + chunk_size_chars).min(total_chars);
        let mut emitted_end_char = end_char;

        // Convert character indices to byte indices for string slicing
        let start_byte: usize = chars[..start_char].iter().map(|c| c.len_utf8()).sum();
        let mut end_byte: usize = chars[..end_char].iter().map(|c| c.len_utf8()).sum();

        // Try to break at sentence or word boundary for cleaner chunks
        if end_char < total_chars {
            let slice = &text[start_byte..end_byte];
            let sentence_boundary = slice.rfind(". ").map(|index| index + 2);
            let word_boundary = slice.rfind(' ').map(|index| index + 1);
            let boundary = sentence_boundary
                .filter(|end| slice[..*end].chars().count() > overlap_chars)
                .or_else(|| {
                    word_boundary.filter(|end| slice[..*end].chars().count() > overlap_chars)
                });

            if let Some(boundary) = boundary {
                end_byte = start_byte + boundary;
                emitted_end_char = start_char + slice[..boundary].chars().count();
            }
        }

        // Extract chunk
        chunks.push(text[start_byte..end_byte].to_string());

        if emitted_end_char >= total_chars {
            break;
        }

        start_char = emitted_end_char
            .saturating_sub(overlap_chars)
            .max(start_char + 1);
    }

    info!("Created {} chunks from text", chunks.len());
    chunks
}

/// Extracts meeting name from the first heading in markdown
///
/// # Arguments
/// * `markdown` - Markdown content
///
/// # Returns
/// Meeting name if found, None otherwise
pub fn extract_meeting_name_from_markdown(markdown: &str) -> Option<String> {
    markdown
        .lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line.trim_start_matches("# ").trim().to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedMeetingSummary {
    pub final_markdown: String,
    pub successful_chunk_count: i64,
    pub reasoning_stripped: bool,
}

pub(crate) fn build_final_prompts(
    template: &Template,
    language_name: &str,
    content: &str,
    original_text: &str,
    chunked: bool,
    custom_prompt: &str,
    notes_block: Option<&str>,
    dictionary: &[String],
) -> (String, String) {
    use crate::summary::user_notes::{marked_lines_block, transcript_has_marks, NOTES_INSTRUCTION};
    let mut system = template.render_system_prompt(language_name);
    // Look at the original transcript: after chunking, `content` is model-written summaries.
    if notes_block.is_some() || transcript_has_marks(original_text) {
        system.push_str(NOTES_INSTRUCTION);
    }
    if !dictionary.is_empty() {
        system.push_str(&format!("Correct spellings of names and terms: {}.\n", dictionary.join(", ")));
    }
    let mut user = format!("<transcript_chunks>\n{content}\n</transcript_chunks>\n");
    if chunked {
        if let Some(block) = marked_lines_block(original_text) {
            user.push('\n');
            user.push_str(&block);
            user.push('\n');
        }
    }
    if let Some(block) = notes_block {
        user.push('\n');
        user.push_str(block);
        user.push('\n');
    }
    if !custom_prompt.is_empty() {
        user.push_str("\n\nUser Provided Context:\n\n<user_context>\n");
        user.push_str(custom_prompt);
        user.push_str("\n</user_context>");
    }
    (system, user)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn generate_meeting_summary(
    client: &Client,
    provider: &LLMProvider,
    model_name: &str,
    api_key: &str,
    text: &str,
    custom_prompt: &str,
    notes_block: Option<&str>,
    dictionary: &[String],
    template: &Template,
    language_name: &str,
    token_threshold: usize,
    ollama_endpoint: Option<&str>,
    custom_openai_endpoint: Option<&str>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    app_data_dir: Option<&PathBuf>,
    cancellation_token: Option<&CancellationToken>,
    // Every call that got a response adds its usage here, even when the call then fails: the caller records it.
    usage: &mut Vec<TokenUsage>,
) -> Result<GeneratedMeetingSummary, String> {
    if cancellation_token.is_some_and(CancellationToken::is_cancelled) {
        return Err("Summary generation was cancelled".to_string());
    }
    info!("Starting summary generation with provider: {:?}, model: {}", provider, model_name);

    let total_tokens = rough_token_count(text);
    let mut content_to_summarize = text.to_string();
    let successful_chunk_count;
    let mut reasoning_stripped = false;
    let mut chunked = false;

    if (provider == &LLMProvider::Ollama || provider == &LLMProvider::BuiltInAI)
        && total_tokens >= token_threshold
    {
        chunked = true;
        // service.rs takes the prompt reserve from the threshold for both Ollama and BuiltInAI.
        let chunk_tokens = token_threshold;
        if chunk_tokens == 0 {
            return Err("The model's context is too small to summarize this transcript".to_string());
        }
        let chunks = chunk_text(text, chunk_tokens, 100);
        let num_chunks = chunks.len();
        let mut chunk_summaries = Vec::with_capacity(num_chunks);
        for (index, chunk) in chunks.iter().enumerate() {
            if cancellation_token.is_some_and(CancellationToken::is_cancelled) {
                return Err("Summary generation was cancelled".to_string());
            }
            let prompt = build_chunk_summary_user_prompt(chunk, language_name);
            for attempt in 1..=MAX_CHUNK_ATTEMPTS {
                let result = match generate_summary(
                    client, provider, model_name, api_key, CHUNK_SYSTEM_PROMPT,
                    &prompt, ollama_endpoint, custom_openai_endpoint, max_tokens, temperature,
                    top_p, app_data_dir, cancellation_token,
                )
                .await
                {
                    Ok(completion) => {
                        usage.extend(completion.usage);
                        // A retry with the same prompt would be cut off the same way, so fail now.
                        reject_truncated(&completion)?;
                        let cleaned = clean_llm_markdown_detailed(&completion.content);
                        reasoning_stripped |=
                            completion.reasoning_stripped || cleaned.reasoning_stripped;
                        require_visible_markdown("Summary chunk", &cleaned).map(|()| cleaned)
                    }
                    Err(error) => Err(error),
                };

                match result {
                    Ok(cleaned) => {
                        chunk_summaries.push(cleaned.markdown);
                        break;
                    }
                    Err(error)
                        if cancellation_token.is_some_and(CancellationToken::is_cancelled) =>
                    {
                        return Err("Summary generation was cancelled".to_string());
                    }
                    Err(error) if should_retry_chunk_failure(attempt, cancellation_token) => {
                        warn!(
                            "Failed processing chunk {}/{} on attempt {}/{}: {}; retrying",
                            index + 1,
                            num_chunks,
                            attempt,
                            MAX_CHUNK_ATTEMPTS,
                            error
                        );
                    }
                    Err(error) => {
                        error!(
                            "Failed processing chunk {}/{} on attempt {}/{}: {}",
                            index + 1,
                            num_chunks,
                            attempt,
                            MAX_CHUNK_ATTEMPTS,
                            error
                        );
                        return Err(format!(
                            "Summary generation could not complete because transcript section {} of {} failed after {} attempts: {}. Please retry.",
                            index + 1,
                            num_chunks,
                            MAX_CHUNK_ATTEMPTS,
                            error
                        ));
                    }
                }
            }
        }
        if chunk_summaries.is_empty() {
            return Err("Multi-level summarization failed: No chunks were processed successfully.".to_string());
        }
        successful_chunk_count = chunk_summaries.len() as i64;
        content_to_summarize = if chunk_summaries.len() == 1 {
            chunk_summaries.remove(0)
        } else {
            let prompt = build_combine_summary_user_prompt(&chunk_summaries.join("\n---\n"), language_name);
            let completion = generate_summary(
                client, provider, model_name, api_key,
                "You are an expert at synthesizing meeting summaries.", &prompt,
                ollama_endpoint, custom_openai_endpoint, max_tokens, temperature, top_p,
                app_data_dir, cancellation_token,
            )
            .await?;
            usage.extend(completion.usage);
            reject_truncated(&completion)?;
            let cleaned = clean_llm_markdown_detailed(&completion.content);
            reasoning_stripped |=
                completion.reasoning_stripped || cleaned.reasoning_stripped;
            require_visible_markdown("Combined summary", &cleaned)?;
            cleaned.markdown
        };
    } else {
        successful_chunk_count = 1;
    }

    info!("Generating final markdown report with template: {}", template.name);
    let (final_system_prompt, final_user_prompt) = build_final_prompts(
        template, language_name, &content_to_summarize, text, chunked, custom_prompt, notes_block, dictionary,
    );
    let completion = generate_summary(
        client, provider, model_name, api_key, &final_system_prompt, &final_user_prompt,
        ollama_endpoint, custom_openai_endpoint, max_tokens, temperature, top_p,
        app_data_dir, cancellation_token,
    )
    .await?;
    usage.extend(completion.usage);
    reject_truncated(&completion)?;
    let cleaned = clean_llm_markdown_detailed(&completion.content);
    reasoning_stripped |= completion.reasoning_stripped || cleaned.reasoning_stripped;
    require_visible_markdown("Final summary", &cleaned)?;

    Ok(GeneratedMeetingSummary {
        final_markdown: cleaned.markdown,
        successful_chunk_count,
        reasoning_stripped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::templates;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn long_transcript() -> String {
        "The team reviewed the quarterly plan and agreed on next steps. ".repeat(50)
    }

    fn reply(content: &str, completion_tokens: u32) -> String {
        format!(
            r#"{{"choices":[{{"message":{{"content":"{content}"}}}}],"usage":{{"prompt_tokens":10,"completion_tokens":{completion_tokens}}}}}"#
        )
    }

    fn outputs(usage: &[TokenUsage]) -> Vec<Option<u64>> {
        usage.iter().map(|u| u.output).collect()
    }

    async fn read_request(stream: &mut TcpStream) {
        let mut buf = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let n = stream.read(&mut chunk).await.unwrap();
            assert_ne!(n, 0, "connection closed before the request was complete");
            buf.extend_from_slice(&chunk[..n]);
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map_or(0, |v| v.trim().parse::<usize>().unwrap());
                if buf.len() >= end + 4 + length {
                    return;
                }
            }
        }
    }

    /// Serves one canned (status line, JSON body) per connection, in order. Returns the base URL.
    async fn serve(replies: Vec<(&'static str, String)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_request(&mut stream).await;
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).await.unwrap();
                stream.write_all(body.as_bytes()).await.unwrap();
            }
        });
        format!("http://{address}")
    }

    /// Ollama with a threshold that splits `long_transcript` into two chunks.
    async fn run_chunked(endpoint: &str, usage: &mut Vec<TokenUsage>) -> Result<GeneratedMeetingSummary, String> {
        let template = templates::get_template(templates::STANDARD_TEMPLATE_ID).unwrap();
        generate_meeting_summary(
            &Client::new(), &LLMProvider::Ollama, "m", "", &long_transcript(), "", None, &[],
            &template, "English", 1000, Some(endpoint), None, None, None, None, None, None, usage,
        )
        .await
    }

    #[tokio::test]
    async fn chunked_summary_collects_usage() {
        assert_eq!(chunk_text(&long_transcript(), 700, 100).len(), 2);
        let endpoint = serve(vec![
            ("200 OK", reply("Chunk one.", 1)),
            ("200 OK", reply("Chunk two.", 2)),
            ("200 OK", reply("Combined.", 3)),
            ("200 OK", reply("Final summary.", 4)),
        ])
        .await;
        let mut usage = Vec::new();
        let generated = run_chunked(&endpoint, &mut usage).await.unwrap();
        assert_eq!(generated.successful_chunk_count, 2);
        // Two chunks, the combine call and the final call: one entry per HTTP call, in order.
        assert_eq!(outputs(&usage), vec![Some(1), Some(2), Some(3), Some(4)]);
    }

    #[tokio::test]
    async fn chunk_failure_keeps_usage_of_earlier_calls() {
        let endpoint = serve(vec![
            ("200 OK", reply("Chunk one.", 1)),
            ("500 Internal Server Error", "{}".to_string()),
            ("500 Internal Server Error", "{}".to_string()),
        ])
        .await;
        let mut usage = Vec::new();
        assert!(run_chunked(&endpoint, &mut usage).await.is_err());
        assert_eq!(outputs(&usage), vec![Some(1)]);
    }

    #[tokio::test]
    async fn truncated_chunk_fails_without_a_retry() {
        let cut_off = r#"{"choices":[{"message":{"content":"Half"},"finish_reason":"length"}],"usage":{"prompt_tokens":10,"completion_tokens":8192}}"#;
        let endpoint = serve(vec![
            ("200 OK", cut_off.to_string()),
            ("200 OK", cut_off.to_string()),
            ("200 OK", cut_off.to_string()),
        ])
        .await;
        let mut usage = Vec::new();
        let error = run_chunked(&endpoint, &mut usage).await.unwrap_err();
        assert_eq!(error, crate::summary::llm_client::OUTPUT_LIMIT_MESSAGE);
        // The cut-off call is billed once and not repeated with the same prompt.
        assert_eq!(outputs(&usage), vec![Some(8192)]);
    }

    #[tokio::test]
    async fn ollama_without_room_for_a_chunk_fails_before_any_call() {
        let template = templates::get_template(templates::STANDARD_TEMPLATE_ID).unwrap();
        let mut usage = Vec::new();
        let error = generate_meeting_summary(
            &Client::new(), &LLMProvider::Ollama, "m", "", &long_transcript(), "", None, &[],
            &template, "English", 0, Some("http://127.0.0.1:9"), None, None, None, None, None, None,
            &mut usage,
        )
        .await
        .unwrap_err();
        assert!(error.contains("too small"), "unexpected error: {error}");
        assert!(usage.is_empty());
    }

    #[tokio::test]
    async fn builtin_chunks_are_as_large_as_the_threshold() {
        // service.rs already reserved the prompt room in the threshold, so the chunks must not lose it again.
        let transcript = long_transcript().repeat(2);
        let expected = chunk_text(&transcript, 1000, 100).len();
        assert_ne!(expected, chunk_text(&transcript, 700, 100).len(), "the transcript must tell the two sizes apart");
        let template = templates::get_template(templates::STANDARD_TEMPLATE_ID).unwrap();
        let mut usage = Vec::new();
        // No app data dir: the first chunk fails before any model runs, and the error names the chunk count.
        let error = generate_meeting_summary(
            &Client::new(), &LLMProvider::BuiltInAI, "m", "", &transcript, "", None, &[],
            &template, "English", 1000, None, None, None, None, None, None, None,
            &mut usage,
        )
        .await
        .unwrap_err();
        assert!(error.contains(&format!("section 1 of {expected} ")), "unexpected error: {error}");
    }

    #[test]
    fn chunk_text_preserves_content_after_early_sentence_boundary() {
        let marker = "LOST_MARKER";
        let text = format!("Intro. {marker} trailing content ensures chunking");

        let chunks = chunk_text(&text, 10, 1);

        assert!(
            chunks.iter().any(|chunk| chunk.contains(marker)),
            "marker was omitted from all chunks: {chunks:?}"
        );
    }

    #[test]
    fn chunk_text_keeps_unicode_boundaries() {
        assert_eq!(chunk_text("é ab", 1, 0), vec!["é ", "ab"]);
    }

    #[test]
    fn chunk_text_progresses_when_overlap_matches_window() {
        assert_eq!(chunk_text("abcd", 1, 1), vec!["abc", "bcd"]);
    }

    #[test]
    fn chunk_and_combine_prompts_ask_for_the_target_language_not_english() {
        let chunk = build_chunk_summary_user_prompt("текст", "Russian");
        let combine = build_combine_summary_user_prompt("a\n---\nb", "Russian");
        for p in [&chunk, &combine] {
            assert!(p.contains("Write in Russian."), "{p}");
            assert!(!p.contains("English"), "{p}");
        }
    }

    #[test]
    fn chunk_prompt_forbids_reasoning_output() {
        let prompt = build_chunk_summary_user_prompt("x", "English");
        assert!(
            prompt.contains("Do not include reasoning")
                || prompt.contains("meta-commentary")
                || prompt.contains("self-correction")
        );
    }

    #[test]
    fn chunk_retries_once_unless_cancelled() {
        assert!(should_retry_chunk_failure(1, None));
        assert!(!should_retry_chunk_failure(2, None));
        let cancellation_token = CancellationToken::new();
        cancellation_token.cancel();
        assert!(!should_retry_chunk_failure(1, Some(&cancellation_token)));
    }

    #[test]
    fn cleaner_removes_closed_reasoning_envelopes_everywhere() {
        let cleaned = clean_llm_markdown_detailed(
            "Intro\n<think>private</think>\n# Meeting\nHello\n<thinking>also private</thinking>\nTail",
        );
        assert!(cleaned.reasoning_stripped);
        assert!(!cleaned.markdown.contains("<think"));
        assert!(!cleaned.markdown.contains("<thinking"));
        assert!(cleaned.markdown.contains("Intro"));
        assert!(cleaned.markdown.contains("# Meeting"));
        assert!(cleaned.markdown.contains("Tail"));

        let fenced = clean_llm_markdown_detailed("```\n<think>private</think>\nvisible\n```");
        assert_eq!(fenced.markdown, "visible");
        assert!(fenced.reasoning_stripped);

        let attributed = clean_llm_markdown_detailed(
            "Visible\n<thinking class=\"internal\">private</thinking>\nTail",
        );
        assert!(attributed.reasoning_stripped);
        assert!(!attributed.markdown.contains("private"));
        assert!(attributed.markdown.contains("Visible"));
        assert!(attributed.markdown.contains("Tail"));

        let literal = clean_llm_markdown_detailed("<thinker>Visible</thinker>");
        assert!(!literal.reasoning_stripped);
        assert_eq!(literal.markdown, "<thinker>Visible</thinker>");
    }

    #[test]
    fn cleaner_rejects_unterminated_reasoning_markers() {
        for raw in [
            "Visible\n<think>private",
            "Visible\n</thinking>",
            "Visible\n<think class=\"internal\">private",
        ] {
            let cleaned = clean_llm_markdown_detailed(raw);
            assert_eq!(
                require_visible_markdown("Final summary", &cleaned),
                Err("Final summary contained an unterminated reasoning marker".to_string())
            );
        }
    }

    #[test]
    fn reasoning_only_and_empty_fences_fail_visible_content_guard() {
        let reasoning_only = clean_llm_markdown_detailed("<think>private</think>");
        assert_eq!(
            require_visible_markdown("Final summary", &reasoning_only),
            Err("Final summary returned no visible summary content after reasoning removal".to_string())
        );
        for raw in ["```\n```", "```markdown\n```", "```\r\n```", "```markdown\r\n```"] {
            let empty_fence = clean_llm_markdown_detailed(raw);
            assert_eq!(empty_fence.markdown, "");
            assert_eq!(
                require_visible_markdown("Translation", &empty_fence),
                Err("Translation returned no visible summary content after reasoning removal".to_string())
            );
        }
    }

    #[test]
    fn final_prompts_add_notes_only_when_present() {
        let t: crate::summary::templates::Template =
            serde_json::from_value(serde_json::json!({"name": "T", "description": "", "sections": []})).unwrap();
        let (sys, user) = build_final_prompts(&t, "English", "[00:01] hi", "[00:01] hi", false, "", None, &[]);
        assert!(!sys.contains("marked transcript lines"));
        assert!(!user.contains("<user_notes>"));

        let marked = "[00:01] [MARKED: action] hi";
        let (sys, user) = build_final_prompts(&t, "English", marked, marked, false, "", None, &[]);
        assert!(sys.contains("marked transcript lines"));
        assert!(!user.contains("<user_notes>"));
        assert!(!user.contains("<marked_lines>")); // not chunked: the lines are already in the transcript

        let (sys, user) = build_final_prompts(&t, "English", "[00:01] hi", "[00:01] hi", false, "ctx", Some("<user_notes>\n[00:01] x\n</user_notes>"), &[]);
        assert!(sys.contains("marked transcript lines"));
        assert!(user.contains("</transcript_chunks>\n\n<user_notes>"));
        assert!(user.contains("<user_context>\nctx"));
    }

    #[test]
    fn chunked_summaries_still_get_the_marked_lines() {
        let t: crate::summary::templates::Template =
            serde_json::from_value(serde_json::json!({"name": "T", "description": "", "sections": []})).unwrap();
        let original = "[00:01] hi\n[00:05] [MARKED: action] send deck";
        let (sys, user) = build_final_prompts(&t, "English", "Chunk summary without marks", original, true, "", None, &[]);
        assert!(sys.contains("marked transcript lines"));
        assert!(user.contains("<marked_lines>\n[00:05] [MARKED: action] send deck\n</marked_lines>"));
    }

    #[test]
    fn final_prompt_spelling_hints_only_when_dictionary_is_not_empty() {
        let t: crate::summary::templates::Template =
            serde_json::from_value(serde_json::json!({"name": "T", "description": "", "sections": []})).unwrap();
        let base = t.render_system_prompt("English");
        let (sys, user) = build_final_prompts(&t, "English", "[00:01] hi", "[00:01] hi", false, "", None, &[]);
        assert_eq!(sys, base);

        let words = vec!["Tauri".to_string()];
        let (hinted_sys, hinted_user) = build_final_prompts(&t, "English", "[00:01] hi", "[00:01] hi", false, "", None, &words);
        assert_eq!(hinted_sys, format!("{base}Correct spellings of names and terms: Tauri.\n"));
        assert_eq!(hinted_user, user);

        let words = vec!["Tauri".to_string(), "Claude Code".to_string()];
        let (sys, _) = build_final_prompts(&t, "English", "[00:01] hi", "[00:01] hi", false, "", None, &words);
        assert!(sys.ends_with("Correct spellings of names and terms: Tauri, Claude Code.\n"), "{sys}");
    }

    #[test]
    fn cleaner_strips_outer_crlf_fence_without_normalizing_inner_markdown() {
        let markdown = "# Heading\r\n\r\n```rust\r\nlet x = 1;\r\n```";
        let wrapped = format!("```\r\n{markdown}\r\n```");
        let cleaned = clean_llm_markdown_detailed(&wrapped);
        assert_eq!(cleaned.markdown, markdown);
        assert!(!cleaned.reasoning_stripped);
        assert_eq!(clean_llm_markdown_detailed(markdown).markdown, markdown);
    }
}
