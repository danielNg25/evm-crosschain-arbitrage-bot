use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use toml;

#[derive(Debug, Clone, Deserialize)]
pub struct MongoDbConfig {
    pub uri: String,
    pub database: String,
    pub poll_interval_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessorConfig {
    pub max_iterations: u32,
    pub default_max_amount_usd: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecutorConfig {
    pub default_recheck_interval: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    pub token: String,
    pub chat_id: String,
}

/// Main application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct RawConfig {
    pub mongodb: MongoDbConfig,
    pub processor: ProcessorConfig,
    pub executor: ExecutorConfig,
    pub telegram: TelegramConfig,
}

impl RawConfig {
    /// Load configuration from a file
    pub fn load() -> Result<Self> {
        let config_path = PathBuf::from("configs/config.toml");
        let config = fs::read_to_string(config_path)?;
        let config: RawConfig = toml::from_str(&config)?;
        Ok(config)
    }
}
