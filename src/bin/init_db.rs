use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use arch_indexer::db::schema::initialize_database;
use arch_indexer::config::Settings;

#[tokio::main]
async fn main() -> Result<()> {
    let settings = Settings::new()?;
    
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "postgres://{}:{}@{}:{}/{}",
            settings.database.username,
            settings.database.password,
            settings.database.host,
            settings.database.port,
            settings.database.database_name
        ))
        .await?;

    initialize_database(&pool).await?;
    println!("Database initialized successfully");

    Ok(())
}