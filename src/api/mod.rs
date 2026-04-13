pub mod admin;
pub mod health;
pub mod proxy;

use axum::{
    middleware,
    routing::{delete, get, patch, post},
    Router,
};

use crate::auth::middleware::{admin_middleware, auth_middleware};
use crate::budget::middleware::budget_middleware;
use crate::ratelimit::middleware::rate_limit_middleware;
use crate::state::AppState;

/// The open source router. It exposes:
/// - public: `/health`, `/health/db`, `/admin/` (dashboard SHELL only)
/// - proxy: `/v1/messages`, `/v1/messages/count_tokens`, `/v1/chat/completions`
///   protected by Auth → RateLimit → Budget pipeline
/// - admin: CRUD for keys/users/teams + spend summary endpoints used by the
///   base dashboard
///
/// Cloud crates can call this and then `.merge()` premium routes
/// (`/signup`, `/auth/login`, `/scim/v2/*`, `/admin/audit`, `/billing/*`).
pub fn router(state: AppState) -> Router {
    // Public routes (no auth). The dashboard SHELL is public; the JS inside
    // it sends Basic auth on every fetch to the protected /admin/* API
    // endpoints, so anonymous visitors get the shell but no data.
    let public_router = Router::new()
        .route("/health", get(health::health))
        .route("/health/db", get(health::health_db))
        .route("/admin/", get(admin::dashboard_index))
        .route("/i18n/{lang}.json", get(admin::serve_i18n))
        .route("/static/i18n.js", get(admin::serve_i18n_js));

    // Proxy routes — pipeline: Auth → RateLimit → Budget → Proxy handler.
    // `route_layer` (vs `layer`) keeps the middleware scoped to handlers
    // registered on this Router and not its 404 fallback.
    let proxy = Router::new()
        .route("/v1/messages", post(proxy::proxy_messages))
        .route("/v1/messages/count_tokens", post(proxy::proxy_count_tokens))
        .route("/v1/chat/completions", post(proxy::proxy_chat_completions))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            budget_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // Admin routes (require admin role or Basic auth). Open source set:
    // CRUD for keys/users/teams plus the basic spend / by_model / recent
    // queries the base dashboard needs. No advanced analytics here — those
    // live in snodus-cloud.
    let admin = Router::new()
        .route("/admin/keys", post(admin::create_key))
        .route("/admin/keys", get(admin::list_keys))
        .route("/admin/keys/{id}", delete(admin::revoke_key))
        .route("/admin/keys/{id}", patch(admin::patch_key))
        .route("/admin/users", post(admin::create_user))
        .route("/admin/users", get(admin::list_users))
        .route("/admin/teams", post(admin::create_team))
        .route("/admin/teams", get(admin::list_teams))
        .route("/admin/teams/{id}", patch(admin::patch_team))
        .route("/admin/spend", get(admin::get_spend))
        .route("/admin/spend/by_model", get(admin::spend_by_model))
        .route("/admin/spend/recent", get(admin::spend_recent))
        .route("/admin/providers", get(admin::list_providers))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            admin_middleware,
        ));

    Router::new()
        .merge(public_router)
        .merge(proxy)
        .merge(admin)
        .with_state(state)
}
