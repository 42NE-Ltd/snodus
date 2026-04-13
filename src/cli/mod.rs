use clap::{Parser, Subcommand};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "snodus", version, about = "Snodus LLM Gateway")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage API keys
    Keys {
        #[command(subcommand)]
        action: KeysAction,
    },
    /// Manage users
    Users {
        #[command(subcommand)]
        action: UsersAction,
    },
    /// Manage teams
    Teams {
        #[command(subcommand)]
        action: TeamsAction,
    },
    /// Show gateway status
    Status,
    /// Show budget vs spend for keys and teams
    Budget,
    /// Show configured providers
    Providers,
    /// Start the gateway server (default)
    Serve,
}

#[derive(Subcommand)]
pub enum KeysAction {
    /// Create a new API key
    Create {
        #[arg(long)]
        user_id: Uuid,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        rate_limit: Option<i32>,
        /// Monthly budget in cents (e.g. 5000 = $50)
        #[arg(long)]
        budget: Option<i64>,
    },
    /// List all API keys
    List {
        #[arg(long)]
        user_id: Option<Uuid>,
    },
    /// Revoke an API key
    Revoke {
        #[arg(long)]
        id: Uuid,
    },
    /// Update rate limit or budget on an existing key
    Update {
        #[arg(long)]
        id: Uuid,
        #[arg(long)]
        rate_limit: Option<i32>,
        #[arg(long)]
        budget: Option<i64>,
    },
    /// Rotate a key: generate new, old gets 24h grace period
    Rotate {
        #[arg(long)]
        id: Uuid,
    },
}

#[derive(Subcommand)]
pub enum UsersAction {
    /// Create a new user
    Create {
        #[arg(long)]
        email: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        team_id: Option<Uuid>,
        #[arg(long, default_value = "member")]
        role: String,
    },
    /// List all users
    List,
}

#[derive(Subcommand)]
pub enum TeamsAction {
    /// Create a new team
    Create {
        #[arg(long)]
        name: String,
        /// Monthly budget in cents
        #[arg(long)]
        budget: Option<i64>,
    },
    /// List all teams
    List,
    /// Update budget on a team
    Update {
        #[arg(long)]
        id: Uuid,
        #[arg(long)]
        budget: Option<i64>,
    },
}

