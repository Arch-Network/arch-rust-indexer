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

    // Start a lightweight real-time forwarder: connect to Arch node WS and broadcast events
    if settings.websocket.enabled && settings.indexer.enable_realtime {
        let ws_settings = settings.websocket.clone();
        let node_ws_url = settings.arch_node.websocket_url.clone();
        let server_for_events = Arc::clone(&ws_server);
        tokio::spawn(async move {
            let client = WebSocketClient::new(ws_settings, node_ws_url);
            let (tx, mut rx) = tokio::sync::mpsc::channel::<WebSocketEvent>(1000);
            // Connection task
            tokio::spawn(async move {
                let _ = client.start(tx).await;
            });
            // Forward only block events to UI clients
            while let Some(event) = rx.recv().await {
                if event.topic == "block" {
                    let _ = server_for_events.broadcast_event(event).await;
                }
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
