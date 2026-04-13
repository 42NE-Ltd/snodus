use crate::auth::KeyInfo;
use crate::db::keys::hash_key;
use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Authenticated request info, injected into extensions
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub key_info: KeyInfo,
}

/// Middleware that validates the API key from Authorization header.
///
/// `snodus-core` keeps this hookless: cloud crates that need audit logging
/// wrap the request with their own middleware layer that reads `AuthContext`
/// from request extensions after this one runs.
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let key = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| req.headers().get("x-api-key").and_then(|v| v.to_str().ok()));

    let key = match key {
        Some(k) => k.trim().to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Missing API key. Use Authorization: Bearer sk-xxx"})),
            )
                .into_response();
        }
    };

    let hash = hash_key(&key);

    let info = match state.key_cache.get(&hash) {
        Some(entry) => entry.clone(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid API key"})),
            )
                .into_response();
        }
    };

    if !info.is_active {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "API key has been revoked"})),
        )
            .into_response();
    }

    if let Some(exp) = info.expires_at {
        if chrono::Utc::now() > exp {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "API key expired",
                    "hint": "Use your rotated key",
                })),
            )
                .into_response();
        }
    }

    // Update last_used in background (fire and forget)
    let pool = state.pool.clone();
    let key_id = info.key_id;
    tokio::spawn(async move {
        let _ = crate::db::keys::update_last_used(&pool, key_id).await;
    });

    req.extensions_mut().insert(AuthContext { key_info: info });
    next.run(req).await
}

/// Middleware that requires admin role
pub async fn admin_middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(auth) = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(basic) = auth.strip_prefix("Basic ") {
            if let Ok(decoded) = String::from_utf8(base64_decode(basic)) {
                if let Some((user, pass)) = decoded.split_once(':') {
                    if user == state.config.admin_username && pass == state.config.admin_password {
                        return next.run(req).await;
                    }
                }
            }
        }
    }

    if let Some(ctx) = req.extensions().get::<AuthContext>() {
        if ctx.key_info.role == "admin" {
            return next.run(req).await;
        }
    }

    (
        StatusCode::FORBIDDEN,
        Json(json!({"error": "Admin access required"})),
    )
        .into_response()
}

fn base64_decode(input: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .unwrap_or_default()
}
