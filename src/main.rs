mod api;
mod arch_rpc;
mod config;
mod db;
mod indexer;
mod metrics;

use tokio::net::TcpListener;
use anyhow::Result;
use arch_rpc::ArchRpcClient;
use axum::Router;
use sqlx::postgres::PgPoolOptions;
use tracing::{error, info};
use std::sync::Arc;
use std::net::SocketAddr;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Settings;
use crate::indexer::{BlockProcessor, ChainSync};
use crate::metrics::Metrics;
use dotenv::dotenv;


#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
    // Load configuration
    let settings = Settings::new().expect("Failed to load configuration");

    // Set up metrics
    let prometheus_handle = metrics::setup_metrics_recorder();
    let metrics = Metrics::new(prometheus_handle);

    // Initialize database connection pool
    let pool = PgPoolOptions::new()
        .max_connections(settings.database.max_connections)
        .min_connections(settings.database.min_connections)
        .connect(&format!(
            "postgres://{}:{}@{}:{}/{}",
            settings.database.username,
            settings.database.password,
            settings.database.host,
            settings.database.port,
            settings.database.database_name
        ))
        .await?;

    // Initialize Redis connection
    let redis_client = redis::Client::open(settings.redis.url.as_str())?;

    // Initialize Arch RPC client
    let arch_client = ArchRpcClient::new(settings.arch_node.url.clone());

    // Verify node connection
    match arch_client.is_node_ready().await {
        Ok(true) => info!("Successfully connected to Arch node"),
        Ok(false) => {
            tracing::error!("Arch node is not ready");
            std::process::exit(1);
        }
        Err(e) => {
            tracing::error!("Failed to connect to Arch node: {:?}", e);
            std::process::exit(1);
        }
    }

    // Initialize block processor
    let processor = Arc::new(BlockProcessor::new(
        pool.clone(),
        redis_client,
        arch_client.clone(),
    ));

    // Create API router
    let app = Router::new()
        .merge(api::create_router(Arc::new(pool), Arc::clone(&processor)))
        .route("/metrics", axum::routing::get(move || async move {
            let metrics = metrics.prometheus_handle.render();
            (
                [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                metrics,
            )
        }));

    // Get the starting height for sync
    let current_height = match get_sync_start_height(&processor).await {
        Ok(height) => height,
        Err(e) => {
            error!("Failed to get sync start height: {:?}", e);
            std::process::exit(1);
        }
    };

    // Start the chain sync process
    let sync = ChainSync::new(
        Arc::clone(&processor),
        current_height,
        settings.indexer.batch_size,
        settings.indexer.concurrent_batches,
    );

    // Spawn the sync task
    let sync_handle = tokio::spawn(async move {
        sync.start().await
    });

    // Start the HTTP server
    let addr = SocketAddr::from(([0, 0, 0, 0], settings.application.port));
    info!("listening on {}", addr);

    let addr = SocketAddr::from(([0, 0, 0, 0], settings.application.port));
    info!("listening on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Wait for sync to complete
    sync_handle.await??;

    Ok(())
}
async fn get_sync_start_height(processor: &BlockProcessor) -> Result<i64> {
    // Try to get the last processed height from the database
    if let Some(height) = processor.get_last_processed_height().await? {
        info!("Resuming sync from height {}", height);
        Ok(height + 1)
    } else {
        info!("Starting sync from genesis (height 0)");
        Ok(0)
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}