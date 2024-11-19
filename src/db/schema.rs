use sqlx::PgPool;

pub async fn initialize_database(pool: &PgPool) -> Result<(), sqlx::Error> {
    // Execute each statement separately
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS blocks (
            height BIGINT PRIMARY KEY,
            hash TEXT NOT NULL,
            timestamp TIMESTAMPTZ NOT NULL,
            bitcoin_block_height BIGINT
        )"
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS transactions (
            txid TEXT PRIMARY KEY,
            block_height BIGINT NOT NULL,
            data JSONB NOT NULL,
            status INTEGER NOT NULL DEFAULT 0,
            bitcoin_txids TEXT[] DEFAULT '{}',
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (block_height) REFERENCES blocks(height)
        )"
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_transactions_block_height 
            ON transactions(block_height)"
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_blocks_bitcoin_block_height 
            ON blocks(bitcoin_block_height)"
    )
    .execute(pool)
    .await?;

    Ok(())
}