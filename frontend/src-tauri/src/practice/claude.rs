//! Streaming Claude client for Speaking Practice: SSE parsing, request bodies and the streaming call.

use crate::database::repositories::setting::SettingsRepository;
use crate::practice::db::{Material, MaterialKind};
use crate::practice::{CHAT_MAX_TOKENS, CHAT_MODEL, LONG_MAX_TOKENS, PLAN_MODEL, REPORT_MODEL};
use base64::Engine as _;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";

#[derive(Debug, PartialEq)]
pub enum SseEvent {
    Text(String),
    Stop,
    Error { kind: String, message: String },
    Ignore,
}

#[derive(Default)]
pub struct SseParser {
    buf: String,
}

impl SseParser {
    pub fn push(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        while let Some(nl) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=nl).collect();
            // Each event's JSON is on one `data:` line and carries its own `type`, so `event:` lines are not needed.
            if let Some(data) = line.trim_end().strip_prefix("data:") {
                out.push(parse_event(data.trim()));
            }
        }
        out
    }
}

fn parse_event(data: &str) -> SseEvent {
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        return SseEvent::Ignore;
    };
    match v["type"].as_str() {
        Some("content_block_delta") if v["delta"]["type"] == "text_delta" => {
            SseEvent::Text(v["delta"]["text"].as_str().unwrap_or_default().to_string())
        }
        Some("message_stop") => SseEvent::Stop,
        Some("error") => SseEvent::Error {
            kind: v["error"]["type"].as_str().unwrap_or("unknown").to_string(),
            message: v["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        },
        _ => SseEvent::Ignore,
    }
}

pub struct ChatMsg {
    pub role: &'static str,
    pub content: String,
}

pub fn chat_body(system: &str, msgs: &[ChatMsg]) -> Value {
    let messages: Vec<Value> = msgs
        .iter()
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();
    json!({
        "model": CHAT_MODEL,
        "max_tokens": CHAT_MAX_TOKENS,
        "stream": true,
        "system": system,
        "messages": messages,
        "thinking": {"type": "disabled"},
        "output_config": {"effort": "low"},
        "cache_control": {"type": "ephemeral"},
    })
}

pub fn plan_body(system: &str, user: &str, materials: &[Material]) -> Value {
    let mut blocks: Vec<Value> = materials
        .iter()
        .map(|m| match m.meta.kind {
            MaterialKind::Pdf => json!({
                "type": "document",
                "source": {
                    "type": "base64",
                    "media_type": "application/pdf",
                    "data": base64::engine::general_purpose::STANDARD.encode(&m.content),
                },
            }),
            MaterialKind::Text => json!({
                "type": "text",
                "text": format!("Material: {}\n{}", m.meta.name, String::from_utf8_lossy(&m.content)),
            }),
        })
        .collect();
    blocks.push(json!({"type": "text", "text": user}));
    json!({
        "model": PLAN_MODEL,
        "max_tokens": LONG_MAX_TOKENS,
        "stream": true,
        "system": system,
        "messages": [{"role": "user", "content": blocks}],
        "output_config": {"effort": "high"},
    })
}

pub fn report_body(system: &str, user: &str, schema: &Value) -> Value {
    json!({
        "model": REPORT_MODEL,
        "max_tokens": LONG_MAX_TOKENS,
        "stream": true,
        "system": system,
        "messages": [{"role": "user", "content": user}],
        "output_config": {"effort": "high", "format": {"type": "json_schema", "schema": schema}},
    })
}

#[derive(Debug, PartialEq)]
pub enum ClaudeError {
    Cancelled,
    NoKey,
    Rejected,
    Busy,
    Http(u16),
    Network(String),
    Stream(String),
}

impl ClaudeError {
    pub fn user_message(&self) -> &'static str {
        match self {
            ClaudeError::NoKey => "Add your Claude API key in Settings to use Practice",
            _ => "Couldn't reach Claude — tap to talk and try again",
        }
    }
}

pub async fn stream(
    client: &reqwest::Client,
    key: &str,
    body: &Value,
    cancel: &CancellationToken,
    mut on_text: impl FnMut(&str),
) -> Result<String, ClaudeError> {
    let request = client
        .post(MESSAGES_URL)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .json(body)
        .send();
    let resp = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(ClaudeError::Cancelled),
        r = request => r.map_err(|e| ClaudeError::Network(e.to_string()))?,
    };
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        // Only the status is logged: the body may echo request content.
        log::warn!("practice: Claude returned HTTP {status}");
        return Err(match status {
            401 | 403 => ClaudeError::Rejected,
            429 | 529 => ClaudeError::Busy,
            code => ClaudeError::Http(code),
        });
    }

    let mut parser = SseParser::default();
    let mut pending = Vec::new();
    let mut text = String::new();
    let mut chunks = resp.bytes_stream();
    loop {
        let next = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ClaudeError::Cancelled),
            next = chunks.next() => next,
        };
        // At the end of the body, a final unterminated line is flushed by feeding a newline.
        let (events, ended) = match next {
            Some(chunk) => {
                let chunk = chunk.map_err(|e| ClaudeError::Network(e.to_string()))?;
                (feed(&mut parser, &mut pending, &chunk), false)
            }
            None => (feed(&mut parser, &mut pending, b"\n"), true),
        };
        for event in events {
            match event {
                SseEvent::Text(t) => {
                    on_text(&t);
                    text.push_str(&t);
                }
                SseEvent::Stop => {
                    log::debug!("practice: Claude reply of {} chars", text.len());
                    return Ok(text);
                }
                SseEvent::Error { kind, .. } => return Err(map_error_kind(&kind)),
                SseEvent::Ignore => {}
            }
        }
        if ended {
            return Err(ClaudeError::Stream("ended early".into()));
        }
    }
}

