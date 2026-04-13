# Contributing to Snodus

Thanks for helping! Snodus is a small, focused project — we care about keeping
the core simple, the tests honest, and the surface area tight.

## Getting started

```bash
git clone https://github.com/42NE-Ltd/snodus
cd snodus
cp .env.example .env
# Edit .env: DATABASE_URL, ANTHROPIC_API_KEY, ADMIN_PASSWORD
docker compose up -d postgres
cargo build
cargo test
```

To run the gateway locally:

```bash
cargo run -- serve
```

## Workflow

1. **Fork** the repo and create a feature branch off `main`.
2. Write your change with tests (`#[cfg(test)]` inline for unit tests, `tests/`
   for integration tests).
3. Run the full quality suite before pushing:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   cargo test
   ```
4. Commit with [Conventional Commits](https://www.conventionalcommits.org):
   `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`.
5. Open a PR against `main`. Link any related issue. Fill the PR template.

## What we look for

- **Small diffs.** One logical change per PR. Split aggressively.
- **Tests.** If you add behavior, add a test. If you fix a bug, add a test that
  would've caught it.
- **No unrelated cleanup.** Keep cleanup PRs separate from feature PRs.
- **Docstrings for WHY, not WHAT.** Comment the non-obvious decisions. Don't
  restate what the code already says.
- **Provider-neutral core.** The core proxy, auth, rate limit, budget should
  not know about specific provider APIs — only the `providers/*` modules do.

## What we do NOT want

- Backwards-compat shims for code that was never released.
- Feature flags for hypothetical futures. Add the flag when we need it.
- Dependencies that duplicate what `std` or an existing dep already gives us.
- Commits that bypass hooks (`--no-verify`) or signing.

## Code style

- `rustfmt` default config. No custom overrides.
- Errors: `anyhow::Result` inside the CLI / init path; typed errors + `IntoResponse`
  inside HTTP handlers.
- Async: everything is Tokio. No blocking calls inside request handlers.
- SQL: parametrized queries via `sqlx::query_as`. Never interpolate strings.
- Logging: `tracing::info!` for state changes, `tracing::debug!` for per-request
  details, `tracing::error!` when we bail.

## Architecture in a sentence

Axum router → middleware pipeline (Auth → RateLimit → Budget) → Smart Router (if
`model: "auto"`) → ProviderPool → concrete provider (Anthropic/OpenAI/Ollama) →
response body → SSE interceptor or buffered JSON → Spend Tracker (mpsc → flush
batch to Postgres) → Notify dispatcher (webhook + notification_log).

Read the phase plans in `docs/snodus-fase{1,2,3,4}-*.md` for full context.

## Reporting issues

Use the bug template in `.github/ISSUE_TEMPLATE/`. Include:
- Snodus version (`snodus --version`)
- Rust version (`rustc --version`)
- OS + platform
- Exact steps to reproduce
- Actual vs expected behavior
- Logs from `RUST_LOG=snodus=debug`

## License

By contributing you agree your changes are licensed under MIT.
