use crate::services::database::models::token::Token;
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

/// Token repository for MongoDB operations
#[derive(Debug, Clone)]
pub struct TokenRepository {
    client: Arc<MongoDbClient>,
}

impl TokenRepository {
    /// Create a new TokenRepository instance
    pub fn new(client: Arc<MongoDbClient>) -> Self {
        Self { client }
    }

    /// Insert a token if it doesn't exist, or update if it exists
    pub async fn insert_or_update(&self, token: Token) -> Result<(String, bool)> {
        let collection = self.client.collection::<Token>("tokens");
        let addr_str = token.address.clone();

        // Check if token already exists
        let filter = doc! {
            "network_id": token.network_id as i64,
            "address": &addr_str
        };

        let existing = collection.find_one(filter.clone()).await?;

        if let Some(existing_token) = existing {
            // Update token with new information if provided
            let mut update_doc = doc! {};
            let mut needs_update = false;

            if let Some(name) = &token.name {
                if existing_token.name.as_ref() != Some(name) {
                    update_doc.insert("name", name);
                    needs_update = true;
                }
            }

            if let Some(symbol) = &token.symbol {
                if existing_token.symbol.as_ref() != Some(symbol) {
                    update_doc.insert("symbol", symbol);
                    needs_update = true;
                }
            }

            if let Some(decimals) = token.decimals {
                if existing_token.decimals != Some(decimals) {
                    update_doc.insert("decimals", decimals as i32);
                    needs_update = true;
                }
            }

            if needs_update {
                update_doc.insert("updated_at", Utc::now().timestamp() as i64);
                let update = doc! { "$set": update_doc };
                collection.update_one(filter, update).await?;
                debug!("Updated token {}", addr_str);
                return Ok((addr_str, true));
            }

            return Ok((addr_str, false));
        }

        // Insert new token
        collection.insert_one(token.clone()).await?;
        info!("Inserted new token: {}", addr_str);

        Ok((addr_str, true))
    }

    /// Find a token by network_id and address
    pub async fn find_by_address(
        &self,
        network_id: u64,
        address: &Address,
    ) -> Result<Option<Token>> {
        let collection = self.client.collection::<Token>("tokens");
        let addr_str = address_to_string(address);

        let filter = doc! {
            "network_id": network_id as i64,
            "address": &addr_str
        };

        let token = collection.find_one(filter).await?;
        Ok(token)
    }

    /// Find all tokens for a given network
    pub async fn find_by_network_id(&self, network_id: u64) -> Result<Vec<Token>> {
        let collection = self.client.collection::<Token>("tokens");
        let filter = doc! { "network_id": network_id as i64 };
        let mut cursor = collection.find(filter).await?;
        let mut tokens = Vec::new();

        while let Some(token) = cursor.try_next().await? {
            tokens.push(token);
        }

        Ok(tokens)
    }

    /// Bulk insert tokens if they don't exist yet
    /// Returns (inserted_count, updated_count)
    pub async fn bulk_insert_or_update(&self, tokens: Vec<Token>) -> Result<(usize, usize)> {
        if tokens.is_empty() {
            return Ok((0, 0));
        }

        let collection = self.client.collection::<Token>("tokens");
        let network_id = tokens[0].network_id;

        // Collect target addresses as strings
        let target_addresses: Vec<String> = tokens.iter().map(|t| t.address.clone()).collect();

        // Find existing tokens in one query
        let filter = doc! {
            "network_id": network_id as i64,
            "address": { "$in": &target_addresses }
        };
        let mut cursor = collection.find(filter).await?;
        let mut existing: HashSet<String> = HashSet::new();
        while let Some(t) = cursor.try_next().await? {
            existing.insert(t.address);
        }

        // Separate tokens into insert and update batches
        let mut to_insert = Vec::new();
        let mut to_update = Vec::new();

        for token in tokens {
            if existing.contains(&token.address) {
                to_update.push(token);
            } else {
                to_insert.push(token);
            }
        }

        let mut inserted = 0;
        let mut updated = 0;

        // Insert new tokens
        if !to_insert.is_empty() {
            let result = collection.insert_many(to_insert).await?;
            inserted = result.inserted_ids.len();
        }

        // Update existing tokens
        for token in to_update {
            let filter = doc! {
                "network_id": token.network_id as i64,
                "address": &token.address
            };

            let mut update_doc = bson::Document::new();
            update_doc.insert("updated_at", Utc::now().timestamp() as i64);

            if let Some(name) = &token.name {
                update_doc.insert("name", name);
            }
            if let Some(symbol) = &token.symbol {
                update_doc.insert("symbol", symbol);
            }
            if let Some(decimals) = token.decimals {
                update_doc.insert("decimals", decimals as i32);
            }

            let update = doc! { "$set": update_doc };
            collection.update_one(filter, update).await?;
            updated += 1;
        }

        debug!(
            "Bulk operation: inserted {} tokens, updated {} tokens",
            inserted, updated
        );

        Ok((inserted, updated))
    }

    /// Update token metadata (name, symbol, decimals)
    pub async fn update_metadata(
        &self,
        network_id: u64,
        address: &Address,
        name: Option<String>,
        symbol: Option<String>,
        decimals: Option<u8>,
    ) -> Result<bool> {
        let collection = self.client.collection::<Token>("tokens");
        let addr_str = address_to_string(address);

        let filter = doc! {
            "network_id": network_id as i64,
            "address": &addr_str
        };

        let mut update_doc = bson::Document::new();
        update_doc.insert("updated_at", Utc::now().timestamp() as i64);

        if let Some(name) = name {
            update_doc.insert("name", name);
        }
        if let Some(symbol) = symbol {
            update_doc.insert("symbol", symbol);
        }
        if let Some(decimals) = decimals {
            update_doc.insert("decimals", decimals as i32);
        }

        let update = doc! { "$set": update_doc };
        let result = collection.update_one(filter, update).await?;

        if result.matched_count > 0 {
            debug!("Updated metadata for token {}", addr_str);
            Ok(true)
        } else {
            debug!("Token {} not found", addr_str);
            Ok(false)
        }
    }

    /// Delete a token by network_id and address
    pub async fn delete_by_address(&self, network_id: u64, address: &Address) -> Result<bool> {
        let collection = self.client.collection::<Token>("tokens");
        let addr_str = address_to_string(address);

        let filter = doc! {
            "network_id": network_id as i64,
            "address": &addr_str
        };

        let result = collection.delete_one(filter).await?;

        if result.deleted_count > 0 {
            info!("Deleted token {}", addr_str);
            Ok(true)
        } else {
            debug!("Token {} not found", addr_str);
            Ok(false)
        }
    }

    /// Count tokens for a given network
    pub async fn count_by_network_id(&self, network_id: u64) -> Result<u64> {
        let collection = self.client.collection::<Token>("tokens");
        let filter = doc! { "network_id": network_id as i64 };
        let count = collection.count_documents(filter).await?;

        Ok(count)
    }
}
