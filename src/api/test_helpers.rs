use sqlx::PgPool;
use anyhow::Result;
use dotenv::from_filename;

pub async fn setup_test_db() -> Result<PgPool> {
    // Load test environment variables
    from_filename(".env.test").ok();
    
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env.test");
    
    let pool = PgPool::connect(&database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?;

    // Clean up any existing data and schema
    cleanup_test_db(&pool).await?;
    
    // Initialize schema
    sqlx::query("DROP TYPE IF EXISTS transaction_status CASCADE")
        .execute(&pool)
        .await?;

    // Initialize database schema
    crate::db::schema::initialize_database(&pool).await?;

    Ok(pool)
}

pub async fn cleanup_test_db(pool: &PgPool) -> Result<()> {
    // Drop all tables and types
    sqlx::query("DROP TABLE IF EXISTS transactions CASCADE")
        .execute(pool)
        .await?;
    
    sqlx::query("DROP TABLE IF EXISTS blocks CASCADE")
        .execute(pool)
        .await?;

    Ok(())
}