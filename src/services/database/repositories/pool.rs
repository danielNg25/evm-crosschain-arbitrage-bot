use crate::services::database::models::pool::Pool;
use crate::services::database::models::utils::address_to_string;
use crate::services::database::mongodb::MongoDbClient;
use alloy::primitives::Address;
use anyhow::Result;
use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use log::{debug, info};
use std::collections::HashSet;
use std::sync::Arc;

/// Pool repository for MongoDB operations
#[derive(Debug, Clone)]
pub struct PoolRepository {
    client: Arc<MongoDbClient>,
}

impl PoolRepository {
    /// Create a new PoolRepository instance
    pub fn new(client: Arc<MongoDbClient>) -> Self {
        Self { client }
    }

    /// Insert a pool if it doesn't exist
    pub async fn insert_if_not_exists(&self, pool: Pool) -> Result<(String, bool)> {
        let collection = self.client.collection::<Pool>("pools");
        let addr_str = pool.address.clone();

        // Check if pool already exists
        let filter = doc! {
            "network_id": pool.network_id as i64,
            "address": &addr_str
        };

        let existing = collection.find_one(filter).await?;

        if existing.is_some() {
            debug!("Pool {} already exists", addr_str);
            return Ok((addr_str, false));
        }

        // Insert new pool
        collection.insert_one(pool.clone()).await?;
        info!(
            "Inserted new pool: {} (network_id: {})",
            addr_str, pool.network_id
        );

        Ok((addr_str, true))
    }

    /// Find a pool by network_id and address
    pub async fn find_by_address(
        &self,
        network_id: u64,
        address: &Address,
    ) -> Result<Option<Pool>> {
        let collection = self.client.collection::<Pool>("pools");
        let addr_str = address_to_string(address);

        let filter = doc! {
            "network_id": network_id as i64,
            "address": &addr_str,
            "$or": [
                { "deleted_at": { "$exists": false } },
                 { "deleted_at": null },
                { "deleted_at": 0 }
            ]
        };

        let pool = collection.find_one(filter).await?;
        Ok(pool)
    }

    /// Find all pools for a given network
    pub async fn find_by_network_id(&self, network_id: u64) -> Result<Vec<Pool>> {
        let collection = self.client.collection::<Pool>("pools");
        let filter = doc! {
            "network_id": network_id as i64,
            "$or": [
                { "deleted_at": { "$exists": false } },
                 { "deleted_at": null },
                { "deleted_at": 0 }
            ]
        };
        let mut cursor = collection.find(filter).await?;
        let mut pools = Vec::new();

        while let Some(pool) = cursor.try_next().await? {
            pools.push(pool);
        }

        Ok(pools)
    }

    /// Bulk insert pools if they don't exist yet
    /// Returns (inserted_count, skipped_count)
    pub async fn bulk_insert_if_not_exists(&self, pools: Vec<Pool>) -> Result<(usize, usize)> {
        if pools.is_empty() {
            return Ok((0, 0));
        }

        let collection = self.client.collection::<Pool>("pools");
        let network_id = pools[0].network_id;

        // Collect target addresses as strings
        let target_addresses: Vec<String> = pools.iter().map(|p| p.address.clone()).collect();

        // Find existing pools in one query (including deleted ones for duplicate check)
        let filter = doc! {
            "network_id": network_id as i64,
            "address": { "$in": &target_addresses }
        };
        let mut cursor = collection.find(filter).await?;
        let mut existing: HashSet<String> = HashSet::new();
        while let Some(p) = cursor.try_next().await? {
            existing.insert(p.address);
        }

        // Filter out existing pools
        let to_insert: Vec<Pool> = pools
            .into_iter()
            .filter(|p| !existing.contains(&p.address))
            .collect();

        if to_insert.is_empty() {
            let skipped = target_addresses.len();
            return Ok((0, skipped));
        }

        let result = collection.insert_many(to_insert).await?;
        let inserted = result.inserted_ids.len();
        let skipped = target_addresses.len().saturating_sub(inserted);

        debug!(
            "Bulk inserted {} pools, skipped {} (already exist)",
            inserted, skipped
        );

        Ok((inserted, skipped))
    }

