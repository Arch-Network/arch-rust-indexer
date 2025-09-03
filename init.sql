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