CREATE TABLE IF NOT EXISTS blocks (
    height bigint PRIMARY KEY,
    hash text NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    bitcoin_block_height bigint
);

CREATE TABLE IF NOT EXISTS transactions (
    txid text PRIMARY KEY,
    block_height bigint NOT NULL REFERENCES blocks(height),
    data jsonb NOT NULL,
    status jsonb NOT NULL, -- Change this line
    bitcoin_txids text[],
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_transactions_block_height ON transactions(block_height);
CREATE INDEX IF NOT EXISTS idx_blocks_timestamp ON blocks(timestamp);

-- Enhance transactions table for additional metadata used by API
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS logs jsonb DEFAULT '[]'::jsonb;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS rollback_status jsonb DEFAULT '"NotRolledback"'::jsonb;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS accounts_tags jsonb DEFAULT '[]'::jsonb;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS compute_units_consumed integer;
CREATE INDEX IF NOT EXISTS idx_transactions_compute_units ON transactions (compute_units_consumed);

-- Programs and transaction_programs for program analytics
CREATE TABLE IF NOT EXISTS programs (
    program_id text PRIMARY KEY,
    transaction_count bigint DEFAULT 0,
    first_seen_at timestamptz DEFAULT CURRENT_TIMESTAMP,
    last_seen_at timestamptz DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS transaction_programs (
    txid text NOT NULL,
    program_id text NOT NULL,
    PRIMARY KEY (txid, program_id),
    FOREIGN KEY (txid) REFERENCES transactions(txid) ON DELETE CASCADE,
    FOREIGN KEY (program_id) REFERENCES programs(program_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_transaction_programs_txid ON transaction_programs (txid);
CREATE INDEX IF NOT EXISTS idx_transaction_programs_program_id ON transaction_programs (program_id);

-- Mempool tracking
CREATE TABLE IF NOT EXISTS mempool_transactions (
    txid text PRIMARY KEY,
    data jsonb NOT NULL,
    added_at timestamptz DEFAULT CURRENT_TIMESTAMP,
    fee_priority integer,
    size_bytes integer,
    status text DEFAULT 'pending'
);

CREATE INDEX IF NOT EXISTS idx_mempool_added_at ON mempool_transactions (added_at);
CREATE INDEX IF NOT EXISTS idx_mempool_fee_priority ON mempool_transactions (fee_priority);
CREATE INDEX IF NOT EXISTS idx_mempool_status ON mempool_transactions (status);
CREATE INDEX IF NOT EXISTS idx_mempool_size ON mempool_transactions (size_bytes);

-- Mempool stats view used by API
CREATE OR REPLACE VIEW mempool_stats AS
SELECT 
    COUNT(*) as total_transactions,
    COUNT(*) FILTER (WHERE status = 'pending') as pending_count,
    COUNT(*) FILTER (WHERE status = 'confirmed') as confirmed_count,
    AVG(fee_priority) as avg_fee_priority,
    AVG(size_bytes) as avg_size_bytes,
    SUM(size_bytes) as total_size_bytes,
    MIN(added_at) as oldest_transaction,
    MAX(added_at) as newest_transaction
FROM mempool_transactions;