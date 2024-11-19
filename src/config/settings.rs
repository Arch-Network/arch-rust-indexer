use serde::Deserialize;
use config::{Config, ConfigError, Environment};

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub application: ApplicationSettings,
    pub arch_node: ArchNodeSettings,
    pub redis: RedisSettings,
    pub indexer: IndexerSettings,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseSettings {
    pub username: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub database_name: String,
    pub max_connections: u32,
    pub min_connections: u32,
}

#[derive(Debug, Deserialize)]
pub struct ApplicationSettings {
    pub port: u16,
    pub host: String,
}

#[derive(Debug, Deserialize)]
pub struct ArchNodeSettings {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct RedisSettings {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct IndexerSettings {
    pub batch_size: usize,
    pub concurrent_batches: usize,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(Environment::default().separator("__"))
            .build()?;

        config.try_deserialize()
    }
}