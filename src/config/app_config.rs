use crate::config::RawConfig;
use anyhow::{anyhow, Result};
use serde::Deserialize;

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
    pub error_thread_id: u64,
    pub opp_thread_id: u64,
    pub error_log_interval_secs: u64,
}

/// Main application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub mongodb: MongoDbConfig,
    pub processor: ProcessorConfig,
    pub executor: ExecutorConfig,
    pub telegram: TelegramConfig,
}

impl AppConfig {
    /// Load configuration from a file
    pub fn from_raw_config(raw_config: RawConfig) -> Result<Self> {
        Ok(Self {
            mongodb: MongoDbConfig {
                uri: raw_config.mongodb.uri,
                database: raw_config.mongodb.database,
                poll_interval_secs: raw_config.mongodb.poll_interval_secs,
            },
            processor: ProcessorConfig {
                max_iterations: raw_config.processor.max_iterations,
                default_max_amount_usd: raw_config.processor.default_max_amount_usd,
            },
            executor: ExecutorConfig {
                default_recheck_interval: raw_config.executor.default_recheck_interval,
            },
            telegram: TelegramConfig {
                token: raw_config.telegram.token,
                chat_id: raw_config.telegram.chat_id,
                error_thread_id: raw_config.telegram.error_thread_id,
                opp_thread_id: raw_config.telegram.opp_thread_id,
                error_log_interval_secs: raw_config.telegram.error_log_interval_secs,
            },
        })
    }
}

impl MongoDbConfig {
    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.uri.is_empty() {
            return Err(anyhow!("MongoDB URI not configured"));
        }

        if self.database.is_empty() {
            return Err(anyhow!("MongoDB database name not configured"));
        }

        Ok(())
    }
}
