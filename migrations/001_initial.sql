-- Snodus Gateway — Initial Schema

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- Teams
CREATE TABLE teams (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                 TEXT NOT NULL UNIQUE,
    budget_monthly_cents BIGINT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Users
CREATE TABLE users (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email      TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    team_id    UUID REFERENCES teams(id) ON DELETE SET NULL,
    role       TEXT NOT NULL DEFAULT 'member',
    is_active  BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Virtual API keys
CREATE TABLE api_keys (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key_hash             TEXT NOT NULL UNIQUE,
    key_prefix           TEXT NOT NULL,
    user_id              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name                 TEXT,
    rate_limit           INT NOT NULL DEFAULT 60,
    budget_monthly_cents BIGINT,
    is_active            BOOLEAN NOT NULL DEFAULT TRUE,
    last_used            TIMESTAMPTZ,
    expires_at           TIMESTAMPTZ,
    replaced_by          UUID,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Spend log
CREATE TABLE spend_log (
    id                 BIGSERIAL PRIMARY KEY,
    api_key_id         UUID NOT NULL REFERENCES api_keys(id),
    user_id            UUID NOT NULL REFERENCES users(id),
    team_id            UUID REFERENCES teams(id),
    model              TEXT NOT NULL,
    provider           TEXT NOT NULL DEFAULT 'anthropic',
    input_tokens       INT NOT NULL,
    output_tokens      INT NOT NULL,
    cost_cents         BIGINT NOT NULL,
    duration_ms        INT,
    routed_from        TEXT,
    routing_method     TEXT,
    routing_latency_ms INT,
    region             TEXT,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_spend_log_created ON spend_log(created_at);
CREATE INDEX idx_spend_log_user    ON spend_log(user_id, created_at);
CREATE INDEX idx_spend_log_team    ON spend_log(team_id, created_at);
CREATE INDEX idx_spend_log_model   ON spend_log(model, created_at);
CREATE INDEX idx_spend_log_key     ON spend_log(api_key_id, created_at);

-- Cache invalidation
CREATE TABLE cache_version (
    id      INT PRIMARY KEY DEFAULT 1,
    version BIGINT NOT NULL DEFAULT 0
);
INSERT INTO cache_version VALUES (1, 0);
