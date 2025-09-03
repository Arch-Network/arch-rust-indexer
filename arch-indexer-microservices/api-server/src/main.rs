use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tower_http::cors::CorsLayer;
use axum::http::{header, HeaderValue, Method};
use dotenv::dotenv;

use api_server::{
    api::routes::create_router, 
    config::Settings,
    metrics
};
use api_server::arch_rpc::websocket::{WebSocketClient, WebSocketEvent};
use api_server::arch_rpc::ArchRpcClient;
use api_server::indexer::RealtimeProcessor;
use tokio::sync::mpsc;

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

    info!("🚀 Starting Arch Indexer API Server...");

    // Load configuration
    let settings = Settings::new().unwrap_or_else(|e| {
        error!("Failed to load configuration: {:?}", e);
        std::process::exit(1);
    });

    info!("Loaded settings: {:?}", settings);

    // Set up metrics
    let prometheus_handle = metrics::setup_metrics_recorder();
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
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1800))
        .connect(&connection_string)
        .await?;

    info!("✅ Database connection established");
    
    // Test database connection health
    match sqlx::query("SELECT 1").execute(&pool).await {
        Ok(_) => info!("Database connection pool is healthy"),
        Err(e) => {
            error!("Database connection pool health check failed: {:?}", e);
            return Err(anyhow::anyhow!("Database connection pool is not healthy"));
        }
    }

    // Optional, idempotent timestamp type fix for AWS rollout
    // Guarded by env var so local is untouched. Safe to run repeatedly.
    if std::env::var("APPLY_TS_TZ_FIX").ok().as_deref() == Some("1") {
        info!("Applying TIMESTAMPTZ fix (guarded) on startup...");
        const TS_FIX_SQL: &str = r#"
DO $$
BEGIN
    -- blocks.timestamp
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'blocks'
          AND column_name = 'timestamp' AND data_type = 'timestamp without time zone'
    ) THEN
        EXECUTE 'ALTER TABLE public.blocks ALTER COLUMN "timestamp" TYPE timestamptz USING "timestamp" AT TIME ZONE ''UTC'';';
    END IF;

    -- transactions.created_at
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'transactions'
          AND column_name = 'created_at' AND data_type = 'timestamp without time zone'
    ) THEN
        EXECUTE 'ALTER TABLE public.transactions ALTER COLUMN "created_at" TYPE timestamptz USING "created_at" AT TIME ZONE ''UTC'';';
    END IF;

    -- programs.first_seen_at
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'programs'
          AND column_name = 'first_seen_at' AND data_type = 'timestamp without time zone'
    ) THEN
        EXECUTE 'ALTER TABLE public.programs ALTER COLUMN "first_seen_at" TYPE timestamptz USING "first_seen_at" AT TIME ZONE ''UTC'';';
    END IF;

    -- programs.last_seen_at
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'programs'
          AND column_name = 'last_seen_at' AND data_type = 'timestamp without time zone'
    ) THEN
        EXECUTE 'ALTER TABLE public.programs ALTER COLUMN "last_seen_at" TYPE timestamptz USING "last_seen_at" AT TIME ZONE ''UTC'';';
    END IF;

    -- mempool_transactions.added_at
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'mempool_transactions'
          AND column_name = 'added_at' AND data_type = 'timestamp without time zone'
    ) THEN
        EXECUTE 'ALTER TABLE public.mempool_transactions ALTER COLUMN "added_at" TYPE timestamptz USING "added_at" AT TIME ZONE ''UTC'';';
    END IF;
END
$$;
"#;
        match sqlx::query(TS_FIX_SQL).execute(&pool).await {
            Ok(_) => info!("TIMESTAMPTZ fix applied (or already in place)"),
            Err(e) => error!("Failed to apply TIMESTAMPTZ fix: {:?}", e),
        }
    }

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

    // Create core API router
    let api_router = create_router(Arc::new(pool.clone()))
        .route("/metrics", axum::routing::get(move || async move {
            let metrics = prometheus_handle.render();
            (
                [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                metrics,
            )
        }))
        .layer(cors);

    // Initialize in-process websocket server state and expose /ws
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel::<api_server::arch_rpc::websocket::WebSocketEvent>(1000);
    let ws_server = Arc::new(api_server::api::websocket_server::WebSocketServer::new(event_tx));
    let ws_router = axum::Router::new()
        .route("/ws", axum::routing::get(api_server::api::websocket_server::WebSocketServer::handle_websocket))
        .with_state(Arc::clone(&ws_server));

    // Start real-time processor wired to the in-process websocket server.
    if settings.websocket.enabled && settings.indexer.enable_realtime {
        let ws_settings = settings.websocket.clone();
        let node_ws_url = settings.arch_node.websocket_url.clone();
        let arch_http_url = settings.arch_node.url.clone();
        let pool_clone = Arc::new(pool.clone());
        let server_for_events = Arc::clone(&ws_server);
        tokio::spawn(async move {
            // Connect to Arch node WS → mpsc channel of events
            let client = WebSocketClient::new(ws_settings, node_ws_url);
            let (tx, rx) = mpsc::channel::<WebSocketEvent>(1000);
            tokio::spawn(async move { let _ = client.start(tx).await; });

            // Use the indexer's RealtimeProcessor to enrich and aggregate, then broadcast
            let rpc_client = Arc::new(ArchRpcClient::new(arch_http_url));
            let processor = RealtimeProcessor::new(pool_clone, rpc_client, Some(server_for_events));
            if let Err(e) = processor.start(rx).await {
                error!("Realtime processor ended with error: {:?}", e);
            }
        });
    }

    // Start the server with both routers
    let addr = format!("{}:{}", settings.application.host, settings.application.port);
    info!("🌐 API Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let app = api_router.merge(ws_router);
    axum::serve(listener, app).await?;

    Ok(())
}
