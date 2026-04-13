use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::auth::middleware::AuthContext;
use crate::budget::{next_month_reset, BudgetResult};
use crate::state::AppState;

pub async fn budget_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(ctx) = req.extensions().get::<AuthContext>().cloned() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Missing auth context"})),
        )
            .into_response();
    };

    // Per-key budget
    if let Some(limit) = ctx.key_info.budget_monthly_cents {
        let result = state
            .budget
            .check_key_budget(ctx.key_info.key_id, Some(limit))
            .await;
        if let BudgetResult::Denied {
            limit_cents,
            spent_cents,
        } = result
        {
            tracing::info!(
                key_id = %ctx.key_info.key_id,
                spent_cents,
                limit_cents,
                "Per-key budget exceeded"
            );
            return budget_denied_response("key", spent_cents, limit_cents);
        }
    }

    // Per-team budget
    if let Some(team_id) = ctx.key_info.team_id {
        let team_limit = state
            .team_budgets
            .get(&team_id)
            .map(|r| *r.value())
            .unwrap_or(None);
        if let Some(limit) = team_limit {
            let result = state.budget.check_team_budget(team_id, Some(limit)).await;
            if let BudgetResult::Denied {
                limit_cents,
                spent_cents,
            } = result
            {
                tracing::info!(
                    team_id = %team_id,
                    spent_cents,
                    limit_cents,
                    "Team budget exceeded"
                );
                return budget_denied_response("team", spent_cents, limit_cents);
            }
        }
    }

    next.run(req).await
}

fn budget_denied_response(scope: &str, spent_cents: i64, limit_cents: i64) -> Response {
    (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({
            "error": "Budget exceeded",
            "scope": scope,
            "budget_cents": limit_cents,
            "spent_cents": spent_cents,
            "resets_at": next_month_reset().to_rfc3339(),
        })),
    )
        .into_response()
}
