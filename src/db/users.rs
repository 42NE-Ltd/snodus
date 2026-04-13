use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub team_id: Option<Uuid>,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

pub async fn create_user(
    pool: &PgPool,
    email: &str,
    name: &str,
    team_id: Option<Uuid>,
    role: &str,
) -> Result<UserRow, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"INSERT INTO users (email, name, team_id, role)
           VALUES ($1, $2, $3, $4)
           RETURNING *"#,
    )
    .bind(email)
    .bind(name)
    .bind(team_id)
    .bind(role)
    .fetch_one(pool)
    .await
}

pub async fn find_user_by_id(pool: &PgPool, id: Uuid) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, team_id, role, created_at FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_email(pool: &PgPool, email: &str) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, team_id, role, created_at FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}

pub async fn list_users(pool: &PgPool) -> Result<Vec<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, team_id, role, created_at FROM users ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

/// Soft-delete: marks the user inactive and revokes all their API keys.
pub async fn deactivate_user(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET is_active = FALSE WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE api_keys SET is_active = FALSE WHERE user_id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
