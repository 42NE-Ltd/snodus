use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct TeamRow {
    pub id: Uuid,
    pub name: String,
    pub budget_monthly_cents: Option<i64>,
    pub created_at: DateTime<Utc>,
}

pub async fn create_team(
    pool: &PgPool,
    name: &str,
    budget_monthly_cents: Option<i64>,
) -> Result<TeamRow, sqlx::Error> {
    sqlx::query_as::<_, TeamRow>(
        r#"INSERT INTO teams (name, budget_monthly_cents)
           VALUES ($1, $2)
           RETURNING *"#,
    )
    .bind(name)
    .bind(budget_monthly_cents)
    .fetch_one(pool)
    .await
}

pub async fn list_teams(pool: &PgPool) -> Result<Vec<TeamRow>, sqlx::Error> {
    sqlx::query_as::<_, TeamRow>(
        "SELECT id, name, budget_monthly_cents, created_at FROM teams ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn update_team(
    pool: &PgPool,
    team_id: Uuid,
    budget_monthly_cents: Option<i64>,
    webhook_url: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE teams SET
             budget_monthly_cents = $2,
             webhook_url = COALESCE($3, webhook_url)
           WHERE id = $1"#,
    )
    .bind(team_id)
    .bind(budget_monthly_cents)
    .bind(webhook_url)
    .execute(pool)
    .await?;
    Ok(())
}
