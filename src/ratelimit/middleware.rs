use axum::{
    extract::{Request, State},
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::auth::middleware::AuthContext;
use crate::ratelimit::RateLimitResult;
use crate::state::AppState;

pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(ctx) = req.extensions().get::<AuthContext>().cloned() else {
        // Shouldn't happen — auth middleware runs first — but fail open with 500.
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Missing auth context"})),
        )
            .into_response();
    };

    let limit = ctx.key_info.rate_limit.max(0) as u32;
    // 0 means unlimited — skip the check entirely.
    if limit == 0 {
        return next.run(req).await;
    }

    let result = state.rate_limiter.check(ctx.key_info.key_id, limit);
    match result {
        RateLimitResult::Allowed {
            remaining,
            limit,
            reset_secs,
        } => {
            let mut resp = next.run(req).await;
            let headers = resp.headers_mut();
            headers.insert(
                HeaderName::from_static("x-ratelimit-limit"),
                HeaderValue::from(limit),
            );
            headers.insert(
                HeaderName::from_static("x-ratelimit-remaining"),
                HeaderValue::from(remaining),
            );
            headers.insert(
                HeaderName::from_static("x-ratelimit-reset"),
                HeaderValue::from(reset_secs),
            );
            resp
        }
        RateLimitResult::Denied {
            retry_after_secs,
            limit,
        } => {
            tracing::info!(
                key_id = %ctx.key_info.key_id,
                limit,
                "Rate limit exceeded"
            );

            let body = Json(json!({
                "error": "Rate limit exceeded",
                "retry_after": retry_after_secs,
                "limit": limit,
                "remaining": 0,
            }));
            let mut resp = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
            let headers = resp.headers_mut();
            headers.insert(
                HeaderName::from_static("x-ratelimit-limit"),
                HeaderValue::from(limit),
            );
            headers.insert(
                HeaderName::from_static("x-ratelimit-remaining"),
                HeaderValue::from(0u32),
            );
            headers.insert(
                HeaderName::from_static("x-ratelimit-reset"),
                HeaderValue::from(retry_after_secs),
            );
            headers.insert(
                HeaderName::from_static("retry-after"),
                HeaderValue::from(retry_after_secs),
            );
            resp
        }
    }
}
