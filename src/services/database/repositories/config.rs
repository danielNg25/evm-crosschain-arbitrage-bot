use crate::services::database::models::Config;
use crate::services::database::mongodb::MongoDbClient;
use anyhow::Result;
use bson::doc;
use chrono::Utc;
use log::{debug, info};
use std::sync::Arc;

/// Config repository for MongoDB operations
/// Config is treated as a singleton - there should only be one config record
#[derive(Debug, Clone)]
pub struct ConfigRepository {
    client: Arc<MongoDbClient>,
}

impl ConfigRepository {
    /// Create a new ConfigRepository instance
    pub fn new(client: Arc<MongoDbClient>) -> Self {
        Self { client }
    }

    /// Get the config (singleton pattern - returns the first/only config)
    pub async fn get(&self) -> Result<Option<Config>> {
        let collection = self.client.collection::<Config>("configs");
        let config = collection.find_one(doc! {}).await?;
        Ok(config)
    }

    /// Get or create the config with default values
    pub async fn get_or_create(
        &self,
        default_max_amount_usd: f64,
        default_recheck_interval: u64,
    ) -> Result<Config> {
        let collection = self.client.collection::<Config>("configs");

        // Try to find existing config
        if let Some(config) = collection.find_one(doc! {}).await? {
            return Ok(config);
        }

        // Create new config if none exists
        let config = Config::new(default_max_amount_usd, default_recheck_interval);
        collection.insert_one(config.clone()).await?;
        info!(
            "Created new config with max_amount_usd: {}",
            default_max_amount_usd
        );

        Ok(config)
    }

    /// Update max_amount_usd
    pub async fn update_max_amount_usd(&self, max_amount_usd: f64) -> Result<()> {
        let collection = self.client.collection::<Config>("configs");
        let filter = doc! {};
        let update = doc! {
            "$set": {
                "max_amount_usd": max_amount_usd,
                "updated_at": Utc::now().timestamp() as i64
            }
        };

        collection.update_one(filter, update).await?;
        info!("Updated max_amount_usd to {}", max_amount_usd);

        Ok(())
    }

    /// Update recheck_interval
    pub async fn update_recheck_interval(&self, recheck_interval: u64) -> Result<()> {
        let collection = self.client.collection::<Config>("configs");
        let filter = doc! {};
        let update = doc! {
            "$set": {
                "recheck_interval": recheck_interval as i64,
                "updated_at": Utc::now().timestamp() as i64
            }
        };

        collection.update_one(filter, update).await?;
        info!("Updated recheck_interval to {}", recheck_interval);

        Ok(())
    }

    /// Upsert config (insert if not exists, update if exists)
    pub async fn upsert(&self, config: Config) -> Result<()> {
        let collection = self.client.collection::<Config>("configs");
        let filter = doc! {};
        let mut updated_config = config;
        updated_config.updated_at = Utc::now().timestamp() as u64;

        let update = doc! {
            "$set": bson::to_bson(&updated_config)?
        };

        let result = collection.update_one(filter.clone(), update).await?;

        if result.matched_count == 0 {
            collection.insert_one(updated_config).await?;
            info!("Inserted new config");
        } else {
            debug!("Updated config");
        }

        Ok(())
    }
}
