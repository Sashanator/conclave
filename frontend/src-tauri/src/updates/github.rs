//! GitHub `releases/latest` lookup: finds the `latest.json` asset's API URL. GitHub JSON is untrusted.
//! The token goes only to api.github.com; every message that leaves `updates/` passes through `scrub`.

use log::warn;
use std::time::Duration;

pub const OWNER: &str = "Sashanator";
pub const REPO: &str = "conclave";
pub const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/Sashanator/conclave/releases/latest";
pub const ASSET_PREFIX: &str = "https://api.github.com/repos/Sashanator/conclave/releases/assets/";
pub const NETWORK_MESSAGE: &str = "Couldn't reach GitHub. Check your internet connection.";

/// Exact match: https, api.github.com, no port/userinfo/query/fragment, `/repos/<owner>/<repo>/releases/assets/<digits>`.
pub fn is_conclave_asset(u: &url::Url) -> bool {
    let id = u
        .path()
        .strip_prefix("/repos/Sashanator/conclave/releases/assets/");
    u.scheme() == "https"
        && u.host_str() == Some("api.github.com")
        && u.port().is_none()
        && u.username().is_empty()
        && u.password().is_none()
        && u.query().is_none()
        && u.fragment().is_none()
        && id.is_some_and(|d| !d.is_empty() && d.len() <= 20 && d.bytes().all(|b| b.is_ascii_digit()))
}

pub fn latest_json_url(release: &serde_json::Value) -> Result<url::Url, String> {
    let raw = release
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|a| {
            a.iter()
                .find(|x| x.get("name").and_then(|n| n.as_str()) == Some("latest.json"))
        })
        .and_then(|x| x.get("url"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| "The latest release has no update file (latest.json).".to_string())?;
    url::Url::parse(raw)
        .ok()
        .filter(is_conclave_asset)
        .ok_or_else(|| "The update file points outside the Conclave repository.".to_string())
}

pub fn status_message(status: u16) -> String {
    match status {
        401 | 403 => "Token rejected by GitHub: check it has read access to Conclave.".into(),
        404 => "No published Conclave release found, or the token can't see the Conclave repository."
            .into(),
        s => format!("GitHub answered {s}. Try again later."),
    }
}

/// Redacts the exact token, and anything shaped like a GitHub token (a different one may appear in a body).
pub fn scrub(msg: &str, token: &str) -> String {
    static SHAPE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?:github_pat|gh[pousr])_[A-Za-z0-9_]+").unwrap());
    let m = if token.is_empty() { msg.to_string() } else { msg.replace(token, "[token]") };
    SHAPE.replace_all(&m, "[token]").into_owned()
}

pub fn validate_token(raw: &str) -> Result<String, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Err("Paste a token first.".into());
    }
    if t.len() > 255 || !t.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("That doesn't look like a GitHub token.".into());
    }
    Ok(t.to_string())
}

/// Largest `releases/latest` body we will read.
const MAX_BODY: usize = 2 * 1024 * 1024;