pub async fn api_key(pool: &sqlx::SqlitePool) -> Result<String, ClaudeError> {
    match SettingsRepository::get_api_key(pool, "claude").await {
        Ok(Some(key)) if !key.trim().is_empty() => Ok(key.trim().to_string()),
        Ok(_) => Err(ClaudeError::NoKey),
        Err(e) => {
            // A failed read is reported as a missing key; the DB error is logged, never shown.
            log::warn!("practice: could not read the Claude key: {e}");
            Err(ClaudeError::NoKey)
        }
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn map_error_kind(kind: &str) -> ClaudeError {
    match kind {
        "overloaded_error" | "rate_limit_error" => ClaudeError::Busy,
        "authentication_error" | "permission_error" => ClaudeError::Rejected,
        other => ClaudeError::Stream(format!("error event: {other}")),
    }
}

/// Feeds raw bytes; only whole lines are decoded, so a character split across network chunks waits for its tail.
fn feed(parser: &mut SseParser, pending: &mut Vec<u8>, bytes: &[u8]) -> Vec<SseEvent> {
    pending.extend_from_slice(bytes);
    let cut = pending
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let complete: Vec<u8> = pending.drain(..cut).collect();
    parser.push(&String::from_utf8_lossy(&complete))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::db::MaterialMeta;

    const SAMPLE_KEY: &str = "sk-ant-api03-SAMPLEKEY-0123456789";

    fn text(t: &str) -> SseEvent {
        SseEvent::Text(t.to_string())
    }

    fn delta(t: &str) -> String {
        format!(
            "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{t}\"}}}}\n\n"
        )
    }

    #[test]
    fn parser_joins_events_split_across_chunks() {
        let s = format!(
            "{}event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n",
            delta("Привет")
        );
        let mut p = SseParser::default();
        let mut events = Vec::new();
        for c in s.chars() {
            events.extend(p.push(&c.to_string()));
        }
        assert_eq!(events, vec![text("Привет"), SseEvent::Stop]);
    }

    #[test]
    fn parser_handles_crlf_and_several_events_per_chunk() {
        let s = format!(
            "event: ping\r\ndata: {{\"type\":\"ping\"}}\r\n\r\n{}event: message_stop\r\ndata: {{\"type\":\"message_stop\"}}\r\n\r\n",
            delta("Hi").replace('\n', "\r\n")
        );
        let mut p = SseParser::default();
        assert_eq!(
            p.push(&s),
            vec![SseEvent::Ignore, text("Hi"), SseEvent::Stop]
        );

        let mut split = SseParser::default();
        let mut events = Vec::new();
        for c in s.chars() {
            events.extend(split.push(&c.to_string()));
        }
        assert_eq!(events, vec![SseEvent::Ignore, text("Hi"), SseEvent::Stop]);
    }

    #[test]
    fn parser_ignores_thinking_and_ping() {
        let s = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hmm\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"abc\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: ping\ndata: {\"type\":\"ping\"}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
        );
        let mut p = SseParser::default();
        let events = p.push(s);
        assert_eq!(events.len(), 8);
        assert!(events.iter().all(|e| *e == SseEvent::Ignore));
    }

    #[test]
    fn parser_reports_error_event() {
        let s = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        let mut p = SseParser::default();
        assert_eq!(
            p.push(s),
            vec![SseEvent::Error {
                kind: "overloaded_error".into(),
                message: "Overloaded".into()
            }]
        );
    }

    #[test]
    fn map_error_kind_maps_overload_to_busy() {
        assert_eq!(map_error_kind("overloaded_error"), ClaudeError::Busy);
        assert_eq!(map_error_kind("rate_limit_error"), ClaudeError::Busy);
        assert_eq!(
            map_error_kind("authentication_error"),
            ClaudeError::Rejected
        );
        assert!(matches!(
            map_error_kind("api_error"),
            ClaudeError::Stream(_)
        ));
    }

    #[test]
    fn feed_keeps_multibyte_char_split_across_chunks() {
        let line = delta("Привет").into_bytes();
        // Cut inside the two-byte "П" so the first chunk ends mid-character.
        let cut = line.windows(2).position(|w| w == [0xD0, 0x9F]).unwrap() + 1;
        let mut p = SseParser::default();
        let mut pending = Vec::new();
        let mut events = feed(&mut p, &mut pending, &line[..cut]);
        events.extend(feed(&mut p, &mut pending, &line[cut..]));
        assert_eq!(events, vec![text("Привет")]);
    }

    #[test]
    fn chat_body_turns_thinking_off_at_low_effort() {
        let msgs = vec![
            ChatMsg {
                role: "user",
                content: "Hello".into(),
            },
            ChatMsg {
                role: "assistant",
                content: "Hi there".into(),
            },
        ];
        let body = chat_body("Be Mike.", &msgs);
        assert_eq!(body["model"], CHAT_MODEL);
        assert_eq!(body["max_tokens"], CHAT_MAX_TOKENS);
        assert_eq!(body["stream"], true);
        assert_eq!(body["system"], "Be Mike.");
        assert_eq!(
            body["messages"],
            json!([
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there"}
            ])
        );
        assert_eq!(body["thinking"], json!({"type": "disabled"}));
        assert_eq!(body["output_config"], json!({"effort": "low"}));
        assert_eq!(body["cache_control"], json!({"type": "ephemeral"}));
    }

    #[test]
    fn plan_body_puts_pdfs_before_the_instruction() {
        let pdf_bytes: Vec<u8> = (0..300u32).map(|i| (i * 7) as u8).collect();
        let pdf = Material {
            meta: MaterialMeta {
                id: "m1".into(),
                setup_id: "s1".into(),
                kind: MaterialKind::Pdf,
                name: "brief.pdf".into(),
                bytes: pdf_bytes.len() as i64,
            },
            content: pdf_bytes.clone(),
        };
        let notes = Material {
            meta: MaterialMeta {
                id: "m2".into(),
                setup_id: "s1".into(),
                kind: MaterialKind::Text,
                name: "notes.txt".into(),
                bytes: 5,
            },
            content: b"hello".to_vec(),
        };
        let body = plan_body("Plan it.", "Make the plan.", &[pdf, notes]);
        assert_eq!(body["model"], PLAN_MODEL);
        assert_eq!(body["max_tokens"], LONG_MAX_TOKENS);
        assert_eq!(body["stream"], true);
        assert_eq!(body["output_config"], json!({"effort": "high"}));

        let blocks = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0]["type"], "document");
        assert_eq!(blocks[0]["source"]["type"], "base64");
        assert_eq!(blocks[0]["source"]["media_type"], "application/pdf");
        let data = blocks[0]["source"]["data"].as_str().unwrap();
        assert!(!data.contains('\n'));
        assert_eq!(
            data,
            base64::engine::general_purpose::STANDARD.encode(&pdf_bytes)
        );
        assert_eq!(
            blocks[1],
            json!({"type": "text", "text": "Material: notes.txt\nhello"})
        );
        assert_eq!(blocks[2], json!({"type": "text", "text": "Make the plan."}));
    }

    #[test]
    fn report_body_sets_json_schema_format() {
        let schema = json!({"type": "object", "properties": {"score": {"type": "integer"}}});
        let body = report_body("Score it.", "Report.", &schema);
        assert_eq!(body["model"], REPORT_MODEL);
        assert_eq!(body["max_tokens"], LONG_MAX_TOKENS);
        assert_eq!(body["stream"], true);
        assert_eq!(body["system"], "Score it.");
        assert_eq!(
            body["messages"],
            json!([{"role": "user", "content": "Report."}])
        );
        assert_eq!(
            body["output_config"],
            json!({"effort": "high", "format": {"type": "json_schema", "schema": schema}})
        );
    }

    #[test]
    fn user_messages_never_echo_key() {
        let errors = [
            ClaudeError::Cancelled,
            ClaudeError::NoKey,
            ClaudeError::Rejected,
            ClaudeError::Busy,
            ClaudeError::Http(500),
            ClaudeError::Network("connection reset".into()),
            ClaudeError::Stream("ended early".into()),
        ];
        for e in &errors {
            assert!(!e.user_message().contains(SAMPLE_KEY));
            assert!(!e.user_message().contains("sk-ant"));
        }
        assert_eq!(
            ClaudeError::NoKey.user_message(),
            "Add your Claude API key in Settings to use Practice"
        );
    }

    #[tokio::test]
    async fn stream_returns_cancelled_when_already_cancelled() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let body = chat_body("s", &[]);
        let r = stream(&http_client(), SAMPLE_KEY, &body, &cancel, |_| {}).await;
        assert_eq!(r, Err(ClaudeError::Cancelled));
    }

    #[tokio::test]
    async fn api_key_reads_trimmed_claude_key_and_reports_missing() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        assert_eq!(api_key(&pool).await, Err(ClaudeError::NoKey));

        sqlx::query("INSERT INTO settings (id, provider, model, whisperModel, anthropicApiKey) VALUES ('1', 'claude', 'm', 'w', '  sk-ant-x \n')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(api_key(&pool).await, Ok("sk-ant-x".to_string()));

        sqlx::query("UPDATE settings SET anthropicApiKey = '   ' WHERE id = '1'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(api_key(&pool).await, Err(ClaudeError::NoKey));
    }
}
