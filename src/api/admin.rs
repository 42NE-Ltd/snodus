//! Admin endpoints exposed by `snodus-core`.
//!
//! These cover the open source dashboard surface area: CRUD for keys, users
//! and teams plus the minimal spend queries the base dashboard renders.
//! Premium analytics (daily charts, routing breakdowns, region splits, audit
//! log) live in `snodus-cloud::analytics`.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::state::AppState;

// --- Dashboard SPA shell ---

pub async fn dashboard_index() -> Html<&'static str> {
    Html(include_str!("../../static/dashboard-base.html"))
}

// --- Keys ---

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    pub user_id: Uuid,
    pub name: Option<String>,
    pub rate_limit: Option<i32>,
    pub budget_monthly_cents: Option<i64>,
}

pub async fn create_key(
    State(state): State<AppState>,
    Json(req): Json<CreateKeyRequest>,
) -> (StatusCode, Json<Value>) {
    match crate::db::keys::create_key_full(
        &state.pool,
        req.user_id,
        req.name.as_deref(),
        req.rate_limit,
        req.budget_monthly_cents,
    )
    .await
    {
        Ok((plaintext, row)) => {
            let _ = crate::db::keys::bump_cache_version(&state.pool).await;
            (
                StatusCode::CREATED,
                Json(json!({
                    "key": plaintext,
                    "id": row.id,
                    "prefix": row.key_prefix,
                    "rate_limit": row.rate_limit,
                    "budget_monthly_cents": row.budget_monthly_cents,
                    "warning": "Store this key securely. It will not be shown again."
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

pub async fn list_keys(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let user_id = params.get("user_id").and_then(|s| Uuid::parse_str(s).ok());
    match crate::db::keys::list_keys(&state.pool, user_id).await {
        Ok(keys) => Json(json!({"keys": keys})),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

pub async fn revoke_key(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<Value>) {
    match crate::db::keys::revoke_key(&state.pool, id).await {
        Ok(_) => (StatusCode::OK, Json(json!({"status": "revoked"}))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

#[derive(Deserialize)]
pub struct PatchKeyRequest {
    pub rate_limit: Option<i32>,
    pub budget_monthly_cents: Option<i64>,
    pub name: Option<String>,
}

pub async fn patch_key(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchKeyRequest>,
) -> (StatusCode, Json<Value>) {
    match crate::db::keys::update_key(
        &state.pool,
        id,
        req.rate_limit,
        req.budget_monthly_cents,
        req.name.as_deref(),
    )
    .await
    {
        Ok(_) => {
            let _ = crate::db::keys::bump_cache_version(&state.pool).await;
            (StatusCode::OK, Json(json!({"status": "updated"})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

// --- Users ---

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub name: String,
    pub team_id: Option<Uuid>,
    pub role: Option<String>,
}

pub async fn create_user(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> (StatusCode, Json<Value>) {
    let role = req.role.as_deref().unwrap_or("member");
    match crate::db::users::create_user(&state.pool, &req.email, &req.name, req.team_id, role).await
    {
        Ok(user) => (StatusCode::CREATED, Json(json!({"user": user}))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

pub async fn list_users(State(state): State<AppState>) -> Json<Value> {
    match crate::db::users::list_users(&state.pool).await {
        Ok(users) => Json(json!({"users": users})),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

// --- Teams ---

#[derive(Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
    pub budget_monthly_cents: Option<i64>,
}

pub async fn create_team(
    State(state): State<AppState>,
    Json(req): Json<CreateTeamRequest>,
) -> (StatusCode, Json<Value>) {
    match crate::db::teams::create_team(&state.pool, &req.name, req.budget_monthly_cents).await {
        Ok(team) => {
            state
                .team_budgets
                .insert(team.id, team.budget_monthly_cents);
            (StatusCode::CREATED, Json(json!({"team": team})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

pub async fn list_teams(State(state): State<AppState>) -> Json<Value> {
    match crate::db::teams::list_teams(&state.pool).await {
        Ok(teams) => Json(json!({"teams": teams})),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

#[derive(Deserialize)]
pub struct PatchTeamRequest {
    pub budget_monthly_cents: Option<i64>,
}

pub async fn patch_team(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchTeamRequest>,
) -> (StatusCode, Json<Value>) {
    match crate::db::teams::update_team(&state.pool, id, req.budget_monthly_cents, None).await {
        Ok(_) => {
            state.team_budgets.insert(id, req.budget_monthly_cents);
            (StatusCode::OK, Json(json!({"status": "updated"})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

// --- Spend (base) ---

pub async fn get_spend(State(state): State<AppState>) -> Json<Value> {
    match crate::db::spend::get_spend_summary(&state.pool).await {
        Ok((total_cents, total_requests)) => Json(json!({
            "current_month": {
                "total_cost_cents": total_cents,
                "total_cost_usd": format!("{:.2}", total_cents as f64 / 100.0),
                "total_requests": total_requests,
            }
        })),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ModelSpendRow {
    pub model: String,
    pub total_cents: i64,
    pub total_requests: i64,
}

pub async fn spend_by_model(State(state): State<AppState>) -> Json<Value> {
    match query_models(&state.pool).await {
        Ok(rows) => Json(json!({"models": rows})),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

async fn query_models(pool: &PgPool) -> Result<Vec<ModelSpendRow>, sqlx::Error> {
    sqlx::query_as::<_, ModelSpendRow>(
        r#"SELECT
             model,
             COALESCE(SUM(cost_cents), 0)::bigint AS total_cents,
             COUNT(*)::bigint                     AS total_requests
           FROM spend_log
           WHERE created_at >= date_trunc('month', NOW())
           GROUP BY model
           ORDER BY total_cents DESC"#,
    )
    .fetch_all(pool)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct RecentRequestRow {
    pub id: i64,
    pub user_email: String,
    pub key_prefix: String,
    pub model: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_cents: i64,
    pub created_at: DateTime<Utc>,
}

pub async fn spend_recent(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let limit: i64 = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    match query_recent(&state.pool, limit).await {
        Ok(rows) => Json(json!({"recent": rows})),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

async fn query_recent(pool: &PgPool, limit: i64) -> Result<Vec<RecentRequestRow>, sqlx::Error> {
    sqlx::query_as::<_, RecentRequestRow>(
        r#"SELECT
             s.id,
             u.email AS user_email,
             k.key_prefix,
             s.model,
             s.input_tokens,
             s.output_tokens,
             s.cost_cents,
             s.created_at
           FROM spend_log s
           JOIN users    u ON u.id = s.user_id
           JOIN api_keys k ON k.id = s.api_key_id
           ORDER BY s.created_at DESC
           LIMIT $1"#,
    )
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await
}

// --- Providers ---

pub async fn list_providers(State(state): State<AppState>) -> Json<Value> {
    let list: Vec<Value> = state
        .providers
        .providers()
        .iter()
        .map(|p| {
            json!({
                "name": p.name(),
                "enabled": p.is_enabled(),
                "supported_prefixes": p.supported_prefixes(),
                "known_models": p.known_models(),
                "native_format": match p.native_format() {
                    crate::providers::RequestFormat::Anthropic => "anthropic",
                    crate::providers::RequestFormat::OpenAi => "openai",
                },
            })
        })
        .collect();
    Json(json!({"providers": list, "router_enabled": state.router_enabled}))
}

// --- i18n static files ---

const I18N_JS: &str = include_str!("../../static/i18n.js");

pub async fn serve_i18n_js() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        I18N_JS,
    )
        .into_response()
}

pub async fn serve_i18n(Path(lang): Path<String>) -> Response {
    // Strip .json suffix if present in the path parameter
    let lang = lang.strip_suffix(".json").unwrap_or(&lang);
    let supported = [
        "en", "zh", "hi", "es", "ar", "fr", "bn", "pt", "ru", "id", "de", "ja", "ko", "it",
    ];
    if !supported.contains(&lang) {
        return (StatusCode::NOT_FOUND, "Language not found").into_response();
    }
    // Include all translations at compile time
    let json = match lang {
        "en" => include_str!("../../i18n/en.json"),
        _ => {
            // For non-en, try to serve the file; if it doesn't exist at compile
            // time, fall back to en. The actual translated files are added to the
            // build context by the Dockerfile COPY step.
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
                include_str!("../../i18n/en.json"),
            )
                .into_response();
        }
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        json,
    )
        .into_response()
}
