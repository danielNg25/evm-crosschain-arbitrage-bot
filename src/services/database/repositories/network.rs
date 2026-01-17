use crate::services::database::models::Network;
use crate::services::database::mongodb::MongoDbClient;
use anyhow::Result;
use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use log::{debug, info};
use std::sync::Arc;

/// Network repository for MongoDB operations
#[derive(Debug, Clone)]
pub struct NetworkRepository {
    client: Arc<MongoDbClient>,
}

impl NetworkRepository {
    /// Create a new NetworkRepository instance
    pub fn new(client: Arc<MongoDbClient>) -> Self {
        Self { client }
    }

    /// Insert a network if it doesn't exist, or return existing network's chain_id
    pub async fn insert_if_not_exists(&self, network: Network) -> Result<u64> {
        let collection = self.client.collection::<Network>("networks");

        // Check if network already exists (including deleted ones for duplicate check)
        let filter = doc! { "chain_id": network.chain_id as i64 };
        let existing = collection.find_one(filter.clone()).await?;

        if existing.is_some() {
            debug!("Network with chain_id {} already exists", network.chain_id);
            return Ok(network.chain_id);
        }

        // Insert new network
        collection.insert_one(network.clone()).await?;
        info!(
            "Inserted new network: {} (chain_id: {})",
            network.name, network.chain_id
        );

        Ok(network.chain_id)
    }

    /// Find a network by chain_id
    pub async fn find_by_chain_id(&self, chain_id: u64) -> Result<Option<Network>> {
        let collection = self.client.collection::<Network>("networks");
        let filter = doc! {
            "chain_id": chain_id as i64,
            "$or": [
                { "deleted_at": { "$exists": false } },
                 { "deleted_at": null },
                { "deleted_at": 0 }
            ]
        };
        let network = collection.find_one(filter).await?;

        Ok(network)
    }

    /// Find a network by name
    pub async fn find_by_name(&self, name: &str) -> Result<Option<Network>> {
        let collection = self.client.collection::<Network>("networks");
        let filter = doc! {
            "name": name,
            "$or": [
                { "deleted_at": { "$exists": false } },
                 { "deleted_at": null },
                { "deleted_at": 0 }
            ]
        };
        let network = collection.find_one(filter).await?;

        Ok(network)
    }

    /// Update network RPCs
    pub async fn update_rpcs(&self, chain_id: u64, rpcs: Vec<String>) -> Result<()> {
        let collection = self.client.collection::<Network>("networks");
        let filter = doc! { "chain_id": chain_id as i64 };
        let update = doc! {
            "$set": {
                "rpcs": rpcs,
                "updated_at": Utc::now().timestamp() as i64
            }
        };

        collection.update_one(filter, update).await?;
        debug!("Updated RPCs for network {}", chain_id);

        Ok(())
    }

    /// Update network websocket URLs
    pub async fn update_websocket_urls(
        &self,
        chain_id: u64,
        websocket_urls: Vec<String>,
    ) -> Result<()> {
        let collection = self.client.collection::<Network>("networks");
        let filter = doc! { "chain_id": chain_id as i64 };
        let update = doc! {
            "$set": {
                "websocket_urls": websocket_urls,
                "updated_at": Utc::now().timestamp() as i64
            }
        };

        collection.update_one(filter, update).await?;
        debug!("Updated websocket URLs for network {}", chain_id);

        Ok(())
    }

    /// Update V2 factory to fee mapping
    pub async fn update_v2_factory_to_fee(
        &self,
        chain_id: u64,
        v2_factory_to_fee: std::collections::HashMap<String, u64>,
    ) -> Result<()> {
        let collection = self.client.collection::<Network>("networks");
        let filter = doc! { "chain_id": chain_id as i64 };

        // Convert HashMap to BSON document
        let mut bson_map = bson::Document::new();
        for (key, value) in v2_factory_to_fee {
            bson_map.insert(key, value as i64);
        }

        let update = doc! {
            "$set": {
                "v2_factory_to_fee": bson_map,
                "updated_at": Utc::now().timestamp() as i64
            }
        };

        collection.update_one(filter, update).await?;
        debug!("Updated V2 factory to fee mapping for network {}", chain_id);

        Ok(())
    }

    /// Update Aero factory addresses
    pub async fn update_aero_factory_addresses(
        &self,
        chain_id: u64,
        aero_factory_addresses: Vec<String>,
    ) -> Result<()> {
        let collection = self.client.collection::<Network>("networks");
        let filter = doc! { "chain_id": chain_id as i64 };
        let update = doc! {
            "$set": {
                "aero_factory_addresses": aero_factory_addresses,
                "updated_at": Utc::now().timestamp() as i64
            }
        };

        collection.update_one(filter, update).await?;
        debug!("Updated Aero factory addresses for network {}", chain_id);

        Ok(())
    }

    /// Get all networks
    pub async fn find_all(&self) -> Result<Vec<Network>> {
        let collection = self.client.collection::<Network>("networks");
        let filter = doc! {
            "$or": [
                { "deleted_at": { "$exists": false } },
                 { "deleted_at": null },
                { "deleted_at": 0 }
            ]
        };
        let mut cursor = collection.find(filter).await?;
        let mut networks = Vec::new();

        while let Some(network) = cursor.try_next().await? {
            networks.push(network);
        }

        Ok(networks)
    }

    /// Find networks updated since the given timestamp
    /// Uses updated_at field to track when networks were last modified
    pub async fn find_updated_since(&self, since_timestamp: u64) -> Result<Vec<Network>> {
        let collection = self.client.collection::<Network>("networks");
        // Check both updated_at and created_at to catch new networks (created_at) and updated ones (updated_at)
        let filter = doc! {
            "$and": [
                {
                    "$or": [
                        { "updated_at": { "$gt": since_timestamp as i64 } },
                        { "created_at": { "$gt": since_timestamp as i64 } }
                    ]
                },
                {
                    "$or": [
                        { "deleted_at": { "$exists": false } },
                         { "deleted_at": null },
                        { "deleted_at": 0 }
                    ]
                }
            ]
        };
        let mut cursor = collection.find(filter).await?;
        let mut networks = Vec::new();

        while let Some(network) = cursor.try_next().await? {
            networks.push(network);
        }

        Ok(networks)
    }

    /// Delete a network by chain_id
    pub async fn delete_by_chain_id(&self, chain_id: u64) -> Result<bool> {
        let collection = self.client.collection::<Network>("networks");
        let filter = doc! { "chain_id": chain_id as i64 };
        let result = collection.delete_one(filter).await?;

        if result.deleted_count > 0 {
            info!("Deleted network with chain_id {}", chain_id);
            Ok(true)
        } else {
            debug!("Network with chain_id {} not found", chain_id);
            Ok(false)
        }
    }
}
