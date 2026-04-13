use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use serde_json::{json, Value};
use std::time::Instant;

use crate::auth::middleware::AuthContext;
use crate::providers::{ProviderResponse, RequestFormat};
use crate::spend::{counter, SpendEntry};
use crate::state::AppState;

/// `POST /v1/messages` — Anthropic-format entry point. Supports `model: "auto"`
/// for the rule-based router and resolves provider via ProviderPool.
pub async fn proxy_messages(State(state): State<AppState>, req: Request) -> Response {
    let start = Instant::now();
    let auth = req.extensions().get::<AuthContext>().cloned();
    let headers = req.headers().clone();

    let body = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Failed to read body: {e}")})),
            )
                .into_response();
        }
    };

    let (effective_body, requested_model, routed_from, routing_method, routing_latency_ms) =
        apply_router(&state, body);

    let is_stream = serde_json::from_slice::<Value>(&effective_body)
        .ok()
        .and_then(|v| v.get("stream").and_then(|b| b.as_bool()))
        .unwrap_or(false);

    let provider = match state.providers.resolve(&requested_model) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let provider_name = provider.name().to_string();

    // If the resolved provider speaks OpenAI natively, translate the request.
    let (forward_path, forward_body) = match provider.native_format() {
        RequestFormat::Anthropic => ("/v1/messages".to_string(), effective_body.clone()),
        RequestFormat::OpenAi => {
            let parsed: Value = match serde_json::from_slice(&effective_body) {
                Ok(v) => v,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": format!("invalid JSON body: {e}")})),
                    )
                        .into_response();
                }
            };
            let translated =
                match crate::providers::translate::anthropic_to_openai::translate_request(&parsed) {
                    Ok(v) => v,
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error": format!("translation error: {e}")})),
                        )
                            .into_response();
                    }
                };
            let out = serde_json::to_vec(&translated).unwrap_or_default();
            ("/v1/chat/completions".to_string(), Bytes::from(out))
        }
    };

    let upstream = match provider
        .forward(&forward_path, &headers, forward_body)
        .await
    {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    let duration_ms = start.elapsed().as_millis() as i32;

    match provider.native_format() {
        RequestFormat::Anthropic => {
            build_anthropic_response(
                &state,
                upstream,
                auth,
                provider_name,
                requested_model,
                routed_from,
                routing_method,
                routing_latency_ms,
                duration_ms,
                is_stream,
            )
            .await
        }
        RequestFormat::OpenAi => {
            // Buffer, translate, then emit in Anthropic shape.
            let (parts, body) = upstream_parts(upstream);
            let bytes = match axum::body::to_bytes(body, 50 * 1024 * 1024).await {
                Ok(b) => b,
                Err(e) => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({"error": format!("Failed to read upstream: {e}")})),
                    )
                        .into_response();
                }
            };
            let parsed: Value = match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(_) => {
                    // Pass through as-is (probably a non-JSON error body).
                    return Response::from_parts(parts, Body::from(bytes));
                }
            };
            let anth_shape =
                match crate::providers::translate::openai_to_anthropic::translate_response(&parsed)
                {
                    Ok(v) => v,
                    Err(e) => {
                        return (
                            StatusCode::BAD_GATEWAY,
                            Json(json!({"error": format!("translation: {e}")})),
                        )
                            .into_response();
                    }
                };
            if let Some(auth_ctx) = &auth {
                let input = anth_shape["usage"]["input_tokens"].as_i64().unwrap_or(0) as i32;
                let output = anth_shape["usage"]["output_tokens"].as_i64().unwrap_or(0) as i32;
                let model = anth_shape["model"]
                    .as_str()
                    .unwrap_or(&requested_model)
                    .to_string();
                let cost =
                    counter::calculate_cost_with_provider(&provider_name, &model, input, output);
                let _ = state.spend_tx.send(SpendEntry {
                    api_key_id: auth_ctx.key_info.key_id,
                    user_id: auth_ctx.key_info.user_id,
                    team_id: auth_ctx.key_info.team_id,
                    model,
                    provider: provider_name.clone(),
                    input_tokens: input,
                    output_tokens: output,
                    cost_cents: cost,
                    duration_ms: Some(duration_ms),
                    key_budget_cents: auth_ctx.key_info.budget_monthly_cents,
                    routed_from: routed_from.clone(),
                    routing_method: routing_method.clone(),
                    routing_latency_ms,
                    region: None,
                });
            }
            let body_bytes = serde_json::to_vec(&anth_shape).unwrap_or_default();
            Response::from_parts(parts, Body::from(body_bytes))
        }
    }
}

