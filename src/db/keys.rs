use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct ApiKeyRow {
    pub id: Uuid,
    pub key_hash: String,
    pub key_prefix: String,
    pub user_id: Uuid,
    pub name: Option<String>,
    pub rate_limit: i32,
    pub budget_monthly_cents: Option<i64>,
    pub is_active: bool,
    pub last_used: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub replaced_by: Option<Uuid>,
}

pub fn generate_key() -> (String, String, String) {
    let raw = Uuid::new_v4().to_string().replace('-', "");
    let plaintext = format!("sk-{raw}");
    let prefix = plaintext[..11].to_string();
    let hash = hash_key(&plaintext);
    (plaintext, prefix, hash)
}

pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub async fn create_key(
    pool: &PgPool,
    user_id: Uuid,
    name: Option<&str>,
) -> Result<(String, ApiKeyRow), sqlx::Error> {
    create_key_full(pool, user_id, name, None, None).await
}

pub async fn create_key_full(
    pool: &PgPool,
    user_id: Uuid,
    name: Option<&str>,
    rate_limit: Option<i32>,
    budget_monthly_cents: Option<i64>,
) -> Result<(String, ApiKeyRow), sqlx::Error> {
    let (plaintext, prefix, hash) = generate_key();
    let rate_limit = rate_limit.unwrap_or(60);
    let row = sqlx::query_as::<_, ApiKeyRow>(
        r#"INSERT INTO api_keys (key_hash, key_prefix, user_id, name, rate_limit, budget_monthly_cents)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING *"#,
    )
    .bind(&hash)
    .bind(&prefix)
    .bind(user_id)
    .bind(name)
    .bind(rate_limit)
    .bind(budget_monthly_cents)
    .fetch_one(pool)
    .await?;
    Ok((plaintext, row))
}

pub async fn update_key(
    pool: &PgPool,
    key_id: Uuid,
    rate_limit: Option<i32>,
    budget_monthly_cents: Option<i64>,
    name: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE api_keys SET
             rate_limit = COALESCE($2, rate_limit),
             budget_monthly_cents = CASE WHEN $3::bigint IS NOT NULL THEN $3 ELSE budget_monthly_cents END,
             name = COALESCE($4, name)
           WHERE id = $1"#,
    )
    .bind(key_id)
    .bind(rate_limit)
    .bind(budget_monthly_cents)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn find_key_by_hash(pool: &PgPool, hash: &str) -> Result<Option<ApiKeyRow>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRow>("SELECT * FROM api_keys WHERE key_hash = $1")
        .bind(hash)
        .fetch_optional(pool)
        .await
}

pub async fn list_keys(
    pool: &PgPool,
    user_id: Option<Uuid>,
) -> Result<Vec<ApiKeyRow>, sqlx::Error> {
    match user_id {
        Some(uid) => {
            sqlx::query_as::<_, ApiKeyRow>(
                "SELECT * FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC",
            )
            .bind(uid)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, ApiKeyRow>("SELECT * FROM api_keys ORDER BY created_at DESC")
                .fetch_all(pool)
                .await
        }
    }
}

pub async fn revoke_key(pool: &PgPool, key_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE api_keys SET is_active = FALSE WHERE id = $1")
        .bind(key_id)
        .execute(pool)
        .await?;
    bump_cache_version(pool).await?;
    Ok(())
}

pub async fn update_last_used(pool: &PgPool, key_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE api_keys SET last_used = NOW() WHERE id = $1")
        .bind(key_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn bump_cache_version(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE cache_version SET version = version + 1 WHERE id = 1")
        .execute(pool)
        .await?;
    Ok(())
}

/// Rotate an API key: create a new key inheriting owner/limits, mark the old
/// one as `replaced_by` + `expires_at = NOW() + 24h`. Both remain valid for
/// the grace period so clients can swap.
pub async fn rotate_key(
    pool: &PgPool,
    old_key_id: Uuid,
) -> Result<(String, ApiKeyRow, DateTime<Utc>), sqlx::Error> {
    let old: ApiKeyRow = sqlx::query_as::<_, ApiKeyRow>("SELECT * FROM api_keys WHERE id = $1")
        .bind(old_key_id)
        .fetch_one(pool)
        .await?;

    let (plaintext, new_row) = create_key_full(
        pool,
        old.user_id,
        old.name.as_deref(),
        Some(old.rate_limit),
        old.budget_monthly_cents,
    )
    .await?;

    let expires_at = Utc::now() + chrono::Duration::hours(24);
    sqlx::query("UPDATE api_keys SET expires_at = $2, replaced_by = $3 WHERE id = $1")
        .bind(old_key_id)
        .bind(expires_at)
        .bind(new_row.id)
        .execute(pool)
        .await?;

    bump_cache_version(pool).await?;
    Ok((plaintext, new_row, expires_at))
}

pub async fn get_cache_version(pool: &PgPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT version FROM cache_version WHERE id = 1")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}
