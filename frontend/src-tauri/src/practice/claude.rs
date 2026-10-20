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
/// Longest silence between body chunks before the stream counts as stalled. Anthropic sends `ping` events well inside this.
pub const STREAM_IDLE_SECS: u64 = 60;

#[derive(Debug, PartialEq)]
pub enum SseEvent {
    Text(String),
    StopReason(String),
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
        Some("message_delta") => match v["delta"]["stop_reason"].as_str() {
            Some(reason) => SseEvent::StopReason(reason.to_string()),
            None => SseEvent::Ignore,
        },
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
    /// The settings row could not be read; the key may well exist.
    Settings,
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
            ClaudeError::Rejected => "Claude rejected your API key — check it in Settings",
            ClaudeError::Busy => "Claude is busy right now — tap to talk and try again",
            ClaudeError::Settings => "Couldn't read your settings — try again",
            _ => "Couldn't reach Claude — tap to talk and try again",
        }
    }
}

pub async fn stream(
    client: &reqwest::Client,
    key: &str,
    body: &Value,
    cancel: &CancellationToken,
    on_text: impl FnMut(&str),
) -> Result<String, ClaudeError> {
    let idle = Duration::from_secs(STREAM_IDLE_SECS);
    stream_with_idle(client, MESSAGES_URL, key, body, cancel, idle, on_text).await
}

async fn stream_with_idle(
    client: &reqwest::Client,
    url: &str,
    key: &str,
    body: &Value,
    cancel: &CancellationToken,
    idle: Duration,
    mut on_text: impl FnMut(&str),
) -> Result<String, ClaudeError> {
    let request = client
        .post(url)
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
    if let Some(err) = map_status(status) {
        // Only the status is logged: the body may echo request content.
        log::warn!("practice: Claude returned HTTP {status}");
        return Err(err);
    }

    let mut parser = SseParser::default();
    let mut pending = Vec::new();
    let mut text = String::new();
    let mut stop_reason: Option<String> = None;
    let mut chunks = resp.bytes_stream();
    loop {
        let next = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ClaudeError::Cancelled),
            r = tokio::time::timeout(idle, chunks.next()) => {
                r.map_err(|_| ClaudeError::Network("stalled".into()))?
            }
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
                SseEvent::StopReason(reason) => stop_reason = Some(reason),
                SseEvent::Stop => {
                    return match stop_reason.as_deref() {
                        // A cut-off or refused reply must not pass as a finished answer.
                        Some(r @ ("max_tokens" | "refusal")) => {
                            Err(ClaudeError::Stream(r.to_string()))
                        }
                        _ => {
                            log::debug!("practice: Claude reply of {} chars", text.len());
                            Ok(text)
                        }
                    };
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
            log::warn!("practice: could not read settings: {e}");
            Err(ClaudeError::Settings)
        }
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .expect("practice http client")
}