pub async fn run_cli(cmd: Commands, pool: &PgPool) -> anyhow::Result<()> {
    match cmd {
        Commands::Keys { action } => match action {
            KeysAction::Create {
                user_id,
                name,
                rate_limit,
                budget,
            } => {
                let (plaintext, row) = crate::db::keys::create_key_full(
                    pool,
                    user_id,
                    name.as_deref(),
                    rate_limit,
                    budget,
                )
                .await?;
                let _ = crate::db::keys::bump_cache_version(pool).await;
                println!("\n✅ Key created successfully!\n");
                println!("  Key:        {plaintext}");
                println!("  ID:         {}", row.id);
                println!("  Prefix:     {}", row.key_prefix);
                println!("  Rate limit: {}/min", row.rate_limit);
                if let Some(b) = row.budget_monthly_cents {
                    println!("  Budget:     ${:.2}/month", b as f64 / 100.0);
                } else {
                    println!("  Budget:     unlimited");
                }
                println!("\n  ⚠️  Store this key securely. It will NOT be shown again.\n");
            }
            KeysAction::List { user_id } => {
                let keys = crate::db::keys::list_keys(pool, user_id).await?;
                if keys.is_empty() {
                    println!("No keys found.");
                    return Ok(());
                }
                println!(
                    "{:<38} {:<12} {:<18} {:<10} {:<12} {:<8}",
                    "ID", "PREFIX", "NAME", "RATE/min", "BUDGET", "ACTIVE"
                );
                println!("{}", "-".repeat(110));
                for k in keys {
                    let budget = k
                        .budget_monthly_cents
                        .map(|c| format!("${:.2}", c as f64 / 100.0))
                        .unwrap_or_else(|| "unlimited".into());
                    println!(
                        "{:<38} {:<12} {:<18} {:<10} {:<12} {:<8}",
                        k.id,
                        k.key_prefix,
                        k.name.unwrap_or_else(|| "-".into()),
                        k.rate_limit,
                        budget,
                        if k.is_active { "yes" } else { "no" },
                    );
                }
            }
            KeysAction::Revoke { id } => {
                crate::db::keys::revoke_key(pool, id).await?;
                println!("✅ Key {id} revoked.");
            }
            KeysAction::Update {
                id,
                rate_limit,
                budget,
            } => {
                crate::db::keys::update_key(pool, id, rate_limit, budget, None).await?;
                let _ = crate::db::keys::bump_cache_version(pool).await;
                println!("✅ Key {id} updated.");
            }
            KeysAction::Rotate { id } => {
                let (plaintext, new_row, old_expires) =
                    crate::db::keys::rotate_key(pool, id).await?;
                println!("\n✅ Key rotated\n");
                println!("  New key:    {plaintext}");
                println!("  New id:     {}", new_row.id);
                println!("  Old key expires at: {}", old_expires.to_rfc3339());
                println!("  Grace period: 24 hours\n");
            }
        },
        Commands::Users { action } => match action {
            UsersAction::Create {
                email,
                name,
                team_id,
                role,
            } => {
                let user =
                    crate::db::users::create_user(pool, &email, &name, team_id, &role).await?;
                println!("✅ User created: {} ({})", user.name, user.id);
            }
            UsersAction::List => {
                let users = crate::db::users::list_users(pool).await?;
                if users.is_empty() {
                    println!("No users found.");
                    return Ok(());
                }
                println!("{:<38} {:<25} {:<20} {:<8}", "ID", "EMAIL", "NAME", "ROLE");
                println!("{}", "-".repeat(95));
                for u in users {
                    println!("{:<38} {:<25} {:<20} {:<8}", u.id, u.email, u.name, u.role);
                }
            }
        },
        Commands::Teams { action } => match action {
            TeamsAction::Create { name, budget } => {
                let team = crate::db::teams::create_team(pool, &name, budget).await?;
                println!("✅ Team created: {} ({})", team.name, team.id);
            }
            TeamsAction::List => {
                let teams = crate::db::teams::list_teams(pool).await?;
                if teams.is_empty() {
                    println!("No teams found.");
                    return Ok(());
                }
                println!("{:<38} {:<20} {:<15}", "ID", "NAME", "BUDGET (USD)");
                println!("{}", "-".repeat(75));
                for t in teams {
                    let budget = t
                        .budget_monthly_cents
                        .map(|c| format!("${:.2}", c as f64 / 100.0))
                        .unwrap_or_else(|| "unlimited".into());
                    println!("{:<38} {:<20} {:<15}", t.id, t.name, budget);
                }
            }
            TeamsAction::Update { id, budget } => {
                crate::db::teams::update_team(pool, id, budget, None).await?;
                println!("✅ Team {id} updated.");
            }
        },
        Commands::Status => {
            let (total_cents, total_requests) = crate::db::spend::get_spend_summary(pool).await?;
            let keys = crate::db::keys::list_keys(pool, None).await?;
            let users = crate::db::users::list_users(pool).await?;
            let teams = crate::db::teams::list_teams(pool).await?;

            println!("\n📊 Snodus Gateway Status\n");
            println!("  Users:    {}", users.len());
            println!("  Teams:    {}", teams.len());
            println!(
                "  Keys:     {} active / {} total",
                keys.iter().filter(|k| k.is_active).count(),
                keys.len()
            );
            println!("\n  This month:");
            println!("    Requests: {total_requests}");
            println!("    Cost:     ${:.2}\n", total_cents as f64 / 100.0);
        }
        Commands::Budget => {
            let teams = crate::db::teams::list_teams(pool).await?;
            let keys = crate::db::keys::list_keys(pool, None).await?;
            let users = crate::db::users::list_users(pool).await?;
            let user_map: std::collections::HashMap<_, _> =
                users.iter().map(|u| (u.id, u.email.clone())).collect();

            println!("\n💰 Budget vs Spend (current month)\n");
            println!(
                "  {:<20} {:<14} {:<14} {:<14} {:>5}",
                "TEAM", "BUDGET", "SPENT", "REMAINING", "%"
            );
            println!("  {}", "-".repeat(75));
            for t in &teams {
                let spent = crate::db::spend::get_team_spend_current_month(pool, t.id).await?;
                let (budget_s, remain_s, pct) = format_budget(t.budget_monthly_cents, spent);
                println!(
                    "  {:<20} {:<14} {:<14} {:<14} {:>5}",
                    truncate(&t.name, 20),
                    budget_s,
                    format!("${:.2}", spent as f64 / 100.0),
                    remain_s,
                    pct
                );
            }
            println!(
                "\n  {:<14} {:<25} {:<14} {:<14} {:>5}",
                "KEY", "USER", "BUDGET", "SPENT", "%"
            );
            println!("  {}", "-".repeat(80));
            for k in &keys {
                if !k.is_active {
                    continue;
                }
                let spent = crate::db::spend::get_key_spend_current_month(pool, k.id).await?;
                let (budget_s, _, pct) = format_budget(k.budget_monthly_cents, spent);
                let warn = if k
                    .budget_monthly_cents
                    .map(|b| spent as f64 / b.max(1) as f64 >= 0.9)
                    .unwrap_or(false)
                {
                    " ⚠"
                } else {
                    ""
                };
                println!(
                    "  {:<14} {:<25} {:<14} {:<14} {:>5}{}",
                    k.key_prefix,
                    truncate(
                        user_map.get(&k.user_id).map(|s| s.as_str()).unwrap_or("-"),
                        25
                    ),
                    budget_s,
                    format!("${:.2}", spent as f64 / 100.0),
                    pct,
                    warn
                );
            }
            println!();
        }
        Commands::Providers => {
            let cfg = crate::config::GatewayConfig::from_env().map_err(|e| anyhow::anyhow!(e))?;
            println!("\n🔌 Configured providers:\n");
            println!("  {:<12} {:<10} {:<30}", "NAME", "STATUS", "HANDLES");
            println!("  {}", "-".repeat(60));
            let anth_status = if cfg.anthropic_api_key.is_empty() {
                "disabled"
            } else {
                "enabled"
            };
            println!(
                "  {:<12} {:<10} {:<30}",
                "anthropic", anth_status, "claude-*"
            );
            let openai_status = if cfg.openai_api_key.is_some() {
                "enabled"
            } else {
                "disabled"
            };
            println!(
                "  {:<12} {:<10} {:<30}",
                "openai", openai_status, "gpt-*, o1*, o3*, o4*"
            );
            let ollama_status = if cfg.ollama_base_url.is_some() {
                "enabled"
            } else {
                "disabled"
            };
            println!(
                "  {:<12} {:<10} {:<30}",
                "ollama", ollama_status, "qwen*, llama*, phi*, mistral*"
            );
            println!(
                "\n  Router: {}",
                if cfg.router_enabled {
                    "enabled (model: auto)"
                } else {
                    "disabled"
                }
            );
            println!();
        }
        Commands::Serve => unreachable!(),
    }
    Ok(())
}

#[allow(dead_code)]
fn print_bar(label: &str, current: i64, limit: i64, suffix: &str) {
    if limit <= 0 {
        println!("  {label}:   {current}{suffix}");
        return;
    }
    let pct = (current as f64 / limit as f64 * 100.0).min(100.0) as usize;
    let filled = pct / 5;
    let empty = 20 - filled;
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
    println!("  {label}:    {current} / {limit} ({pct}%){suffix}");
    println!("  {bar}");
}

fn format_budget(budget_cents: Option<i64>, spent_cents: i64) -> (String, String, String) {
    match budget_cents {
        Some(b) => {
            let remaining = b - spent_cents;
            let pct = if b > 0 {
                format!(
                    "{}%",
                    (spent_cents as f64 / b as f64 * 100.0).round() as i64
                )
            } else {
                "-".into()
            };
            (
                format!("${:.2}", b as f64 / 100.0),
                format!("${:.2}", remaining as f64 / 100.0),
                pct,
            )
        }
        None => ("unlimited".into(), "-".into(), "-".into()),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", &s.chars().take(max - 1).collect::<String>())
    } else {
        s.to_string()
    }
}
