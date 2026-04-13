pub mod middleware;

use chrono::{DateTime, Datelike, TimeZone, Utc};
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

const CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
struct CachedSpend {
    total_cents: i64,
    cached_at: Instant,
    month: u32,
    year: i32,
}

#[derive(Debug, Clone)]
pub enum BudgetResult {
    Allowed {
        remaining_cents: i64,
        spent_cents: i64,
        limit_cents: i64,
    },
    Unlimited,
    Denied {
        limit_cents: i64,
        spent_cents: i64,
    },
}

#[derive(Clone)]
pub struct BudgetChecker {
    pool: PgPool,
    key_cache: Arc<DashMap<Uuid, CachedSpend>>,
    team_cache: Arc<DashMap<Uuid, CachedSpend>>,
}

impl BudgetChecker {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            key_cache: Arc::new(DashMap::new()),
            team_cache: Arc::new(DashMap::new()),
        }
    }

    pub async fn check_key_budget(&self, key_id: Uuid, limit_cents: Option<i64>) -> BudgetResult {
        // A budget of None or <= 0 means "no monetary limit" (the token_limit
        // on the plan handles capacity). Skip the DB roundtrip in that case.
        let Some(limit) = limit_cents else {
            return BudgetResult::Unlimited;
        };
        if limit <= 0 {
            return BudgetResult::Unlimited;
        }
        let spent = self.spend_for_key(key_id).await;
        Self::judge(limit, spent)
    }

    pub async fn check_team_budget(&self, team_id: Uuid, limit_cents: Option<i64>) -> BudgetResult {
        let Some(limit) = limit_cents else {
            return BudgetResult::Unlimited;
        };
        if limit <= 0 {
            return BudgetResult::Unlimited;
        }
        let spent = self.spend_for_team(team_id).await;
        Self::judge(limit, spent)
    }

    /// Force a refresh of the cached spend for a key/team (used after spend flush).
    pub fn invalidate_key(&self, key_id: Uuid) {
        self.key_cache.remove(&key_id);
    }

    pub fn invalidate_team(&self, team_id: Uuid) {
        self.team_cache.remove(&team_id);
    }

    async fn spend_for_key(&self, key_id: Uuid) -> i64 {
        let now = Instant::now();
        let (year, month) = current_year_month();

        if let Some(entry) = self.key_cache.get(&key_id) {
            if now.duration_since(entry.cached_at) < CACHE_TTL
                && entry.month == month
                && entry.year == year
            {
                return entry.total_cents;
            }
        }

        let total = crate::db::spend::get_key_spend_current_month(&self.pool, key_id)
            .await
            .unwrap_or(0);
        self.key_cache.insert(
            key_id,
            CachedSpend {
                total_cents: total,
                cached_at: now,
                month,
                year,
            },
        );
        total
    }

    async fn spend_for_team(&self, team_id: Uuid) -> i64 {
        let now = Instant::now();
        let (year, month) = current_year_month();

        if let Some(entry) = self.team_cache.get(&team_id) {
            if now.duration_since(entry.cached_at) < CACHE_TTL
                && entry.month == month
                && entry.year == year
            {
                return entry.total_cents;
            }
        }

        let total = crate::db::spend::get_team_spend_current_month(&self.pool, team_id)
            .await
            .unwrap_or(0);
        self.team_cache.insert(
            team_id,
            CachedSpend {
                total_cents: total,
                cached_at: now,
                month,
                year,
            },
        );
        total
    }

    pub fn judge(limit: i64, spent: i64) -> BudgetResult {
        // Defense in depth: a zero or negative limit is always treated as
        // "no monetary budget" so future callers that bypass the public
        // check_* helpers can't accidentally block every request.
        if limit <= 0 {
            return BudgetResult::Unlimited;
        }
        if spent >= limit {
            BudgetResult::Denied {
                limit_cents: limit,
                spent_cents: spent,
            }
        } else {
            BudgetResult::Allowed {
                remaining_cents: limit - spent,
                spent_cents: spent,
                limit_cents: limit,
            }
        }
    }
}

pub fn current_year_month() -> (i32, u32) {
    let now = Utc::now();
    (now.year(), now.month())
}

/// First instant of the next month — used for `resets_at` in the 402 body.
pub fn next_month_reset() -> DateTime<Utc> {
    let now = Utc::now();
    let (year, month) = (now.year(), now.month());
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    Utc.with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judge_allowed_when_under_limit() {
        match BudgetChecker::judge(1000, 200) {
            BudgetResult::Allowed {
                remaining_cents,
                spent_cents,
                limit_cents,
            } => {
                assert_eq!(remaining_cents, 800);
                assert_eq!(spent_cents, 200);
                assert_eq!(limit_cents, 1000);
            }
            _ => panic!("expected allowed"),
        }
    }

    #[test]
    fn judge_denied_when_at_or_over_limit() {
        assert!(matches!(
            BudgetChecker::judge(1000, 1000),
            BudgetResult::Denied { .. }
        ));
        assert!(matches!(
            BudgetChecker::judge(1000, 1500),
            BudgetResult::Denied { .. }
        ));
    }

    #[test]
    fn zero_limit_means_unlimited_not_denied() {
        // Regression: free plan ships with budget_monthly_cents = 0, which
        // should mean "no monetary cap, delegate to token_limit" — not
        // "every request is over budget".
        assert!(matches!(
            BudgetChecker::judge(0, 0),
            BudgetResult::Unlimited
        ));
        assert!(matches!(
            BudgetChecker::judge(0, 1_000_000),
            BudgetResult::Unlimited
        ));
    }

    #[test]
    fn negative_limit_is_unlimited() {
        // Sanity: a stale / migration-glitched negative value must not brick the service.
        assert!(matches!(
            BudgetChecker::judge(-1, 500),
            BudgetResult::Unlimited
        ));
    }

    #[test]
    fn next_month_reset_is_future() {
        assert!(next_month_reset() > Utc::now());
    }
}
