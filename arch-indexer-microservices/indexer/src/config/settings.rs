use config::{Config, ConfigError};
use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub database: DatabaseSettings,
    #[serde(default)]
    pub application: ApplicationSettings,
    #[serde(default)]
    pub arch_node: ArchNodeSettings,
    #[serde(default)]
    pub redis: RedisSettings,
    #[serde(default)]
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
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
}

impl DatabaseSettings {
    pub fn url(&self) -> String {
        format!(
            "postgresql://{}:{}@{}:{}/{}",
            self.username, self.password, self.host, self.port, self.database_name
        )
    }
}

fn default_max_connections() -> u32 { 30 }
fn default_min_connections() -> u32 { 5 }

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ApplicationSettings {
    #[serde(default = "default_app_port")]
    pub port: u16,
    #[serde(default = "default_app_host")]
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

fn default_app_port() -> u16 { 8080 }
fn default_app_host() -> String { "0.0.0.0".to_string() }

#[derive(Debug, Deserialize, Clone)]
pub struct ArchNodeSettings {
    #[serde(default = "default_arch_node_url")]
    pub url: String,
    #[serde(default = "default_websocket_url")]
    pub websocket_url: String,
}

impl Default for ArchNodeSettings {
    fn default() -> Self {
        Self {
            url: default_arch_node_url(),
            websocket_url: default_websocket_url(),
        }
    }
}

fn default_websocket_url() -> String {
    "ws://localhost:8081".to_string()
}

fn default_arch_node_url() -> String {
    "http://localhost:8080".to_string()
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RedisSettings {
    #[serde(default = "default_redis_url")]
    pub url: String,
}

fn default_redis_url() -> String { "redis://localhost:6379".to_string() }

#[derive(Debug, Deserialize, Clone)]
pub struct IndexerSettings {
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_concurrent_batches")]
    pub concurrent_batches: usize,
    #[serde(default = "default_bulk_sync_mode")]
    pub bulk_sync_mode: bool,
    #[serde(default = "default_enable_realtime")]
    pub enable_realtime: bool,
}

impl Default for IndexerSettings {
    fn default() -> Self {
        Self {
            batch_size: default_batch_size(),
            concurrent_batches: default_concurrent_batches(),
            bulk_sync_mode: default_bulk_sync_mode(),
            enable_realtime: default_enable_realtime(),
        }
    }
}

fn default_bulk_sync_mode() -> bool {
    true
}

fn default_enable_realtime() -> bool {
    true
}

fn default_batch_size() -> usize { 100 }
fn default_concurrent_batches() -> usize { 2 }

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
    pub fn load() -> Result<Self, ConfigError> {
        // First, try to load from config file
        let mut config = Config::builder()
            .add_source(config::File::with_name("config").required(false));
        // Provide sane defaults at the config layer so deserialization never fails due to
        // missing sections/fields in production (ECS) where we rely on env vars.
        config = config
            .set_default("application.port", default_app_port() as i64)?
            .set_default("application.host", default_app_host())?
            .set_default("application.cors_allow_origin", default_cors_origin())?
            .set_default("application.cors_allow_methods", default_cors_methods())?
            .set_default("application.cors_allow_headers", default_cors_headers())?
            .set_default("arch_node.url", default_arch_node_url())?
            .set_default("arch_node.websocket_url", default_websocket_url())?
            .set_default("redis.url", default_redis_url())?
            .set_default("indexer.batch_size", default_batch_size() as i64)?
            .set_default("indexer.concurrent_batches", default_concurrent_batches() as i64)?
            .set_default("indexer.bulk_sync_mode", default_bulk_sync_mode())?
            .set_default("indexer.enable_realtime", default_enable_realtime())?
            .set_default("websocket.enabled", default_websocket_enabled())?
            .set_default("websocket.reconnect_interval_seconds", default_websocket_reconnect_interval() as i64)?
            .set_default("websocket.max_reconnect_attempts", default_websocket_max_reconnect_attempts() as i64)?;
        
        // Check for environment variables and override config file values
        if let Ok(database_url) = env::var("DATABASE_URL") {
            // Parse DATABASE_URL to extract components
            if let Ok(parsed_url) = url::Url::parse(&database_url) {
                let username = parsed_url.username().to_string();
                let password = parsed_url.password().unwrap_or("").to_string();
                let host = parsed_url.host_str().unwrap_or("localhost").to_string();
                let port = parsed_url.port().unwrap_or(5432);
                let database_name = parsed_url.path().trim_start_matches('/').to_string();
                
                // Override config file values with environment variables
                config = config
                    .set_override("database.username", username)?
                    .set_override("database.password", password)?
                    .set_override("database.host", host)?
                    .set_override("database.port", port)?
                    .set_override("database.database_name", database_name)?;
            }
        }
        
        if let Ok(redis_url) = env::var("REDIS_URL") {
            config = config.set_override("redis.url", redis_url)?;
        }
        
        if let Ok(arch_node_url) = env::var("ARCH_NODE_URL") {
            config = config.set_override("arch_node.url", arch_node_url)?;
        }
        // Backward-compat for misnamed env var with double underscore
        if let Ok(arch_node_url) = env::var("ARCH_NODE__URL") {
            config = config.set_override("arch_node.url", arch_node_url)?;
        }
        
        if let Ok(arch_node_websocket_url) = env::var("ARCH_NODE_WEBSOCKET_URL") {
            config = config.set_override("arch_node.websocket_url", arch_node_websocket_url)?;
        }
        
        if let Ok(enable_realtime) = env::var("ENABLE_REALTIME") {
            config = config.set_override("indexer.enable_realtime", enable_realtime)?;
        }
        
        if let Ok(websocket_enabled) = env::var("WEBSOCKET_ENABLED") {
            config = config.set_override("websocket.enabled", websocket_enabled)?;
        }
        
        let final_config = config.build()?;
        final_config.try_deserialize()
    }
}
