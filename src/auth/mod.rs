pub mod middleware;

use crate::db::keys;
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub key_id: Uuid,
    pub user_id: Uuid,
    pub team_id: Option<Uuid>,
    pub role: String,
    pub rate_limit: i32,
    pub budget_monthly_cents: Option<i64>,
    pub is_active: bool,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub type KeyCache = Arc<DashMap<String, KeyInfo>>;

pub fn new_cache() -> KeyCache {
    Arc::new(DashMap::new())
}

pub async fn load_keys_into_cache(pool: &PgPool, cache: &KeyCache) -> Result<(), sqlx::Error> {
    cache.clear();
    let all_keys = keys::list_keys(pool, None).await?;
    for k in all_keys {
        // We need to join with users to get team_id and role
        let user = crate::db::users::find_user_by_id(pool, k.user_id).await?;
        if let Some(u) = user {
            cache.insert(
                k.key_hash.clone(),
                KeyInfo {
                    key_id: k.id,
                    user_id: k.user_id,
                    team_id: u.team_id,
                    role: u.role,
                    rate_limit: k.rate_limit,
                    budget_monthly_cents: k.budget_monthly_cents,
                    is_active: k.is_active,
                    expires_at: k.expires_at,
                },
            );
        }
    }
    tracing::info!("Loaded {} keys into cache", cache.len());
    Ok(())
}

/// Background task: poll cache_version and reload when changed
pub async fn cache_refresh_loop(pool: PgPool, cache: KeyCache) {
    let mut last_version: i64 = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        match keys::get_cache_version(&pool).await {
            Ok(v) if v != last_version => {
                tracing::info!("Cache version changed {last_version} -> {v}, reloading");
                if let Err(e) = load_keys_into_cache(&pool, &cache).await {
                    tracing::error!("Failed to reload key cache: {e}");
                }
                last_version = v;
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("Failed to check cache version: {e}"),
        }
    }
}
