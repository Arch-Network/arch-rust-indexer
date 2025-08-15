CREATE TABLE IF NOT EXISTS blocks (
    height BIGINT PRIMARY KEY,
    hash TEXT NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    bitcoin_block_height INTEGER
);

CREATE TABLE IF NOT EXISTS transactions (
    txid TEXT PRIMARY KEY,
    block_height BIGINT NOT NULL,
    data JSONB NOT NULL,
    status TEXT NOT NULL,
    bitcoin_txids TEXT[] NOT NULL,
    created_at TIMESTAMP NOT NULL,
    FOREIGN KEY (block_height) REFERENCES blocks(height)
);

CREATE TABLE IF NOT EXISTS programs (
    program_id TEXT PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS transaction_programs (
    txid TEXT NOT NULL,
    program_id TEXT NOT NULL,
    PRIMARY KEY (txid, program_id),
    FOREIGN KEY (txid) REFERENCES transactions(txid),
    FOREIGN KEY (program_id) REFERENCES programs(program_id)
); 