use crate::models::path::PoolDirection;
use crate::services::database::models::utils::address_to_string;
use crate::services::database::models::Path;
use crate::services::database::mongodb::MongoDbClient;
use alloy::primitives::Address;
use anyhow::Result;
use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use log::{debug, info};
use std::sync::Arc;

/// Path repository for MongoDB operations
#[derive(Debug, Clone)]
pub struct PathRepository {
    client: Arc<MongoDbClient>,
}

impl PathRepository {
    /// Create a new PathRepository instance
    pub fn new(client: Arc<MongoDbClient>) -> Self {
        Self { client }
    }

    /// Insert a path if it doesn't exist
    pub async fn insert_if_not_exists(&self, path: Path) -> Result<Option<bson::oid::ObjectId>> {
        let collection = self.client.collection::<Path>("paths");

        // Check if a similar path already exists (including deleted ones for duplicate check)
        // We consider paths with same source/target networks and same chains as duplicates
        let filter = doc! {
            "paths": bson::to_bson(&path.paths)?
        };

        let existing = collection.find_one(filter).await?;

        if let Some(existing_path) = existing {
            debug!("Path already exists");
            return Ok(existing_path.id);
        }

        // Insert new path
        let result = collection.insert_one(path.clone()).await?;
        let id = result.inserted_id.as_object_id();
        info!("Inserted new path (id: {:?})", id);

        Ok(id)
    }

    /// Find a path by ID
    pub async fn find_by_id(&self, id: &bson::oid::ObjectId) -> Result<Option<Path>> {
        let collection = self.client.collection::<Path>("paths");
        let filter = doc! { "_id": id };
        let path = collection.find_one(filter).await?;

        Ok(path)
    }

    /// Find all paths
    pub async fn find_all(&self) -> Result<Vec<Path>> {
        let collection = self.client.collection::<Path>("paths");
        let filter = doc! {
            "$or": [
                { "deleted_at": { "$exists": false } },
                 { "deleted_at": null },
                { "deleted_at": 0 }
            ]
        };
        let paths = collection
            .find(filter)
            .await?
            .try_collect::<Vec<Path>>()
            .await?;

        Ok(paths)
    }

    pub async fn find_by_anchor_token(&self, anchor_token: &Address) -> Result<Vec<Path>> {
        let collection = self.client.collection::<Path>("paths");
        let anchor_token_str = address_to_string(anchor_token);
        let filter = doc! {
            "$and": [
                { "paths": { "$elemMatch": { "anchor_token": &anchor_token_str } } },
                {
                    "$or": [
                        { "deleted_at": { "$exists": false } },
                         { "deleted_at": null },
                        { "deleted_at": 0 }
                    ]
                }
            ]
        };
        let paths = collection
            .find(filter)
            .await?
            .try_collect::<Vec<Path>>()
            .await?;

        Ok(paths)
    }

    pub async fn find_by_chain_id(&self, chain_id: u64) -> Result<Vec<Path>> {
        let collection = self.client.collection::<Path>("paths");
        let filter = doc! {
            "$and": [
                { "paths": { "$elemMatch": { "chain_id": chain_id as i64 } } },
                {
                    "$or": [
                        { "deleted_at": { "$exists": false } },
                         { "deleted_at": null },
                        { "deleted_at": 0 }
                    ]
                }
            ]
        };
        let paths = collection
            .find(filter)
            .await?
            .try_collect::<Vec<Path>>()
            .await?;

        Ok(paths)
    }

