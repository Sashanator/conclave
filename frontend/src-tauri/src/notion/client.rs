//! Notion HTTP client. The token is only ever sent to api.notion.com and never logged or put in an error.
use std::time::Duration;

use serde_json::Value;

pub(crate) const NOTION_API: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2026-03-11";

/// No `Debug` on purpose: it holds the token.
pub(crate) struct NotionClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

pub(crate) struct CreatedPage {
    pub id: String,
    pub url: Option<String>,
}

#[derive(Debug)]
pub(crate) enum CallError {
    Http { status: u16, code: String, message: String },
    /// Could not connect: nothing was sent.
    Offline,
    /// Timeout or a connection lost after sending, or an unreadable 2xx: the write may have happened.
    NoAnswer,
}

impl CallError {
    pub fn is_ambiguous(&self) -> bool {
        match self {
            CallError::NoAnswer => true,
            CallError::Http { status, .. } => *status >= 500,
            CallError::Offline => false,
        }
    }

    pub fn state(&self) -> &'static str {
        match self {
            // Notion reports an unshared/unknown parent as 404, but a bad parent can also come as a 400
            // validation_error whose message names the failing path `body.parent...`; a property error
            // names `body.properties...`. Match the path, not the bare word "parent" (a property may be so named).
            CallError::Http { status: 400, message, .. } if message.contains("body.parent") => "not_found",
            CallError::Http { status: 400, .. } => "mapping_broken",
            CallError::Http { status: 401, .. } => "token_rejected",
            CallError::Http { status: 403 | 404, .. } => "not_shared",
            _ => "offline",
        }
    }

    pub fn message(&self, what: &str) -> String {
        match self {
            CallError::Http { status: 401, .. } => "Notion rejected the token. Paste a new one in Settings → Notion.".into(),
            CallError::Http { status: 403, .. } => format!("{what}: the Conclave connection isn't allowed to add pages here. In Notion, give it the Insert content capability."),
            CallError::Http { status: 404, .. } => format!("{what} isn't shared with Conclave, or was deleted. In Notion: database ••• → Connections → add Conclave"),
            CallError::Http { status: 429, .. } => "Notion asked to slow down. Try again in a minute.".into(),
            CallError::Http { status, .. } if *status >= 500 => format!("Notion had a problem (HTTP {status})."),
            CallError::Http { status, message, .. } if message.is_empty() => format!("{what}: Notion refused it (HTTP {status})."),
            CallError::Http { message, .. } => format!("{what}: Notion refused it: {message}"),
            CallError::Offline => "Notion could not be reached. Check your internet connection.".into(),
            CallError::NoAnswer => "Notion did not answer in time.".into(),
        }
    }
}

