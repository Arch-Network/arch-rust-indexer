use sqlx::postgres::PgPoolOptions;
use anyhow::Result;
use dotenv::dotenv;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    
    let pool = PgPoolOptions::new()
        .connect(&database_url)
        .await?;

    println!("Creating database schema...");

    sqlx::query(r#"
        CREATE TABLE IF NOT EXISTS blocks (
            height BIGINT PRIMARY KEY,
            hash TEXT NOT NULL,
            timestamp TIMESTAMPTZ NOT NULL,
            bitcoin_block_height BIGINT NOT NULL
        )
    "#)
    .execute(&pool)
    .await?;

    sqlx::query(r#"
        CREATE TABLE IF NOT EXISTS transactions (
            txid TEXT PRIMARY KEY,
            block_height BIGINT NOT NULL REFERENCES blocks(height),
            data JSONB NOT NULL,
            status INTEGER NOT NULL,
            bitcoin_txids TEXT[] NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
    "#)
    .execute(&pool)
    .await?;

    println!("Schema created successfully!");
    Ok(())
}