fn upstream_parts(upstream: ProviderResponse) -> (axum::http::response::Parts, Body) {
    let mut builder = Response::builder().status(upstream.status);
    for (k, v) in upstream.headers.iter() {
        builder = builder.header(k, v);
    }
    let resp = builder.body(upstream.body).unwrap();
    resp.into_parts()
}

#[allow(clippy::too_many_arguments)]
async fn build_anthropic_response(
    state: &AppState,
    upstream: ProviderResponse,
    auth: Option<AuthContext>,
    provider_name: String,
    requested_model: String,
    routed_from: Option<String>,
    routing_method: Option<String>,
    routing_latency_ms: Option<i32>,
    duration_ms: i32,
    is_stream: bool,
) -> Response {
    if !is_stream {
        let (parts, body) = upstream_parts(upstream);
        match axum::body::to_bytes(body, 50 * 1024 * 1024).await {
            Ok(resp_bytes) => {
                if let Some(auth_ctx) = &auth {
                    if let Some((model, input, output)) =
                        counter::extract_usage_from_json(&resp_bytes)
                    {
                        let cost = counter::calculate_cost_with_provider(
                            &provider_name,
                            &model,
                            input,
                            output,
                        );
                        let _ = state.spend_tx.send(SpendEntry {
                            api_key_id: auth_ctx.key_info.key_id,
                            user_id: auth_ctx.key_info.user_id,
                            team_id: auth_ctx.key_info.team_id,
                            model: model.clone(),
                            provider: provider_name.clone(),
                            input_tokens: input,
                            output_tokens: output,
                            cost_cents: cost,
                            duration_ms: Some(duration_ms),
                            key_budget_cents: auth_ctx.key_info.budget_monthly_cents,
                            routed_from: routed_from.clone(),
                            routing_method: routing_method.clone(),
                            routing_latency_ms,
                            region: None,
                        });
                    }
                }
                Response::from_parts(parts, Body::from(resp_bytes))
            }
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("Failed to read upstream: {e}")})),
            )
                .into_response(),
        }
    } else {
        let fallback_model = requested_model.clone();
        if let Some(auth_ctx) = auth {
            let spend_tx = state.spend_tx.clone();
            let region: Option<String> = None;
            let (parts, body) = upstream_parts(upstream);
            let routed_from = routed_from.clone();
            let routing_method = routing_method.clone();
            let provider_name_cb = provider_name.clone();
            let intercepted = crate::providers::sse::intercept(body, move |usage| {
                let model = usage.model.unwrap_or(fallback_model.clone());
                let cost = counter::calculate_cost_with_provider(
                    &provider_name_cb,
                    &model,
                    usage.input_tokens,
                    usage.output_tokens,
                );
                let _ = spend_tx.send(SpendEntry {
                    api_key_id: auth_ctx.key_info.key_id,
                    user_id: auth_ctx.key_info.user_id,
                    team_id: auth_ctx.key_info.team_id,
                    model,
                    provider: provider_name_cb.clone(),
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    cost_cents: cost,
                    duration_ms: Some(duration_ms),
                    key_budget_cents: auth_ctx.key_info.budget_monthly_cents,
                    routed_from: routed_from.clone(),
                    routing_method: routing_method.clone(),
                    routing_latency_ms,
                    region: region.clone(),
                });
            });
            Response::from_parts(parts, intercepted)
        } else {
            let (parts, body) = upstream_parts(upstream);
            Response::from_parts(parts, body)
        }
    }
}

