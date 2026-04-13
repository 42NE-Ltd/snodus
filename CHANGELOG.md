# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.4.0] — 2026-04-13

Initial public release.

### Added
- Multi-provider LLM proxy (Anthropic, OpenAI, Ollama) behind a single API
- Virtual API keys with SHA-256 hash, per-user rate limiting, monthly budgets
- Auth middleware (Bearer token / x-api-key / Basic auth for admin)
- Sliding window rate limiter with `X-RateLimit-*` response headers
- Budget enforcement returning 402 Payment Required with monthly reset
- Spend tracker with async mpsc buffer and batch flush to Postgres
- SSE streaming interceptor for real-time token extraction
- Smart router: `"model": "auto"` routes by input size and keyword rules
- Provider trait with `ProviderPool` resolution by model prefix
- OpenAI format translator (Anthropic to/from OpenAI request/response)
- Ollama provider with `/api/tags` model discovery
- Key rotation with 24h grace period
- Dashboard (light theme, horizontal tabs, spend overview, keys, users and teams)
- i18n system with 14 languages (en, zh, hi, es, ar, fr, bn, pt, ru, id, de, ja, ko, it), 279 keys per language, RTL support for Arabic
- Language switcher component in all pages
- Admin API for keys, users, teams, spend summary
- CLI: `serve`, `status`, `keys`, `users`, `teams`, `budget`, `providers`
- Free plan: 100K tokens/month with monthly reset
- 107 tests (40 unit + 67 integration) covering proxy, routing, rate limiting, budget, auth, SSE, and provider translation
- CI matrix: Rust stable/nightly and Postgres 14/15/16
- Security pipeline: `cargo audit` and `cargo deny`
- Stress test suite for health, admin, and proxy endpoints
- Doc example validation test suite (15 automated checks)
- Postgres migrations for core schema
- Docker image for self-hosting
- Documentation site with API reference, CLI guide, self-hosting instructions
- `deny.toml` for license and advisory compliance

[0.4.0]: https://github.com/42NE-Ltd/snodus/releases/tag/v0.4.0
