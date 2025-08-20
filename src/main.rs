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
use axum::{
    routing::get,
    Json,
};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tracing::{error, info, warn};
use std::sync::Arc;
use std::net::SocketAddr;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tower_http::cors::CorsLayer;
use axum::http::{header, HeaderValue, Method};

use config::Settings;
use crate::indexer::{BlockProcessor, ChainSync, HybridSync};
use crate::metrics::Metrics;
use dotenv::dotenv;
use crate::config::validation;
use clap::Parser;

#[derive(Parser)]
struct Args {
    /// Reset the database before starting the sync
    #[arg(long)]
    reset: bool,
}


#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // Parse command-line arguments
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let settings = Settings::new().unwrap_or_else(|e| {
        error!("Failed to load configuration: {:?}", e);
        std::process::exit(1);
    });

    info!("Loaded settings: {:?}", settings);

    // Set up metrics
    let prometheus_handle = metrics::setup_metrics_recorder();
    let metrics = Metrics::new(prometheus_handle);

    info!("Prometheus metrics initialized");

    let connection_string = if settings.database.host.starts_with("/cloudsql") {
        format!(
            "postgres://{}:{}@localhost/{}?host={}",
            settings.database.username,
            settings.database.password,
            settings.database.database_name,
            settings.database.host
        )
    } else {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            settings.database.username,
            settings.database.password,
            settings.database.host,
            settings.database.port,
            settings.database.database_name
        )
    };

    info!("Connection string (sanitized): {}", connection_string.replace(&settings.database.password, "REDACTED"));

    // Initialize database connection pool
    let pool = PgPoolOptions::new()
        .max_connections(settings.database.max_connections)
        .min_connections(settings.database.min_connections)
        .connect(&connection_string)
        .await?;

    info!("Successfully connected to database");

    // Reset the database if the --reset flag is provided
    if args.reset {
        reset_database(&pool).await?;
        info!("Database reset successfully");
    }

    // Initialize Redis connection
    let redis_client = redis::Client::open(settings.redis.url.as_str())?;

    info!("Successfully connected to Redis");

    // Initialize Arch RPC client
    let arch_client = ArchRpcClient::new(settings.arch_node.url.clone());

    info!("Successfully connected to Arch node");

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
        Arc::new(arch_client.clone()),
    ));

    info!("Successfully initialized block processor");

    // Initialize real-time sync if enabled
    let realtime_sync = if settings.websocket.enabled {
        info!("Initializing real-time sync with WebSocket URL: {}", settings.arch_node.websocket_url);
        Some(RealtimeSync::new(
            Arc::clone(&processor),
            settings.arch_node.websocket_url.clone(),
            Arc::new(arch_client.clone()),
        ))
    } else {
        info!("Real-time sync disabled");
        None
    };

    // Run sync_missing_program_data in a separate task
    let processor_clone = Arc::clone(&processor);
    tokio::spawn(async move {
        if let Err(e) = processor_clone.sync_missing_program_data().await {
            error!("Failed to sync missing program data: {:?}", e);
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(settings.application.cors_allow_origin.parse::<HeaderValue>().unwrap_or_else(|_| {
            HeaderValue::from_static("*")
        }))
        .allow_methods(
            settings.application.cors_allow_methods
                .split(',')
                .map(|s| s.trim().parse::<Method>().unwrap_or(Method::GET))
                .collect::<Vec<Method>>()
        )
        .allow_headers(
            settings.application.cors_allow_headers
                .split(',')
                .map(|s| match s.trim().to_lowercase().as_str() {
                    "content-type" => header::CONTENT_TYPE,
                    "authorization" => header::AUTHORIZATION,
                    _ => header::HeaderName::from_lowercase(s.trim().to_lowercase().as_bytes()).unwrap_or(header::CONTENT_TYPE),
                })
                .collect::<Vec<_>>()
        );

    // Create API router
    let app = Router::new()
        .merge(api::create_router(Arc::new(pool), Arc::clone(&processor)))
        .route("/metrics", axum::routing::get(move || async move {
            let metrics = metrics.prometheus_handle.render();
            (
                [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                metrics,
            )
        }))
        .layer(cors);

    info!("Successfully initialized API router");

    // Get the starting height for sync
    let current_height = match get_sync_start_height(&processor).await {
        Ok(height) => height,
        Err(e) => {
            error!("Failed to get sync start height: {:?}", e);
            std::process::exit(1);
        }
    };

    // Start the chain sync process
    let sync_handle = if settings.websocket.enabled && settings.indexer.enable_realtime {
        info!("Starting hybrid sync with real-time WebSocket support");
        
        let hybrid_sync = HybridSync::new(
            Arc::new(settings.clone()),
            pool.clone(),
        );
        
        tokio::spawn(async move {
            hybrid_sync.start().await
        })
    } else {
        info!("Starting traditional sync without real-time support");
        
        let sync = ChainSync::new(pool.clone());
        
        tokio::spawn(async move {
            sync.start().await
        })
    };

    // Start the HTTP server
    let addr = SocketAddr::from((
        settings.application.host.parse::<std::net::IpAddr>().unwrap_or_else(|_| "0.0.0.0".parse().unwrap()),
        settings.application.port
    ));
    info!("listening on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    info!("Successfully bound to address: {}", addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Wait for sync to complete
    sync_handle.await??;

    info!("Starting Arch Indexer service...");
    info!("API server listening on {}", addr);

    Ok(())
}

async fn reset_database(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::query("TRUNCATE TABLE blocks, transactions RESTART IDENTITY CASCADE")
        .execute(pool)
        .await?;
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