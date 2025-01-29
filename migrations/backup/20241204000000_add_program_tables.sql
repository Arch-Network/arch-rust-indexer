-- Add program tracking tables
CREATE TABLE IF NOT EXISTS programs (
    program_id TEXT PRIMARY KEY,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    transaction_count BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS transaction_programs (
    txid TEXT REFERENCES transactions(txid) ON DELETE CASCADE,
    program_id TEXT REFERENCES programs(program_id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (txid, program_id)
);

-- Add indexes and trigger as in your original migration