impl NotionClient {
    pub fn new(token: String) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Self { base: NOTION_API.into(), token, http }
    }

    #[cfg(test)]
    pub fn with_base(base: &str, token: &str, timeout: Duration) -> Self {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()
            .unwrap();
        Self { base: base.into(), token: token.into(), http }
    }

    pub async fn me(&self) -> Result<(), CallError> {
        self.call(reqwest::Method::GET, "/users/me", None).await.map(|_| ())
    }

    pub async fn database(&self, id: &str) -> Result<Value, CallError> {
        self.call(reqwest::Method::GET, &format!("/databases/{id}"), None).await
    }

    pub async fn data_source(&self, id: &str) -> Result<Value, CallError> {
        self.call(reqwest::Method::GET, &format!("/data_sources/{id}"), None).await
    }

    pub async fn create_page(&self, body: &Value) -> Result<CreatedPage, CallError> {
        let v = self.call(reqwest::Method::POST, "/pages", Some(body)).await?;
        let id = v.get("id").and_then(Value::as_str).ok_or(CallError::NoAnswer)?.to_string();
        let url = v.get("url").and_then(Value::as_str).map(str::to_string);
        Ok(CreatedPage { id, url })
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<Value, CallError> {
        let mut retries = 0;
        loop {
            let mut req = self
                .http
                .request(method.clone(), format!("{}{path}", self.base))
                .bearer_auth(&self.token)
                .header("Notion-Version", NOTION_VERSION);
            if let Some(b) = body {
                req = req.json(b);
            }
            let resp = req.send().await.map_err(|e| if e.is_connect() { CallError::Offline } else { CallError::NoAnswer })?;
            let status = resp.status().as_u16();
            if status == 429 && retries < 3 {
                retries += 1;
                let secs = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or(1)
                    .min(30);
                tokio::time::sleep(Duration::from_secs(secs)).await;
                continue;
            }
            // A body lost mid-read after a 2xx header is ambiguous; on an error status it does not matter.
            let text = resp.text().await.map_err(|_| CallError::NoAnswer)?;
            let json: Option<Value> = serde_json::from_str(&text).ok();
            if (200..300).contains(&status) {
                return json.ok_or(CallError::NoAnswer);
            }
            let field = |k: &str| json.as_ref().and_then(|j| j.get(k)).and_then(Value::as_str).unwrap_or("").to_string();
            let message: String = field("message").replace(&self.token, "[token]").chars().take(300).collect();
            return Err(CallError::Http { status, code: field("code"), message });
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    pub enum Reply {
        Http(String),
        Hang,
        Drop,
    }
    pub struct Req {
        pub head: String,
        pub body: String,
    }
    pub struct Fake {
        pub base: String,
        pub requests: Arc<Mutex<Vec<Req>>>,
    }

    pub fn reply(status: u16, extra_headers: &str, body: &str) -> Reply {
        Reply::Http(format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ))
    }

    pub async fn serve(replies: Vec<Reply>) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        tokio::spawn(async move {
            for r in replies {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head_end, len) = loop {
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break (buf.len(), 0);
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (p + 4, len);
                    }
                };
                while buf.len() < head_end + len {
                    let n = s.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf[..head_end.min(buf.len())]).to_string();
                let body = String::from_utf8_lossy(&buf[head_end.min(buf.len())..]).to_string();
                log.lock().unwrap().push(Req { head, body });
                match r {
                    Reply::Http(text) => {
                        let _ = s.write_all(text.as_bytes()).await;
                        let _ = s.shutdown().await;
                    }
                    Reply::Hang => {
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(10)).await;
                            drop(s);
                        });
                    }
                    Reply::Drop => drop(s),
                }
            }
        });
        Fake { base, requests }
    }

    impl Fake {
        pub async fn wait_requests(&self, n: usize) {
            for _ in 0..500 {
                if self.requests.lock().unwrap().len() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("fake server did not receive {n} requests");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    const TOKEN: &str = "ntn_SECRET123";
    const T: Duration = Duration::from_secs(5);

    fn client(f: &Fake) -> NotionClient {
        NotionClient::with_base(&f.base, TOKEN, T)
    }

    #[tokio::test]
    async fn sends_auth_and_version_headers() {
        let f = serve(vec![reply(200, "", r#"{"object":"user"}"#)]).await;
        client(&f).me().await.unwrap();
        let head = f.requests.lock().unwrap()[0].head.to_lowercase();
        assert!(head.starts_with("get /users/me "), "{head}");
        assert!(head.contains("authorization: bearer ntn_secret123"));
        assert!(head.contains("notion-version: 2026-03-11"));
    }

    #[tokio::test]
    async fn create_page_posts_body_and_reads_id_and_url() {
        let f = serve(vec![reply(200, "", r#"{"id":"p1","url":"https://www.notion.so/p1"}"#)]).await;
        let body = serde_json::json!({"parent":{"data_source_id":"d"},"properties":{}});
        let p = client(&f).create_page(&body).await.unwrap();
        assert_eq!((p.id.as_str(), p.url.as_deref()), ("p1", Some("https://www.notion.so/p1")));
        let r = &f.requests.lock().unwrap()[0];
        assert!(r.head.to_lowercase().starts_with("post /pages "));
        assert_eq!(serde_json::from_str::<Value>(&r.body).unwrap(), body);
    }

    #[tokio::test]
    async fn retries_429_after_retry_after_then_succeeds() {
        let f = serve(vec![reply(429, "Retry-After: 0\r\n", "{}"), reply(200, "", "{}")]).await;
        client(&f).me().await.unwrap();
        assert_eq!(f.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn gives_up_after_three_429_retries_as_definite() {
        let f = serve((0..4).map(|_| reply(429, "Retry-After: 0\r\n", r#"{"code":"rate_limited"}"#)).collect()).await;
        let e = client(&f).me().await.unwrap_err();
        assert!(matches!(e, CallError::Http { status: 429, .. }) && !e.is_ambiguous());
        assert_eq!(f.requests.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn classifies_statuses() {
        let cases = [
            (400, "validation_error", false, "mapping_broken"),
            (401, "unauthorized", false, "token_rejected"),
            (403, "restricted_resource", false, "not_shared"),
            (404, "object_not_found", false, "not_shared"),
            (409, "conflict_error", false, "offline"),
            (429, "rate_limited", false, "offline"),
            (500, "internal_server_error", true, "offline"),
            (503, "service_unavailable", true, "offline"),
        ];
        for (status, code, ambiguous, state) in cases {
            let body = format!(r#"{{"code":"{code}","message":"bad {TOKEN} here"}}"#);
            let f = serve((0..4).map(|_| reply(status, "Retry-After: 0\r\n", &body)).collect()).await;
            let e = client(&f).me().await.unwrap_err();
            assert_eq!(e.is_ambiguous(), ambiguous, "{status}");
            assert_eq!(e.state(), state, "{status}");
            assert!(!e.message("Personal tasks").contains(TOKEN), "{status}");
            assert!(!format!("{e:?}").contains(TOKEN), "{status}");
        }
    }

    #[test]
    fn a_400_naming_the_parent_path_is_not_found_but_a_property_named_parent_is_not() {
        let e = |m: &str| CallError::Http { status: 400, code: "validation_error".into(), message: m.into() };
        assert_eq!(e("body failed validation: body.parent.data_source_id should be a valid uuid").state(), "not_found");
        assert_eq!(e("body failed validation: body.properties.Parent.select.name is wrong").state(), "mapping_broken");
    }

    #[tokio::test]
    async fn messages_use_the_spec_copy() {
        for (status, body, want) in [
            (400, r#"{"message":"Due is not a property that exists."}"#, "Personal tasks: Notion refused it: Due is not a property that exists."),
            (404, "{}", "Personal tasks isn't shared with Conclave, or was deleted. In Notion: database ••• → Connections → add Conclave"),
            (429, "{}", "Notion asked to slow down. Try again in a minute."),
        ] {
            let f = serve((0..4).map(|_| reply(status, "Retry-After: 0\r\n", body)).collect()).await;
            assert_eq!(client(&f).me().await.unwrap_err().message("Personal tasks"), want);
        }
    }

    #[tokio::test]
    async fn timeout_and_dropped_connection_are_ambiguous_refused_connect_is_definite() {
        let f = serve(vec![Reply::Hang]).await;
        let e = NotionClient::with_base(&f.base, TOKEN, Duration::from_millis(300)).me().await.unwrap_err();
        assert!(matches!(e, CallError::NoAnswer) && e.is_ambiguous());

        let f = serve(vec![Reply::Drop]).await;
        assert!(client(&f).me().await.unwrap_err().is_ambiguous());

        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        drop(l);
        let e = NotionClient::with_base(&base, TOKEN, T).me().await.unwrap_err();
        assert!(matches!(e, CallError::Offline) && !e.is_ambiguous());
        assert_eq!(e.state(), "offline");
    }

    #[tokio::test]
    async fn ok_without_id_is_ambiguous() {
        let f = serve(vec![reply(200, "", "{}")]).await;
        let e = client(&f).create_page(&serde_json::json!({})).await.err().unwrap();
        assert!(e.is_ambiguous());
    }
}
