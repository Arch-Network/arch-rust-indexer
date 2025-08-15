use sqlx::postgres::PgPoolOptions;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file
    dotenv::dotenv().ok();

    // Construct database URL from environment variables
    let database_url = format!(
        "postgres://{}:{}@{}:{}/{}",
        env::var("DATABASE__USERNAME").expect("DATABASE__USERNAME must be set"),
        env::var("DATABASE__PASSWORD").expect("DATABASE__PASSWORD must be set"),
        env::var("DATABASE__HOST").expect("DATABASE__HOST must be set"),
        env::var("DATABASE__PORT").expect("DATABASE__PORT must be set"),
        env::var("DATABASE__DATABASE_NAME").expect("DATABASE__DATABASE_NAME must be set")
    );

    // Create connection pool
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;

    // Read and execute the initialization SQL
    let init_sql = include_str!("../../migrations/init.sql");
    sqlx::query(init_sql)
        .execute(&pool)
        .await?;

    println!("Database schema initialized successfully!");

    Ok(())
}