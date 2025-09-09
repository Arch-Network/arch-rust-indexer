use anyhow::Result;
use tracing::{info, error};
use tracing_subscriber::{self, EnvFilter};
use sqlx::PgPool;
use std::env;
use axum::{routing::get, Router};
use axum::http::StatusCode;
use std::net::SocketAddr;

use indexer::{config::Settings, indexer::HybridSync};

#[cfg(feature = "atlas_ingestion")]
use indexer::pipeline_atlas;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing - respect RUST_LOG, default to this crate
    let env_filter = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "indexer=debug,sqlx=info,tokio=warn".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(env_filter))
        .init();

    // Metrics exporter (Prometheus)
    let metrics_addr: SocketAddr = env::var("METRICS_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:9090".to_string())
        .parse()
        .unwrap_or_else(|_| "0.0.0.0:9090".parse().unwrap());
    // metrics-exporter-prometheus 0.12: use PrometheusHandle and serve via axum
    let handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| anyhow::anyhow!("failed to install metrics recorder: {}", e))?;
    let metrics_app = Router::new().route(
        "/metrics",
        get({
            let handle = handle.clone();
            move || {
                let handle = handle.clone();
                async move { (StatusCode::OK, handle.render()) }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind(metrics_addr).await?;
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, metrics_app).await {
            error!("metrics server failed: {}", e);
        }
    });
    info!("📈 Prometheus metrics exporter listening on {}", metrics_addr);

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

    // Optional database reset for fresh start
    if env::var("RESET_DB").map(|v| v == "true" || v == "1").unwrap_or(false) {
        info!("🧹 RESET_DB is set; resetting database schema...");
        if let Err(e) = reset_database(&pool).await {
            error!("Failed to reset database: {}", e);
            std::process::exit(1);
        }
        if env::var("RESET_AND_EXIT").map(|v| v == "true" || v == "1").unwrap_or(false) {
            info!("✅ Reset complete; exiting due to RESET_AND_EXIT=true");
            return Ok(())
        }
    }

    // Ensure schema exists (bootstrap if missing)
    if let Err(e) = bootstrap_schema_if_missing(&pool).await {
        error!("Failed to bootstrap schema: {}", e);
        std::process::exit(1);
    }

    // Choose runtime path via env: INDEXER_RUNTIME=atlas|legacy (default atlas if compiled)
    let runtime = env::var("INDEXER_RUNTIME").unwrap_or_else(|_| "atlas".to_string());
    let rpc_url_env = env::var("ARCH_NODE_URL").ok();
    let ws_url_env = env::var("ARCH_NODE_WEBSOCKET_URL").ok();
    match runtime.as_str() {
        "atlas" => {
            #[cfg(feature = "atlas_ingestion")]
            {
                let rpc_fallback = &settings.arch_node.url;
                let ws_fallback = &settings.arch_node.websocket_url;
                let rpc_url = rpc_url_env.as_deref().unwrap_or(rpc_fallback);
                let ws_url = ws_url_env.as_deref().unwrap_or(ws_fallback);
                let rocks_path = std::env::var("ATLAS_CHECKPOINT_PATH").unwrap_or_else(|_| "./.atlas_checkpoints".to_string());
                info!("🧪 INDEXER_RUNTIME=atlas; starting syncing pipeline (rpc={}, ws={})", rpc_url, ws_url);
                if let Err(e) = pipeline_atlas::run_syncing_pipeline(rpc_url, ws_url, &rocks_path, std::sync::Arc::new(pool)).await {
                    error!("Atlas syncing pipeline failed: {}", e);
                    std::process::exit(1);
                }
            }
            #[cfg(not(feature = "atlas_ingestion"))]
            {
                error!("INDEXER_RUNTIME=atlas set but atlas_ingestion feature not compiled; falling back to legacy runtime");
                let hybrid_sync = HybridSync::new(
                    std::sync::Arc::new(settings),
                    std::sync::Arc::new(pool),
                );
                info!("🚀 Starting legacy indexer...");
                if let Err(e) = hybrid_sync.start().await {
                    error!("❌ Indexer failed to start: {}", e);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            // Legacy runtime
            info!("🔧 INDEXER_RUNTIME=legacy; starting legacy path");
            let hybrid_sync = HybridSync::new(
                std::sync::Arc::new(settings),
                std::sync::Arc::new(pool),
            );
            info!("🚀 Starting indexer...");
            if let Err(e) = hybrid_sync.start().await {
                error!("❌ Indexer failed to start: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Keep the main thread alive
    tokio::signal::ctrl_c().await?;
    info!("🛑 Shutting down indexer service...");

    Ok(())
}

/// Creates the base database schema if it's missing. This protects fresh RDS instances
/// from failing with "relation blocks does not exist" before migrations are applied.
async fn bootstrap_schema_if_missing(pool: &PgPool) -> Result<()> {
    // Check if the core table exists
    let exists: Option<String> = sqlx::query_scalar("SELECT to_regclass('public.blocks')::text")
        .fetch_one(pool)
        .await
        .ok()
        .flatten();

    if exists.is_some() {
        info!("🔎 Detected existing schema; skipping bootstrap");
        return Ok(());
    }

    info!("🧱 No schema detected; applying base schema");
    // Minimal base schema sufficient for the indexer to operate. Full migrations
    // can still be applied later, but this prevents startup failures on fresh DBs.
    const BASE_SCHEMA: &str = r#"
        CREATE TABLE IF NOT EXISTS blocks (
            height BIGINT PRIMARY KEY,
            hash TEXT NOT NULL,
            timestamp TIMESTAMPTZ NOT NULL,
            bitcoin_block_height BIGINT
        );

        CREATE TABLE IF NOT EXISTS transactions (
            txid TEXT PRIMARY KEY,
            block_height BIGINT NOT NULL,
            data JSONB NOT NULL,
            status JSONB NOT NULL DEFAULT '0'::jsonb,
            bitcoin_txids TEXT[] DEFAULT '{}',
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (block_height) REFERENCES blocks(height)
        );

        CREATE TABLE IF NOT EXISTS accounts (
            pubkey TEXT PRIMARY KEY,
            lamports BIGINT NOT NULL,
            owner TEXT NOT NULL,
            data BYTEA NOT NULL,
            height BIGINT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_transactions_block_height ON transactions(block_height);
        CREATE INDEX IF NOT EXISTS idx_blocks_bitcoin_block_height ON blocks(bitcoin_block_height);
        CREATE INDEX IF NOT EXISTS idx_blocks_timestamp ON blocks(timestamp);

        CREATE TABLE IF NOT EXISTS programs (
            program_id TEXT PRIMARY KEY,
            first_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            last_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            transaction_count BIGINT NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS transaction_programs (
            txid TEXT REFERENCES transactions(txid) ON DELETE CASCADE,
            program_id TEXT REFERENCES programs(program_id) ON DELETE CASCADE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (txid, program_id)
        );

        CREATE INDEX IF NOT EXISTS idx_transaction_programs_program_id ON transaction_programs(program_id);
        CREATE INDEX IF NOT EXISTS idx_accounts_owner ON accounts(owner);
        CREATE INDEX IF NOT EXISTS idx_accounts_height ON accounts(height);
    "#;

    // Execute inside a transaction for safety, splitting into separate statements
    let mut tx = pool.begin().await?;
    for stmt in BASE_SCHEMA.split(';') {
        let sql = stmt.trim();
        if sql.is_empty() { continue; }
        sqlx::query(sql).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    info!("✅ Base schema installed");

    Ok(())
}

/// Drops known tables, triggers, and helper functions, then recreates base schema.
async fn reset_database(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;

    // Drop trigger if present
    let drops = [
        "DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions",
        "DROP FUNCTION IF EXISTS update_transaction_programs()",
        "DROP FUNCTION IF EXISTS normalize_program_id(text)",
        "DROP FUNCTION IF EXISTS decode_base58(text)",
        "DROP TABLE IF EXISTS transaction_programs",
        "DROP TABLE IF EXISTS programs",
        "DROP TABLE IF EXISTS transactions",
        "DROP TABLE IF EXISTS blocks"
    ];

    for stmt in drops.iter() {
        sqlx::query(stmt).execute(&mut *tx).await.ok();
    }
    tx.commit().await?;

    // Recreate base schema
    bootstrap_schema_if_missing(pool).await?;
    Ok(())
}
