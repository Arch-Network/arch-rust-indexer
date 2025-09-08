-- Idempotent creation of accounts table for Atlas account processors (Issue #6)
CREATE TABLE IF NOT EXISTS accounts (
    pubkey TEXT PRIMARY KEY,
    lamports BIGINT NOT NULL,
    owner TEXT NOT NULL,
    data BYTEA NOT NULL,
    height BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_accounts_owner ON accounts(owner);
CREATE INDEX IF NOT EXISTS idx_accounts_height ON accounts(height);
