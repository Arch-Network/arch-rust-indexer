CREATE TABLE IF NOT EXISTS blocks (
    height bigint PRIMARY KEY,
    hash text NOT NULL,
    timestamp timestamp NOT NULL,
    bitcoin_block_height bigint
);

CREATE TABLE IF NOT EXISTS transactions (
    txid text PRIMARY KEY,
    block_height bigint NOT NULL REFERENCES blocks(height),
    data jsonb NOT NULL,
    status smallint NOT NULL,
    bitcoin_txids text[] NOT NULL,
    created_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_transactions_block_height ON transactions(block_height);
CREATE INDEX IF NOT EXISTS idx_blocks_timestamp ON blocks(timestamp);