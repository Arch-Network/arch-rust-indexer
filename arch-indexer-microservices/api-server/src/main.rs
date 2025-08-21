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

    // Create the router
    let app = create_router(Arc::new(pool))
        .route("/metrics", axum::routing::get(move || async move {
            let metrics = prometheus_handle.render();
            (
                [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                metrics,
            )
        }))
        .layer(cors);

    // Start the server
    let addr = format!("{}:{}", settings.application.host, settings.application.port);
    info!("🌐 API Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    axum::serve(listener, app).await?;

    Ok(())
}