/// Maps a non-2xx status to its error; `None` for success.
fn map_status(status: u16) -> Option<ClaudeError> {
    match status {
        200..=299 => None,
        401 | 403 => Some(ClaudeError::Rejected),
        429 | 529 => Some(ClaudeError::Busy),
        code => Some(ClaudeError::Http(code)),
    }
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

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
        );
        let mut p = SseParser::default();
        let events = p.push(s);
        assert_eq!(events.len(), 7);
        assert!(events.iter().all(|e| *e == SseEvent::Ignore));
    }

    #[test]
    fn parser_reads_stop_reason_from_message_delta() {
        for reason in ["max_tokens", "refusal", "end_turn"] {
            let s = format!(
                "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{reason}\",\"stop_sequence\":null}}}}\n\n"
            );
            assert_eq!(
                SseParser::default().push(&s),
                vec![SseEvent::StopReason(reason.into())]
            );
        }
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
    fn map_status_maps_each_failure_class() {
        assert_eq!(map_status(200), None);
        assert_eq!(map_status(299), None);
        assert_eq!(map_status(401), Some(ClaudeError::Rejected));
        assert_eq!(map_status(403), Some(ClaudeError::Rejected));
        assert_eq!(map_status(429), Some(ClaudeError::Busy));
        assert_eq!(map_status(529), Some(ClaudeError::Busy));
        assert_eq!(map_status(500), Some(ClaudeError::Http(500)));
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
            ClaudeError::Settings,
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
        assert_eq!(
            ClaudeError::Rejected.user_message(),
            "Claude rejected your API key — check it in Settings"
        );
        assert_eq!(
            ClaudeError::Busy.user_message(),
            "Claude is busy right now — tap to talk and try again"
        );
        assert_eq!(
            ClaudeError::Settings.user_message(),
            "Couldn't read your settings — try again"
        );
        for e in [
            ClaudeError::Cancelled,
            ClaudeError::Http(500),
            ClaudeError::Network("x".into()),
            ClaudeError::Stream("x".into()),
        ] {
            assert_eq!(
                e.user_message(),
                "Couldn't reach Claude — tap to talk and try again"
            );
        }
    }

    /// Serves one SSE-or-plain reply to one request, then holds the connection for `hold` before closing.
    async fn serve_once(reply: String, hold: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/messages", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            read_request(&mut sock).await;
            sock.write_all(reply.as_bytes()).await.unwrap();
            tokio::time::sleep(hold).await;
            let _ = sock.shutdown().await;
        });
        url
    }

    /// Reads headers and the Content-Length body, so the reply is not cut by a connection reset.
    async fn read_request(sock: &mut TcpStream) {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = sock.read(&mut chunk).await.unwrap();
            assert!(n > 0, "client closed before sending a request");
            buf.extend_from_slice(&chunk[..n]);
            let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .map_or(0, |v| v.trim().parse().unwrap());
            while buf.len() < end + 4 + len {
                let n = sock.read(&mut chunk).await.unwrap();
                assert!(n > 0, "client closed mid-request");
                buf.extend_from_slice(&chunk[..n]);
            }
            return;
        }
    }

    fn sse(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
        )
    }

    async fn run(url: &str, idle: Duration) -> Result<String, ClaudeError> {
        // no_proxy: an environment proxy must not capture the loopback server.
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let body = chat_body("s", &[]);
        stream_with_idle(
            &client,
            url,
            SAMPLE_KEY,
            &body,
            &CancellationToken::new(),
            idle,
            |_| {},
        )
        .await
    }

    #[tokio::test]
    async fn stream_errors_when_body_ends_without_message_stop() {
        let url = serve_once(sse(&delta("Hi")), Duration::ZERO).await;
        let r = run(&url, Duration::from_secs(5)).await;
        assert!(matches!(r, Err(ClaudeError::Stream(_))), "{r:?}");
    }

    #[tokio::test]
    async fn stream_reports_stall_when_body_goes_silent() {
        let url = serve_once(sse(&delta("Hi")), Duration::from_secs(3)).await;
        let r = run(&url, Duration::from_millis(200)).await;
        assert_eq!(r, Err(ClaudeError::Network("stalled".into())));
    }

    #[tokio::test]
    async fn stream_maps_mid_stream_overload_to_busy() {
        let body = format!(
            "{}event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}}}\n\n",
            delta("Hi")
        );
        let url = serve_once(sse(&body), Duration::ZERO).await;
        assert_eq!(
            run(&url, Duration::from_secs(5)).await,
            Err(ClaudeError::Busy)
        );
    }

    #[tokio::test]
    async fn stream_returns_text_on_end_turn_and_errors_on_truncation() {
        let cases = [
            ("end_turn", Ok("Hi".to_string())),
            ("max_tokens", Err(ClaudeError::Stream("max_tokens".into()))),
        ];
        for (reason, expected) in cases {
            let body = format!(
                "{}event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{reason}\"}}}}\n\nevent: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n",
                delta("Hi")
            );
            let url = serve_once(sse(&body), Duration::ZERO).await;
            assert_eq!(
                run(&url, Duration::from_secs(5)).await,
                expected,
                "{reason}"
            );
        }
    }

    #[tokio::test]
    async fn stream_maps_rate_limit_status_to_busy() {
        let reply =
            "HTTP/1.1 429 Too Many Requests\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        let url = serve_once(reply.into(), Duration::ZERO).await;
        assert_eq!(
            run(&url, Duration::from_secs(5)).await,
            Err(ClaudeError::Busy)
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

    async fn test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn api_key_reads_trimmed_claude_key_and_reports_missing() {
        let pool = test_pool().await;
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

    #[tokio::test]
    async fn api_key_reports_settings_error_not_missing_key_when_db_fails() {
        let pool = test_pool().await;
        pool.close().await;
        assert_eq!(api_key(&pool).await, Err(ClaudeError::Settings));
    }
}
