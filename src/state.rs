use crate::auth::KeyCache;
use crate::budget::BudgetChecker;
use crate::config::GatewayConfig;
use crate::providers::pool::ProviderPool;
use crate::providers::router::RuleRouter;
use crate::ratelimit::RateLimiter;
use crate::spend::SpendSender;
use dashmap::DashMap;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

pub type TeamBudgetCache = Arc<DashMap<Uuid, Option<i64>>>;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: GatewayConfig,
    pub key_cache: KeyCache,
    pub providers: ProviderPool,
    pub router: Arc<RuleRouter>,
    pub router_enabled: bool,
    pub spend_tx: SpendSender,
    pub rate_limiter: RateLimiter,
    pub budget: BudgetChecker,
    pub team_budgets: TeamBudgetCache,
}

pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}

/// Run only the open source migrations (`migrations/core/`). Cloud crates
/// run their own migrations on top.
pub async fn run_core_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    let sql = include_str!("../migrations/001_initial.sql");
    for statement in sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() && !trimmed.starts_with("--") {
            let _ = sqlx::query(trimmed).execute(pool).await;
        }
    }
    Ok(())
}

pub async fn load_team_budgets(pool: &PgPool, cache: &TeamBudgetCache) -> Result<(), sqlx::Error> {
    cache.clear();
    let teams = crate::db::teams::list_teams(pool).await?;
    for t in teams {
        cache.insert(t.id, t.budget_monthly_cents);
    }
    Ok(())
}
