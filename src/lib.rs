//! `snodus-core` — open source LLM gateway primitives (proxy, auth, rate
//! limit, budget, spend tracking). Re-exported as a library so the proprietary
//! `snodus-cloud` crate can wrap it with premium features.

pub mod api;
pub mod auth;
pub mod budget;
pub mod cli;
pub mod config;
pub mod db;
pub mod providers;
pub mod ratelimit;
pub mod spend;
pub mod state;

use axum::Router;
use sqlx::PgPool;
use std::sync::Arc;

use crate::config::GatewayConfig;
use crate::providers::anthropic::AnthropicProvider;
use crate::providers::ollama::OllamaProvider;
use crate::providers::openai::OpenAiProvider;
use crate::providers::pool::ProviderPool;
use crate::providers::router::RuleRouter;
use crate::state::AppState;

/// Builds the core `AppState` (key cache, providers, rate limiter, budget,
/// spend tracker). Cloud crates can wrap this state inside their own and pass
/// `state.clone()` to `build_router` to get the open source routes.
pub async fn build_state(config: GatewayConfig, pool: PgPool) -> anyhow::Result<AppState> {
    let key_cache = auth::new_cache();
    auth::load_keys_into_cache(&pool, &key_cache).await?;
    tokio::spawn(auth::cache_refresh_loop(pool.clone(), key_cache.clone()));

    let mut pool_providers = ProviderPool::new();
    if !config.anthropic_api_key.is_empty() {
        pool_providers.register(Arc::new(AnthropicProvider::new(
            config.anthropic_api_key.clone(),
        )));
        tracing::info!("Registered provider: anthropic");
    } else {
        tracing::warn!("ANTHROPIC_API_KEY missing — anthropic provider disabled");
    }
    if let Some(key) = &config.openai_api_key {
        let base =
            std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com".into());
        pool_providers.register(Arc::new(OpenAiProvider::with_base(
            key.clone(),
            base.clone(),
            120,
        )));
        tracing::info!("Registered provider: openai (base={base})");
    }
    if let Some(base) = &config.ollama_base_url {
        let ollama = Arc::new(OllamaProvider::new(base.clone()));
        match ollama.discover().await {
            Ok(models) => tracing::info!("Ollama discovered {} models", models.len()),
            Err(e) => tracing::warn!("Ollama discovery failed: {e} (provider still registered)"),
        }
        pool_providers.register(ollama);
        tracing::info!("Registered provider: ollama");
    }

    let rate_limiter = ratelimit::RateLimiter::new(60);
    ratelimit::start_cleanup_task(rate_limiter.clone());

    let budget_checker = budget::BudgetChecker::new(pool.clone());

    let team_budgets: state::TeamBudgetCache = std::sync::Arc::new(dashmap::DashMap::new());
    state::load_team_budgets(&pool, &team_budgets).await?;

    let spend_tx =
        spend::start_spend_tracker(pool.clone(), budget_checker.clone(), team_budgets.clone());

    Ok(AppState {
        pool,
        config,
        key_cache,
        providers: pool_providers,
        router: Arc::new(RuleRouter::default()),
        router_enabled: false, // core ships rules-only router; cloud may flip this on
        spend_tx,
        rate_limiter,
        budget: budget_checker,
        team_budgets,
    })
}

/// The open source router: health, proxy, admin base (CRUD keys/users/teams,
/// /admin/spend, dashboard base HTML). All cloud-only routes (auth/login,
/// /scim/v2/, /admin/audit, /signup, /billing/) live in `snodus-cloud`.
pub fn build_router(state: AppState) -> Router {
    api::router(state)
}
