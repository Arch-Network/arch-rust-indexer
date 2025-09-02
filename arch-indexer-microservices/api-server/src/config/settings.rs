use config::{Config, ConfigError, Environment};
use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub application: ApplicationSettings,
    #[serde(default)]
    pub arch_node: ArchNodeSettings,
    #[serde(default)]
    pub redis: RedisSettings,
    pub indexer: IndexerSettings,
    #[serde(default)]
    pub websocket: WebSocketSettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseSettings {
    pub username: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub database_name: String,
    pub max_connections: u32,
    pub min_connections: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApplicationSettings {
    pub port: u16,
    pub host: String,
    #[serde(default = "default_cors_origin")]
    pub cors_allow_origin: String,
    #[serde(default = "default_cors_methods")]
    pub cors_allow_methods: String,
    #[serde(default = "default_cors_headers")]
    pub cors_allow_headers: String,
}

// Default functions for CORS settings
fn default_cors_origin() -> String {
    "*".to_string()
}

fn default_cors_methods() -> String {
    "GET, POST, OPTIONS".to_string()
}

fn default_cors_headers() -> String {
    "Content-Type, Authorization".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct ArchNodeSettings {
    pub url: String,
    #[serde(default = "default_websocket_url")]
    pub websocket_url: String,
}

fn default_websocket_url() -> String {
    "ws://localhost:8081".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct RedisSettings {
    pub url: String,
}

// Provide robust defaults so config loading never fails if env mapping is missing
impl Default for ArchNodeSettings {
    fn default() -> Self {
        let url = env::var("ARCH_NODE__URL")
            .or_else(|_| env::var("ARCH_NODE_URL"))
            .unwrap_or_else(|_| "http://localhost:8081".to_string());
        Self {
            url,
            websocket_url: default_websocket_url(),
        }
    }
}

impl Default for RedisSettings {
    fn default() -> Self {
        let url = env::var("REDIS__URL")
            .or_else(|_| env::var("REDIS_URL"))
            .unwrap_or_else(|_| "redis://localhost:6379".to_string());
        Self { url }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct IndexerSettings {
    pub batch_size: usize,
    pub concurrent_batches: usize,
    #[serde(default = "default_bulk_sync_mode")]
    pub bulk_sync_mode: bool,
    #[serde(default = "default_enable_realtime")]
    pub enable_realtime: bool,
}

fn default_bulk_sync_mode() -> bool {
    true
}

fn default_enable_realtime() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct WebSocketSettings {
    #[serde(default = "default_websocket_enabled")]
    pub enabled: bool,
    #[serde(default = "default_websocket_reconnect_interval")]
    pub reconnect_interval_seconds: u64,
    #[serde(default = "default_websocket_max_reconnect_attempts")]
    pub max_reconnect_attempts: usize,
}

fn default_websocket_enabled() -> bool {
    true
}

fn default_websocket_reconnect_interval() -> u64 {
    5
}

fn default_websocket_max_reconnect_attempts() -> usize {
    10
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        // Add debug prints
        println!("Environment variables:");
        for (key, value) in std::env::vars() {
            println!("{}: {}", key, value);
        }
        
        let config = Config::builder()
            .add_source(config::File::with_name("config").required(false))
            .add_source(Environment::default().separator("__"))
            // Add default values for critical settings
            .set_default("application.host", "0.0.0.0")?
            .set_default("application.port", 8080)?
            .set_default("indexer.batch_size", 100)?
            .set_default("indexer.concurrent_batches", 5)?
            // Safe fallback defaults to avoid missing field errors
            .set_default("arch_node.url", env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string()))?
            .set_default("redis.url", env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()))?
            .build()?;

        config.try_deserialize()
    }
}