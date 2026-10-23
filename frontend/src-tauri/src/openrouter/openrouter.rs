use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::command;

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenRouterModel {
    pub id: String,
    pub name: String,
    pub context_length: Option<u32>,
    pub prompt_price: Option<String>,
    pub completion_price: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterApiModel {
    id: String,
    name: Option<String>,
    context_length: Option<u32>,
    #[serde(default)]
    top_provider: Option<TopProvider>,
    #[serde(default)]
    pricing: Option<Pricing>,
}

#[derive(Debug, Deserialize, Default)]
struct TopProvider {
    context_length: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct Pricing {
    prompt: Option<String>,
    completion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterResponse {
    data: Vec<OpenRouterApiModel>,
}

#[command]
pub async fn get_openrouter_models() -> Result<Vec<OpenRouterModel>, String> {
    fetch_models(&reqwest::Client::new(), "https://openrouter.ai/api/v1/models").await
}

async fn fetch_models(client: &reqwest::Client, url: &str) -> Result<Vec<OpenRouterModel>, String> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Failed to make HTTP request: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP request failed with status: {}", response.status()));
    }

    let api_response: OpenRouterResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse JSON response: {}", e))?;

    let models = api_response
        .data
        .into_iter()
        .map(|m| OpenRouterModel {
            id: m.id,
            name: m.name.unwrap_or_else(|| "Unknown".to_string()),
            context_length: m.top_provider
                .as_ref()
                .and_then(|tp| tp.context_length)
                .or(m.context_length),
            prompt_price: m.pricing.as_ref().and_then(|p| p.prompt.clone()),
            completion_price: m.pricing.as_ref().and_then(|p| p.completion.clone()),
        })
        .collect();

    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};

    async fn serve_once(body: &'static str, stall: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf).await;
            if stall {
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            } else {
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        url
    }

    #[tokio::test]
    async fn fetch_models_parses_the_list() {
        let url = serve_once(
            r#"{"data":[{"id":"a/b","name":"Model B","context_length":8000,"pricing":{"prompt":"0.1","completion":"0.2"}}]}"#,
            false,
        )
        .await;
        let models = fetch_models(&reqwest::Client::new(), &url).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "a/b");
        assert_eq!(models[0].context_length, Some(8000));
        assert_eq!(models[0].prompt_price.as_deref(), Some("0.1"));
    }

    #[tokio::test]
    async fn stalled_server_fails_within_the_timeout() {
        let url = serve_once("", true).await;
        let started = Instant::now();
        let result = fetch_models(&reqwest::Client::new(), &url).await;
        assert!(result.is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
