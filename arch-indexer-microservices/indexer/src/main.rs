use anyhow::Result;
use tracing::{info, error};
use tracing_subscriber::{self, EnvFilter};

use indexer::{config::Settings, indexer::HybridSync};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing - respect RUST_LOG, default to this crate
    let env_filter = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "indexer=debug,sqlx=info,tokio=warn".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(env_filter))
        .init();

    info!("🚀 Starting Arch Indexer Service...");

    // Load configuration
    info!("📋 Loading configuration...");
    let settings = Settings::load()?;
    info!("✅ Configuration loaded successfully");

    // Initialize database connection using the URL from settings
    let database_url = settings.database.url();
    info!("🗄️ Connecting to database: {}", database_url);
    let pool = sqlx::PgPool::connect(&database_url).await?;
    info!("✅ Database connection established");

    // Create and start hybrid sync
    info!("🔧 Creating hybrid sync...");
    let hybrid_sync = HybridSync::new(
        std::sync::Arc::new(settings),
        std::sync::Arc::new(pool),
    );

    // Start the indexer
    info!("🚀 Starting indexer...");
    if let Err(e) = hybrid_sync.start().await {
        error!("❌ Indexer failed to start: {}", e);
        std::process::exit(1);
    }

    // Keep the main thread alive
    tokio::signal::ctrl_c().await?;
    info!("🛑 Shutting down indexer service...");

    Ok(())
}
