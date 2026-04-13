use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct SpendRow {
    pub id: i64,
    pub api_key_id: Uuid,
    pub user_id: Uuid,
    pub team_id: Option<Uuid>,
    pub model: String,
    pub provider: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_cents: i64,
    pub duration_ms: Option<i32>,
    pub created_at: DateTime<Utc>,
}

pub struct SpendInsert {
    pub api_key_id: Uuid,
    pub user_id: Uuid,
    pub team_id: Option<Uuid>,
    pub model: String,
    pub provider: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_cents: i64,
    pub duration_ms: Option<i32>,
    pub routed_from: Option<String>,
    pub routing_method: Option<String>,
    pub routing_latency_ms: Option<i32>,
    pub region: Option<String>,
}

pub async fn insert_spend_batch(pool: &PgPool, entries: &[SpendInsert]) -> Result<(), sqlx::Error> {
    for e in entries {
        sqlx::query(
            r#"INSERT INTO spend_log
               (api_key_id, user_id, team_id, model, provider, input_tokens, output_tokens,
                cost_cents, duration_ms, routed_from, routing_method, routing_latency_ms, region)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"#,
        )
        .bind(e.api_key_id)
        .bind(e.user_id)
        .bind(e.team_id)
        .bind(&e.model)
        .bind(&e.provider)
        .bind(e.input_tokens)
        .bind(e.output_tokens)
        .bind(e.cost_cents)
        .bind(e.duration_ms)
        .bind(&e.routed_from)
        .bind(&e.routing_method)
        .bind(e.routing_latency_ms)
        .bind(&e.region)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn get_total_spend_current_month(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<i64, sqlx::Error> {
    let row: (Option<i64>,) = sqlx::query_as(
        r#"SELECT COALESCE(SUM(cost_cents), 0)::bigint
           FROM spend_log
           WHERE user_id = $1
             AND created_at >= date_trunc('month', NOW())"#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0.unwrap_or(0))
}

pub async fn get_spend_summary(pool: &PgPool) -> Result<(i64, i64), sqlx::Error> {
    let row: (Option<i64>, Option<i64>) = sqlx::query_as(
        r#"SELECT
             COALESCE(SUM(cost_cents), 0)::bigint,
             COUNT(*)::bigint
           FROM spend_log
           WHERE created_at >= date_trunc('month', NOW())"#,
    )
    .fetch_one(pool)
    .await?;
    Ok((row.0.unwrap_or(0), row.1.unwrap_or(0)))
}

pub async fn get_key_spend_current_month(pool: &PgPool, key_id: Uuid) -> Result<i64, sqlx::Error> {
    let row: (Option<i64>,) = sqlx::query_as(
        r#"SELECT COALESCE(SUM(cost_cents), 0)::bigint
           FROM spend_log
           WHERE api_key_id = $1
             AND created_at >= date_trunc('month', NOW())"#,
    )
    .bind(key_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0.unwrap_or(0))
}

pub async fn get_team_spend_current_month(
    pool: &PgPool,
    team_id: Uuid,
) -> Result<i64, sqlx::Error> {
    let row: (Option<i64>,) = sqlx::query_as(
        r#"SELECT COALESCE(SUM(cost_cents), 0)::bigint
           FROM spend_log
           WHERE team_id = $1
             AND created_at >= date_trunc('month', NOW())"#,
    )
    .bind(team_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0.unwrap_or(0))
}

pub async fn get_team_tokens_current_month(
    pool: &PgPool,
    team_id: Uuid,
) -> Result<(i64, i64), sqlx::Error> {
    let row: (Option<i64>, Option<i64>) = sqlx::query_as(
        r#"SELECT
             COALESCE(SUM(input_tokens), 0)::bigint,
             COALESCE(SUM(output_tokens), 0)::bigint
           FROM spend_log
           WHERE team_id = $1
             AND created_at >= date_trunc('month', NOW())"#,
    )
    .bind(team_id)
    .fetch_one(pool)
    .await?;
    Ok((row.0.unwrap_or(0), row.1.unwrap_or(0)))
}
