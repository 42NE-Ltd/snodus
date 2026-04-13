use async_trait::async_trait;
use axum::{
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
};
use bytes::Bytes;
use std::time::Duration;

use super::{Provider, ProviderError, ProviderResponse, RequestFormat};

const DEFAULT_BASE: &str = "https://api.openai.com";
const SUPPORTED_PREFIXES: &[&str] = &["gpt-", "o1", "o3", "o4", "chatgpt-"];

const SKIP_RESPONSE_HEADERS: &[&str] = &[
    "connection",
    "content-length",
    "transfer-encoding",
    "content-encoding",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "upgrade",
];

pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    timeout_secs: u64,
}

impl OpenAiProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_base(api_key, DEFAULT_BASE.into(), 120)
    }

    pub fn with_base(api_key: String, base_url: String, timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("reqwest client");
        Self {
            client,
            api_key,
            base_url,
            timeout_secs,
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supported_prefixes(&self) -> &[&str] {
        SUPPORTED_PREFIXES
    }

    fn native_format(&self) -> RequestFormat {
        RequestFormat::OpenAi
    }

    fn is_enabled(&self) -> bool {
        !self.api_key.is_empty()
    }

    async fn forward(
        &self,
        path: &str,
        _incoming_headers: &HeaderMap,
        body: Bytes,
    ) -> Result<ProviderResponse, ProviderError> {
        // Path passed in is the provider-native path (/v1/chat/completions).
        let url = format!("{}{}", self.base_url, path);

        let req = self
            .client
            .post(&url)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .body(body);

        let upstream = req.send().await.map_err(|e| {
            if e.is_timeout() {
                ProviderError::Timeout {
                    after_secs: self.timeout_secs,
                }
            } else {
                ProviderError::Connection(e.to_string())
            }
        })?;

        let status =
            StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let is_stream = upstream
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        let mut out_headers = HeaderMap::new();
        for (name, value) in upstream.headers() {
            let lower = name.as_str().to_ascii_lowercase();
            if SKIP_RESPONSE_HEADERS.contains(&lower.as_str()) {
                continue;
            }
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(name.as_str().as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                out_headers.insert(n, v);
            }
        }

        Ok(ProviderResponse {
            status,
            headers: out_headers,
            body: Body::from_stream(upstream.bytes_stream()),
            is_stream,
        })
    }
}
