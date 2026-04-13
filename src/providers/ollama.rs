use async_trait::async_trait;
use axum::{
    body::Body,
    http::{HeaderMap, StatusCode},
};
use bytes::Bytes;
use dashmap::DashSet;
use std::sync::Arc;
use std::time::Duration;

use super::{Provider, ProviderError, ProviderResponse, RequestFormat};

#[allow(dead_code)]
const DEFAULT_BASE: &str = "http://localhost:11434";
const STATIC_PREFIXES: &[&str] = &[
    "qwen",
    "llama",
    "phi",
    "mistral",
    "gemma",
    "deepseek",
    "codellama",
];

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    timeout_secs: u64,
    discovered: Arc<DashSet<String>>,
}

impl OllamaProvider {
    pub fn new(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        Self {
            client,
            base_url,
            timeout_secs: 60,
            discovered: Arc::new(DashSet::new()),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// One-shot discovery of installed models via `/api/tags`.
    pub async fn discover(&self) -> Result<Vec<String>, ProviderError> {
        let url = format!("{}/api/tags", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ProviderError::Connection(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ProviderError::Upstream {
                status: resp.status().as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ProviderError::Connection(e.to_string()))?;
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| ProviderError::Connection(e.to_string()))?;
        let mut names: Vec<String> = Vec::new();
        if let Some(arr) = body.get("models").and_then(|v| v.as_array()) {
            for m in arr {
                if let Some(name) = m.get("name").and_then(|v| v.as_str()) {
                    names.push(name.to_string());
                }
            }
        }
        self.discovered.clear();
        for n in &names {
            self.discovered.insert(n.clone());
        }
        Ok(names)
    }

    pub fn discovered_models(&self) -> Vec<String> {
        self.discovered.iter().map(|e| e.key().clone()).collect()
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn supported_prefixes(&self) -> &[&str] {
        STATIC_PREFIXES
    }

    fn handles_model(&self, model: &str) -> bool {
        // Accept if discovered OR matches a known prefix.
        if self.discovered.contains(model) {
            return true;
        }
        let lower = model.to_lowercase();
        STATIC_PREFIXES.iter().any(|p| lower.starts_with(p))
    }

    fn native_format(&self) -> RequestFormat {
        RequestFormat::Anthropic
    }

    fn known_models(&self) -> Vec<String> {
        self.discovered_models()
    }

    async fn forward(
        &self,
        _path: &str,
        _headers: &HeaderMap,
        body: Bytes,
    ) -> Result<ProviderResponse, ProviderError> {
        // Translate Anthropic-shaped request to Ollama /api/chat and back.
        let anth: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| ProviderError::Translation(format!("bad JSON: {e}")))?;

        let ollama_body = translate_request_to_ollama(&anth)?;
        let url = format!("{}/api/chat", self.base_url);

        let upstream = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&ollama_body).unwrap_or_default())
            .send()
            .await
            .map_err(|e| {
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

        if !status.is_success() {
            let body = upstream.text().await.unwrap_or_default();
            return Err(ProviderError::Upstream {
                status: status.as_u16(),
                body,
            });
        }

        let bytes = upstream
            .bytes()
            .await
            .map_err(|e| ProviderError::Connection(e.to_string()))?;
        let raw: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| ProviderError::Connection(e.to_string()))?;
        let anth_response = translate_response_from_ollama(&raw);
        let bytes = serde_json::to_vec(&anth_response).unwrap_or_default();

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );

        Ok(ProviderResponse {
            status,
            headers,
            body: Body::from(bytes),
            is_stream: false,
        })
    }
}

pub fn translate_request_to_ollama(
    anth: &serde_json::Value,
) -> Result<serde_json::Value, ProviderError> {
    use serde_json::json;
    let model = anth
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ProviderError::Translation("missing model".into()))?;

    let mut messages: Vec<serde_json::Value> = Vec::new();
    if let Some(system) = anth.get("system") {
        let text = match system {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(parts) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        if !text.is_empty() {
            messages.push(json!({"role": "system", "content": text}));
        }
    }

    if let Some(arr) = anth.get("messages").and_then(|v| v.as_array()) {
        for m in arr {
            let role = m
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("user")
                .to_string();
            let content = match m.get("content") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Array(parts)) => parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()).map(String::from))
                    .collect::<Vec<_>>()
                    .join(""),
                _ => String::new(),
            };
            messages.push(json!({"role": role, "content": content}));
        }
    }

    let mut options = serde_json::Map::new();
    if let Some(mt) = anth.get("max_tokens").and_then(|v| v.as_i64()) {
        options.insert("num_predict".into(), json!(mt));
    }
    if let Some(t) = anth.get("temperature") {
        options.insert("temperature".into(), t.clone());
    }

    Ok(json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "options": options,
    }))
}

pub fn translate_response_from_ollama(raw: &serde_json::Value) -> serde_json::Value {
    use serde_json::json;
    let model = raw
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("ollama")
        .to_string();
    let content = raw
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let input_tokens = raw
        .get("prompt_eval_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = raw.get("eval_count").and_then(|v| v.as_i64()).unwrap_or(0);
    json!({
        "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type": "text", "text": content}],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn translates_request_with_system() {
        let anth = json!({
            "model": "qwen2.5:3b",
            "max_tokens": 50,
            "system": "be helpful",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let out = translate_request_to_ollama(&anth).unwrap();
        assert_eq!(out["model"], "qwen2.5:3b");
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "be helpful");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(out["options"]["num_predict"], 50);
        assert_eq!(out["stream"], false);
    }

    #[test]
    fn translates_ollama_response_to_anthropic_shape() {
        let raw = json!({
            "model": "qwen2.5:3b",
            "message": {"role": "assistant", "content": "hello"},
            "prompt_eval_count": 12,
            "eval_count": 4
        });
        let out = translate_response_from_ollama(&raw);
        assert_eq!(out["content"][0]["text"], "hello");
        assert_eq!(out["usage"]["input_tokens"], 12);
        assert_eq!(out["usage"]["output_tokens"], 4);
    }

    #[test]
    fn handles_model_static_prefixes() {
        let p = OllamaProvider::new("http://localhost:11434".into());
        assert!(p.handles_model("qwen2.5:3b"));
        assert!(p.handles_model("llama3.2:1b"));
        assert!(!p.handles_model("claude-sonnet-4-5"));
    }
}