    /// Find paths by anchor token and chain ID
    pub async fn find_by_anchor_token_and_chain_id(
        &self,
        anchor_token: &Address,
        chain_id: u64,
    ) -> Result<Vec<Path>> {
        let collection = self.client.collection::<Path>("paths");
        let anchor_token_str = address_to_string(anchor_token);
        let filter = doc! {
            "$and": [
                {
                    "paths": {
                        "$elemMatch": {
                            "anchor_token": &anchor_token_str,
                            "chain_id": chain_id as i64
                        }
                    }
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
        let paths = collection
            .find(filter)
            .await?
            .try_collect::<Vec<Path>>()
            .await?;

        Ok(paths)
    }

    /// Update path chains
    pub async fn update_chains(
        &self,
        id: &bson::oid::ObjectId,
        source_chain: Option<Vec<PoolDirection>>,
        target_chain: Option<Vec<PoolDirection>>,
    ) -> Result<bool> {
        let collection = self.client.collection::<Path>("paths");
        let filter = doc! { "_id": id };

        let mut update_doc = bson::Document::new();
        update_doc.insert("updated_at", Utc::now().timestamp() as i64);

        if let Some(source_chain) = source_chain {
            update_doc.insert("source_chain", bson::to_bson(&source_chain)?);
        }

        if let Some(target_chain) = target_chain {
            update_doc.insert("target_chain", bson::to_bson(&target_chain)?);
        }

        let update = doc! { "$set": update_doc };
        let result = collection.update_one(filter, update).await?;

        if result.matched_count > 0 {
            debug!("Updated path {}", id);
            Ok(true)
        } else {
            debug!("Path {} not found", id);
            Ok(false)
        }
    }

    /// Update path's updated_at timestamp
    pub async fn touch(&self, id: &bson::oid::ObjectId) -> Result<bool> {
        let collection = self.client.collection::<Path>("paths");
        let filter = doc! { "_id": id };
        let update = doc! {
            "$set": {
                "updated_at": Utc::now().timestamp() as i64
            }
        };

        let result = collection.update_one(filter, update).await?;

        if result.matched_count > 0 {
            debug!("Touched path {}", id);
            Ok(true)
        } else {
            debug!("Path {} not found", id);
            Ok(false)
        }
    }

    /// Delete a path by ID
    pub async fn delete_by_id(&self, id: &bson::oid::ObjectId) -> Result<bool> {
        let collection = self.client.collection::<Path>("paths");
        let filter = doc! { "_id": id };
        let result = collection.delete_one(filter).await?;

        if result.deleted_count > 0 {
            info!("Deleted path {}", id);
            Ok(true)
        } else {
            debug!("Path {} not found", id);
            Ok(false)
        }
    }

    /// Delete all paths for a given source network
    pub async fn delete_by_source_network(&self, source_network_id: u64) -> Result<u64> {
        let collection = self.client.collection::<Path>("paths");
        let filter = doc! { "source_network_id": source_network_id as i64 };
        let result = collection.delete_many(filter).await?;

        let deleted_count = result.deleted_count;
        if deleted_count > 0 {
            info!(
                "Deleted {} paths for source network {}",
                deleted_count, source_network_id
            );
        }

        Ok(deleted_count)
    }

    /// Delete all paths for a given target network
    pub async fn delete_by_target_network(&self, target_network_id: u64) -> Result<u64> {
        let collection = self.client.collection::<Path>("paths");
        let filter = doc! { "target_network_id": target_network_id as i64 };
        let result = collection.delete_many(filter).await?;

        let deleted_count = result.deleted_count;
        if deleted_count > 0 {
            info!(
                "Deleted {} paths for target network {}",
                deleted_count, target_network_id
            );
        }

        Ok(deleted_count)
    }

    /// Count paths between two networks
    pub async fn count_by_networks(
        &self,
        source_network_id: u64,
        target_network_id: u64,
    ) -> Result<u64> {
        let collection = self.client.collection::<Path>("paths");
        let filter = doc! {
            "$and": [
                {
                    "source_network_id": source_network_id as i64,
                    "target_network_id": target_network_id as i64
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
        let count = collection.count_documents(filter).await?;

        Ok(count)
    }

    /// Find paths updated since the given timestamp
    /// Uses updated_at field, or created_at if updated_at is not set
    pub async fn find_updated_since(&self, since_timestamp: u64) -> Result<Vec<Path>> {
        let collection = self.client.collection::<Path>("paths");
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
        let mut paths = Vec::new();

        while let Some(path) = cursor.try_next().await? {
            paths.push(path);
        }

        Ok(paths)
    }
}