/// If `model == "auto"` and router is enabled, picks a concrete model and rewrites
/// the body. Returns (new_body, requested_model, routed_from, method, latency_ms).
fn apply_router(
    state: &AppState,
    body: Bytes,
) -> (Bytes, String, Option<String>, Option<String>, Option<i32>) {
    let Ok(mut parsed) = serde_json::from_slice::<Value>(&body) else {
        return (body, "unknown".into(), None, None, None);
    };
    let requested = parsed
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if !state.router_enabled || requested != "auto" {
        return (body, requested, None, None, None);
    }

    let decision = state.router.route(&parsed);
    tracing::info!(
        target = %decision.target_model,
        reason = %decision.reason,
        "Router routed 'auto'"
    );
    parsed["model"] = Value::String(decision.target_model.clone());
    let new_body = serde_json::to_vec(&parsed).unwrap_or_default();
    (
        Bytes::from(new_body),
        decision.target_model,
        Some("auto".to_string()),
        Some(decision.method.as_str().to_string()),
        Some(decision.latency_ms),
    )
}

/// `POST /v1/messages/count_tokens` — forward to the resolved provider.
pub async fn proxy_count_tokens(State(state): State<AppState>, req: Request) -> Response {
    let headers = req.headers().clone();
    let body = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Failed to read body: {e}")})),
            )
                .into_response();
        }
    };
    let model = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str().map(String::from)))
        .unwrap_or_else(|| "claude-sonnet-4-5".into());

    let provider = match state.providers.resolve(&model) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    match provider
        .forward("/v1/messages/count_tokens", &headers, body)
        .await
    {
        Ok(r) => assemble(r),
        Err(e) => e.into_response(),
    }
}

/// `POST /v1/chat/completions` — native OpenAI entry point.
pub async fn proxy_chat_completions(State(state): State<AppState>, req: Request) -> Response {
    let start = Instant::now();
    let auth = req.extensions().get::<AuthContext>().cloned();
    let headers = req.headers().clone();
    let body = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Failed to read body: {e}")})),
            )
                .into_response();
        }
    };
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("invalid JSON: {e}")})),
            )
                .into_response();
        }
    };
    let model = parsed
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let provider = match state.providers.resolve(&model) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if provider.native_format() != RequestFormat::OpenAi {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("model {model} is handled by provider {} which does not expose /v1/chat/completions", provider.name())
            })),
        )
            .into_response();
    }
    let upstream = match provider
        .forward("/v1/chat/completions", &headers, body)
        .await
    {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let duration_ms = start.elapsed().as_millis() as i32;
    let provider_name = provider.name().to_string();
    let (parts, body) = upstream_parts(upstream);
    match axum::body::to_bytes(body, 50 * 1024 * 1024).await {
        Ok(resp_bytes) => {
            if let Some(auth_ctx) = &auth {
                if let Some((model, input, output)) =
                    counter::extract_usage_from_openai_json(&resp_bytes)
                {
                    let cost = counter::calculate_cost_with_provider(
                        &provider_name,
                        &model,
                        input,
                        output,
                    );
                    let _ = state.spend_tx.send(SpendEntry {
                        api_key_id: auth_ctx.key_info.key_id,
                        user_id: auth_ctx.key_info.user_id,
                        team_id: auth_ctx.key_info.team_id,
                        model,
                        provider: provider_name.clone(),
                        input_tokens: input,
                        output_tokens: output,
                        cost_cents: cost,
                        duration_ms: Some(duration_ms),
                        key_budget_cents: auth_ctx.key_info.budget_monthly_cents,
                        routed_from: None,
                        routing_method: None,
                        routing_latency_ms: None,
                        region: None,
                    });
                }
            }
            Response::from_parts(parts, Body::from(resp_bytes))
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("Failed to read upstream: {e}")})),
        )
            .into_response(),
    }
}

fn assemble(resp: ProviderResponse) -> Response {
    let mut builder = Response::builder().status(resp.status);
    for (k, v) in resp.headers.iter() {
        builder = builder.header(k, v);
    }
    builder.body(resp.body).unwrap()
}

// Ensure HeaderMap is referenced (silences unused import on some configurations).
const _: fn(&HeaderMap) = |_| {};
