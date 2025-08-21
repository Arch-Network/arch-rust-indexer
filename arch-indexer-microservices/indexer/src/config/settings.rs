use config::{Config, ConfigError};
use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub application: ApplicationSettings,
    pub arch_node: ArchNodeSettings,
    pub redis: RedisSettings,
    pub indexer: IndexerSettings,
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

impl DatabaseSettings {
    pub fn url(&self) -> String {
        format!(
            "postgresql://{}:{}@{}:{}/{}",
            self.username, self.password, self.host, self.port, self.database_name
        )
    }
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
    pub fn load() -> Result<Self, ConfigError> {
        // First, try to load from config file
        let mut config = Config::builder()
            .add_source(config::File::with_name("config").required(false));
        
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
