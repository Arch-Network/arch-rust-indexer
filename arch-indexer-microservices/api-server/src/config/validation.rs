use anyhow::{Result, anyhow};
use std::env;

pub fn validate_required_env_vars() -> Result<()> {
    let required_vars = [
        "DB_USERNAME",
        "DB_PASSWORD",
        "DB_NAME",
        "ARCH_NODE_URL",
    ];

    for var in required_vars.iter() {
        if env::var(var).is_err() {
            return Err(anyhow!("Required environment variable {} is not set", var));
        }
    }

    Ok(())
}

pub fn validate_database_settings(settings: &crate::config::Settings) -> Result<()> {
    if settings.database.max_connections < settings.database.min_connections {
        return Err(anyhow!(
            "max_connections ({}) must be greater than min_connections ({})",
            settings.database.max_connections,
            settings.database.min_connections
        ));
    }

    Ok(())
}