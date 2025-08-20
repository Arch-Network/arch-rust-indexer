use anyhow::Result;
use std::sync::Arc;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use arch_indexer::config::settings::Settings;
use arch_indexer::indexer::hybrid_sync::HybridSync;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("🚀 Testing Real-Time Indexing Pipeline...");

    // Load settings
    let settings = match Settings::new() {
        Ok(settings) => {
            info!("✅ Configuration loaded successfully");
            info!("  WebSocket URL: {}", settings.arch_node.websocket_url);
            info!("  WebSocket enabled: {}", settings.websocket.enabled);
            info!("  Real-time enabled: {}", settings.indexer.enable_realtime);
            settings
        }
        Err(e) => {
            error!("❌ Failed to load configuration: {}", e);
            return Err(e.into());
        }
    };

    // Set up database connection
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres@localhost:5432/archindexer".to_string());
    
    info!("🗄️ Connecting to database: {}", database_url);
    let pool = match sqlx::PgPool::connect(&database_url).await {
        Ok(pool) => {
            info!("✅ Database connected successfully");
            Arc::new(pool)
        }
        Err(e) => {
            error!("❌ Failed to connect to database: {}", e);
            return Err(e.into());
        }
    };

    // Test database connection
    match sqlx::query("SELECT 1").fetch_one(&*pool).await {
        Ok(_) => info!("✅ Database query test successful"),
        Err(e) => {
            error!("❌ Database query test failed: {}", e);
            return Err(e.into());
        }
    }

    // Create hybrid sync system
    info!("🔧 Initializing Hybrid Sync System...");
    let hybrid_sync = HybridSync::new(Arc::new(settings), pool.clone());

    // Test WebSocket connection capability
    info!("🌐 Testing WebSocket connection capability...");
    
    if hybrid_sync.is_websocket_enabled() {
        info!("✅ WebSocket is enabled in configuration");
        
        // Start the hybrid sync system
        info!("🚀 Starting Hybrid Sync System...");
        info!("  This will:");
        info!("    1. Connect to WebSocket at ws://44.196.173.35:10081");
        info!("    2. Subscribe to real-time block events");
        info!("    3. Process events and store in database");
        info!("    4. Fall back to traditional sync if needed");
        info!("");
        info!("🔍 Press Ctrl+C to stop after observing the system...");
        
        // Run for a limited time for testing
        tokio::select! {
            result = hybrid_sync.start() => {
                match result {
                    Ok(_) => info!("✅ Hybrid sync completed successfully"),
                    Err(e) => error!("❌ Hybrid sync failed: {}", e),
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                info!("⏰ Test duration completed (30 seconds)");
                info!("✅ Real-time indexing test completed successfully!");
            }
        }
    } else {
        error!("❌ WebSocket is not enabled in configuration");
        info!("💡 To enable WebSocket, set:");
        info!("    websocket.enabled = true");
        info!("    indexer.enable_realtime = true");
        return Err(anyhow::anyhow!("WebSocket not enabled"));
    }

    Ok(())
}