    /// Update pool's updated_at timestamp
    pub async fn touch(&self, network_id: u64, address: &Address) -> Result<bool> {
        let collection = self.client.collection::<Pool>("pools");
        let addr_str = address_to_string(address);

        let filter = doc! {
            "network_id": network_id as i64,
            "address": &addr_str
        };

        let update = doc! {
            "$set": {
                "updated_at": Utc::now().timestamp() as i64
            }
        };

        let result = collection.update_one(filter, update).await?;

        if result.matched_count > 0 {
            debug!("Touched pool {}", addr_str);
            Ok(true)
        } else {
            debug!("Pool {} not found", addr_str);
            Ok(false)
        }
    }

    /// Delete a pool by network_id and address
    pub async fn delete_by_address(&self, network_id: u64, address: &Address) -> Result<bool> {
        let collection = self.client.collection::<Pool>("pools");
        let addr_str = address_to_string(address);

        let filter = doc! {
            "network_id": network_id as i64,
            "address": &addr_str
        };

        let result = collection.delete_one(filter).await?;

        if result.deleted_count > 0 {
            info!("Deleted pool {}", addr_str);
            Ok(true)
        } else {
            debug!("Pool {} not found", addr_str);
            Ok(false)
        }
    }

    /// Count pools for a given network
    pub async fn count_by_network_id(&self, network_id: u64) -> Result<u64> {
        let collection = self.client.collection::<Pool>("pools");
        let filter = doc! {
            "network_id": network_id as i64,
            "$or": [
                { "deleted_at": { "$exists": false } },
                 { "deleted_at": null },
                { "deleted_at": 0 }
            ]
        };
        let count = collection.count_documents(filter).await?;

        Ok(count)
    }

    /// Delete all pools for a given network
    pub async fn delete_by_network_id(&self, network_id: u64) -> Result<u64> {
        let collection = self.client.collection::<Pool>("pools");
        let filter = doc! { "network_id": network_id as i64 };
        let result = collection.delete_many(filter).await?;

        let deleted_count = result.deleted_count;
        if deleted_count > 0 {
            info!("Deleted {} pools for network {}", deleted_count, network_id);
        }

        Ok(deleted_count)
    }

    /// Find all pools (across all networks)
    pub async fn find_all(&self) -> Result<Vec<Pool>> {
        let collection = self.client.collection::<Pool>("pools");
        let filter = doc! {
            "$or": [
                { "deleted_at": { "$exists": false } },
                 { "deleted_at": null },
                { "deleted_at": 0 }
            ]
        };
        let mut cursor = collection.find(filter).await?;
        let mut pools = Vec::new();

        while let Some(pool) = cursor.try_next().await? {
            pools.push(pool);
        }

        Ok(pools)
    }

    /// Find pools updated since the given timestamp
    /// Uses updated_at field, or created_at if updated_at is not set
    pub async fn find_updated_since(&self, since_timestamp: u64) -> Result<Vec<Pool>> {
        let collection = self.client.collection::<Pool>("pools");
        // Find documents where updated_at > since_timestamp OR (updated_at doesn't exist AND created_at > since_timestamp)
        let filter = doc! {
            "$and": [
                {
                    "$or": [
                        { "updated_at": { "$gt": since_timestamp as i64 } },
                        {
                            "$and": [
                                { "updated_at": { "$exists": false } },
                                { "created_at": { "$gt": since_timestamp as i64 } }
                            ]
                        }
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
        let mut pools = Vec::new();

        while let Some(pool) = cursor.try_next().await? {
            pools.push(pool);
        }

        Ok(pools)
    }
}