pub async fn find_latest_json(token: &str) -> Result<url::Url, String> {
    // Defence in depth: the endpoint is a literal, but check before the token is sent.
    let endpoint = url::Url::parse(LATEST_RELEASE_URL).map_err(|e| e.to_string())?;
    if endpoint.host_str() != Some("api.github.com") || endpoint.scheme() != "https" {
        return Err("The update endpoint is not the Conclave repository.".into());
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("Conclave/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| {
            warn!("updates: http client: {}", scrub(&e.without_url().to_string(), token));
            NETWORK_MESSAGE.to_string()
        })?;
    let net = |e: reqwest::Error| {
        warn!("updates: GitHub request failed: {}", scrub(&e.without_url().to_string(), token));
        NETWORK_MESSAGE.to_string()
    };
    let mut resp = client
        .get(endpoint)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(net)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(status_message(status.as_u16()));
    }
    if resp.content_length().is_some_and(|n| n > MAX_BODY as u64) {
        return Err("GitHub's answer was too large to read.".into());
    }
    let mut bytes = Vec::new();
    while let Some(c) = resp.chunk().await.map_err(net)? {
        if bytes.len() + c.len() > MAX_BODY {
            return Err("GitHub's answer was too large to read.".into());
        }
        bytes.extend_from_slice(&c);
    }
    let release: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| "GitHub's answer could not be read.".to_string())?;
    latest_json_url(&release).map_err(|m| scrub(&m, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE_JSON: &str = r#"{"tag_name":"v1.1.0","assets":[
      {"name":"Conclave_1.1.0_x64-setup.exe","url":"https://api.github.com/repos/Sashanator/conclave/releases/assets/700000000","browser_download_url":"https://github.com/Sashanator/conclave/releases/download/v1.1.0/Conclave_1.1.0_x64-setup.exe"},
      {"name":"latest.json","url":"https://api.github.com/repos/Sashanator/conclave/releases/assets/700000001","browser_download_url":"https://github.com/Sashanator/conclave/releases/download/v1.1.0/latest.json"}]}"#;

    fn u(s: &str) -> url::Url {
        url::Url::parse(s).unwrap()
    }

    #[test]
    fn finds_latest_json_api_url() {
        let r: serde_json::Value = serde_json::from_str(RELEASE_JSON).unwrap();
        assert_eq!(
            latest_json_url(&r).unwrap().as_str(),
            "https://api.github.com/repos/Sashanator/conclave/releases/assets/700000001"
        );
    }

    #[test]
    fn missing_latest_json_is_a_plain_error() {
        for v in [
            serde_json::json!({"assets":[{"name":"x.exe","url":"https://api.github.com/repos/Sashanator/conclave/releases/assets/1"}]}),
            serde_json::json!({}),
            serde_json::json!({"assets":[{"name":"latest.json"}]}),
        ] {
            assert_eq!(
                latest_json_url(&v).unwrap_err(),
                "The latest release has no update file (latest.json)."
            );
        }
    }

    #[test]
    fn foreign_asset_url_is_refused() {
        let v = serde_json::json!({"assets":[{"name":"latest.json","url":"https://evil.example/repos/Sashanator/conclave/releases/assets/1"}]});
        assert_eq!(
            latest_json_url(&v).unwrap_err(),
            "The update file points outside the Conclave repository."
        );
    }

    #[test]
    fn is_conclave_asset_cases() {
        let base = "https://api.github.com/repos/Sashanator/conclave/releases/assets/5";
        assert!(is_conclave_asset(&u(base)));
        for bad in [
            "http://api.github.com/repos/Sashanator/conclave/releases/assets/5",
            "https://github.com/Sashanator/conclave/releases/download/v1/latest.json",
            "https://api.github.com/repos/Other/conclave/releases/assets/5",
            "https://api.github.com/repos/Sashanator/conclave/releases/assets/5?x=1",
            "https://user@api.github.com/repos/Sashanator/conclave/releases/assets/5",
            "https://api.github.com:444/repos/Sashanator/conclave/releases/assets/5",
            "https://api.github.com/repos/Sashanator/conclave/releases/assets/abc",
            "https://api.github.com/repos/Sashanator/conclave/releases/assets/",
            "https://api.github.com/repos/Sashanator/conclave/releases/assets/5/x",
        ] {
            assert!(!is_conclave_asset(&u(bad)), "{bad}");
        }
    }

    #[test]
    fn status_messages() {
        assert!(status_message(401).starts_with("Token rejected"));
        assert_eq!(status_message(401), status_message(403));
        assert!(status_message(404).starts_with("No published Conclave release"));
        assert_eq!(status_message(500), "GitHub answered 500. Try again later.");
    }

    #[test]
    fn messages_never_contain_the_token() {
        let t = "github_pat_SECRET123";
        let mut all: Vec<String> = [401u16, 403, 404, 500, 502].iter().map(|s| status_message(*s)).collect();
        all.push(NETWORK_MESSAGE.into());
        let body = serde_json::json!({"assets":[{"name":"latest.json","url":format!("https://evil.example/{t}")}]});
        all.push(latest_json_url(&body).unwrap_err());
        all.push(scrub(&format!("failed with {t} here"), t));
        for m in &all {
            assert!(!m.contains(t), "{m}");
        }
        assert_eq!(scrub("abc", ""), "abc");
        // Other tokens in a message are redacted too (fake, low-entropy fixtures).
        for other in ["github_pat_AAAA_bbbb", "ghp_aaaa", "gho_aaaa", "ghu_aaaa", "ghs_aaaa", "ghr_aaaa"] {
            let s = scrub(&format!("x {other} y"), "unrelated");
            assert_eq!(s, "x [token] y");
        }
    }

    #[test]
    fn validate_token_trims_and_rejects() {
        assert_eq!(validate_token("  abc_123 \n").unwrap(), "abc_123");
        assert_eq!(validate_token("   ").unwrap_err(), "Paste a token first.");
        for bad in ["has space inside", "tab\there", "ключ", &"a".repeat(256)] {
            let e = validate_token(bad).unwrap_err();
            assert_eq!(e, "That doesn't look like a GitHub token.");
            assert!(!e.contains(bad));
        }
    }
}
