pub mod counter;

use crate::budget::BudgetChecker;
use crate::state::TeamBudgetCache;
use sqlx::PgPool;
use std::collections::HashSet;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SpendEntry {
    pub api_key_id: Uuid,
    pub user_id: Uuid,
    pub team_id: Option<Uuid>,
    pub model: String,
    pub provider: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_cents: i64,
    pub duration_ms: Option<i32>,
    pub key_budget_cents: Option<i64>,
    pub routed_from: Option<String>,
    pub routing_method: Option<String>,
    pub routing_latency_ms: Option<i32>,
    pub region: Option<String>,
}

pub type SpendSender = mpsc::UnboundedSender<SpendEntry>;

pub fn start_spend_tracker(
    pool: PgPool,
    budget: BudgetChecker,
    team_budgets: TeamBudgetCache,
) -> SpendSender {
    let (tx, mut rx) = mpsc::unbounded_channel::<SpendEntry>();

    tokio::spawn(async move {
        let mut buffer: Vec<SpendEntry> = Vec::new();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                entry = rx.recv() => {
                    match entry {
                        Some(e) => {
                            buffer.push(e);
                            if buffer.len() >= 100 {
                                flush(&pool, &mut buffer, &budget, &team_budgets).await;
                            }
                        }
                        None => {
                            flush(&pool, &mut buffer, &budget, &team_budgets).await;
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        flush(&pool, &mut buffer, &budget, &team_budgets).await;
                    }
                }
            }
        }
    });

    tx
}

async fn flush(
    pool: &PgPool,
    buffer: &mut Vec<SpendEntry>,
    budget: &BudgetChecker,
    _team_budgets: &TeamBudgetCache,
) {
    if buffer.is_empty() {
        return;
    }

    let mut touched_keys: Vec<Uuid> = Vec::new();
    let mut touched_teams: HashSet<Uuid> = HashSet::new();

    let entries: Vec<_> = buffer
        .drain(..)
        .map(|e| {
            touched_keys.push(e.api_key_id);
            if let Some(team) = e.team_id {
                touched_teams.insert(team);
            }
            crate::db::spend::SpendInsert {
                api_key_id: e.api_key_id,
                user_id: e.user_id,
                team_id: e.team_id,
                model: e.model,
                provider: e.provider,
                input_tokens: e.input_tokens,
                output_tokens: e.output_tokens,
                cost_cents: e.cost_cents,
                duration_ms: e.duration_ms,
                routed_from: e.routed_from,
                routing_method: e.routing_method,
                routing_latency_ms: e.routing_latency_ms,
                region: e.region,
            }
        })
        .collect();

    if let Err(e) = crate::db::spend::insert_spend_batch(pool, &entries).await {
        tracing::error!("Failed to flush spend buffer: {e}");
        return;
    }
    tracing::debug!("Flushed {} spend entries", entries.len());

    // Invalidate cached aggregates so subsequent budget checks see fresh
    // numbers. Cloud crates plug their own webhook/notification logic at the
    // end of this flush via a wrapper task — core stays hookless.
    for key_id in touched_keys.iter() {
        budget.invalidate_key(*key_id);
    }
    for team_id in touched_teams.iter() {
        budget.invalidate_team(*team_id);
    }
}
