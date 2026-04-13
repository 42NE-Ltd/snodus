# Snodus

**LLM Gateway open source. Un endpoint, tutti i provider.**

Route, authenticate, and track AI API usage across Anthropic, OpenAI, and local models (Ollama).

![CI](https://github.com/42NE-Ltd/snodus/actions/workflows/ci.yml/badge.svg)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)
![Rust](https://img.shields.io/badge/rust-1.82+-orange)

## Features

- **Multi-provider** — Anthropic, OpenAI, Ollama behind a single API
- **Smart routing** — `"model": "auto"` picks the right model (rule-based + LLM classifier)
- **Virtual API keys** — per user/team, with budget, rate limit, rotation, and expiry
- **Dashboard** — real-time spend, model breakdown, top users, CSV export
- **Budget enforcement** — 402 Payment Required when a team exceeds its monthly budget
- **Webhook notifications** — threshold alerts at 70%, 90%, 100% with 1h dedup
- **Full CLI** — teams, users, keys, plans, budget, providers, status
- **Docker deploy** — `docker compose up` in 30 seconds
- **Rust** — stateless, async, Axum + Tokio + sqlx

## Quick Start

```bash
git clone https://github.com/42NE-Ltd/snodus
cd snodus
cp .env.example .env
# Edita .env con la tua ANTHROPIC_API_KEY e ADMIN_PASSWORD
docker compose up -d

# Test
curl http://localhost:8080/health
```

Poi crea il primo team, utente e virtual key:

```bash
snodus teams create --name "Azienda" --budget 5000
snodus users create --email admin@azienda.it --name Admin --team-id <UUID> --role admin
snodus keys create --user-id <UUID> --name dev
```

Usa la key ricevuta con Claude Code:

```bash
export ANTHROPIC_BASE_URL=http://localhost:8080
export ANTHROPIC_AUTH_TOKEN=sk-xxx
claude
```

## Architecture

```
Client
  │ Authorization: Bearer sk-xxx
  ▼
Auth → RateLimit → Budget → SmartRouter → ProviderPool
                                              │
                    ┌─────────────────────────┼─────────────────────────┐
                    ▼                         ▼                         ▼
              Anthropic                   OpenAI                    Ollama
              (claude-*)                  (gpt-*, o1*)             (local)
                    │                         │                         │
                    └─────────────────────────┼─────────────────────────┘
                                              ▼
                                     Spend Tracker (mpsc buffer)
                                              │
                                              ▼
                                       Postgres (spend_log)
                                              │
                                              ▼
                                    Webhook notify (budget alerts)
```

## Provider support

| Provider | Models | Endpoint | Cost |
|----------|--------|----------|------|
| Anthropic | `claude-*` | `/v1/messages` | Real pricing |
| OpenAI | `gpt-*`, `o1-*`, `o3-*`, `o4-*` | `/v1/messages` (tradotto) or `/v1/chat/completions` | Real pricing |
| Ollama | discovered + `qwen*`, `llama*`, `phi*`, `mistral*`, … | `/v1/messages` (tradotto) | Free (local) |

## Links

| | |
|---|---|
| **snodus.org** | Open source project — download, install, contribute |
| **snodus.ai** | Managed cloud service — signup, pricing, dashboard |
| **snodus.dev** | Developer docs — API reference, CLI, configuration |
| **GitHub** | [42NE-Ltd/snodus](https://github.com/42NE-Ltd/snodus) |

## Self-hosting

See the [self-hosting guide](https://snodus.dev#self-host).

Required env vars:

```
DATABASE_URL=postgresql://snodus:snodus@localhost:5432/snodus
ANTHROPIC_API_KEY=sk-ant-...
ADMIN_PASSWORD=changeme
```

Optional:

```
OPENAI_API_KEY=sk-...              # enable OpenAI provider
OLLAMA_BASE_URL=http://localhost:11434
SNODUS_ROUTER_ENABLED=true         # enable "model": "auto"
SNODUS_WEBHOOK_URL=https://...     # budget alerts
SNODUS_FEATURES_SIGNUP=true        # enable /signup (hosted only)
SNODUS_FEATURES_PLANS=true         # plan enforcement
SNODUS_FEATURES_LANDING=true       # landing page on /
```

## Development

```bash
cargo build
cargo test
cargo fmt
cargo clippy -- -D warnings

# Start local postgres + gateway
docker compose up -d postgres
cargo run -- serve
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full workflow.

## Status

- ✅ **Phase 1** (MVP): proxy, auth, spend tracking, CLI
- ✅ **Phase 2** (Control): rate limiting, budget, notifications, dashboard SPA, SSE spend
- ✅ **Phase 3** (Multi-provider): OpenAI, Ollama, smart routing
- ✅ **Phase 4** (Product): signup, plans, landing page, docs, open source
- ⏳ Phase 4b (post-traction): Ekos.chat integration

See [CHANGELOG.md](CHANGELOG.md) for release history.

## License

MIT — 42NE Ltd
