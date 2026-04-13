pub mod anthropic;
pub mod ollama;
pub mod openai;
pub mod pool;
pub mod router;
pub mod sse;
pub mod translate;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use serde_json::json;

#[derive(Debug)]
#[allow(dead_code)]
pub enum ProviderError {
    Upstream { status: u16, body: String },
    Timeout { after_secs: u64 },
    Connection(String),
    ModelNotSupported(String),
    AuthFailed(String),
    ProviderDisabled(String),
    Translation(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Upstream { status, body } => write!(f, "Upstream error {status}: {body}"),
            Self::Timeout { after_secs } => write!(f, "Timed out after {after_secs}s"),
            Self::Connection(e) => write!(f, "Connection error: {e}"),
            Self::ModelNotSupported(m) => write!(f, "Model not supported: {m}"),
            Self::AuthFailed(e) => write!(f, "Auth failed: {e}"),
            Self::ProviderDisabled(p) => write!(f, "Provider disabled: {p}"),
            Self::Translation(e) => write!(f, "Translation error: {e}"),
        }
    }
}

impl IntoResponse for ProviderError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            Self::Upstream { status, .. } => (
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY),
                self.to_string(),
            ),
            Self::Timeout { .. } => (StatusCode::GATEWAY_TIMEOUT, self.to_string()),
            Self::Connection(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            Self::ModelNotSupported(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            Self::AuthFailed(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            Self::ProviderDisabled(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            Self::Translation(_) => (StatusCode::BAD_REQUEST, self.to_string()),
        };
        (status, Json(json!({"error": msg}))).into_response()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestFormat {
    Anthropic,
    OpenAi,
}

pub struct ProviderResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Body,
    pub is_stream: bool,
}

/// A registered backend that can handle one or more model families.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Unique identifier ("anthropic", "openai", "ollama").
    fn name(&self) -> &str;

    /// Prefix patterns accepted by `handles_model` (e.g. ["claude-"]).
    fn supported_prefixes(&self) -> &[&str];

    fn handles_model(&self, model: &str) -> bool {
        self.supported_prefixes()
            .iter()
            .any(|p| model.starts_with(p))
    }

    /// The request format this provider exposes on its native endpoint.
    fn native_format(&self) -> RequestFormat {
        RequestFormat::Anthropic
    }

    /// True if the provider is currently reachable/configured.
    fn is_enabled(&self) -> bool {
        true
    }

    /// Optional: list of currently available models (for Ollama discovery etc.).
    fn known_models(&self) -> Vec<String> {
        Vec::new()
    }

    /// Forward a request whose body is already in the provider's native format.
    async fn forward(
        &self,
        path: &str,
        headers: &HeaderMap,
        body: Bytes,
    ) -> Result<ProviderResponse, ProviderError>;
}
