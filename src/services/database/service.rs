use alloy::primitives::Address;
use anyhow::Result;
use std::sync::Arc;

use super::models::utils::address_to_string;
use super::models::{Network, Path, Pool, Token};
use super::mongodb::MongoDbClient;
use super::repositories::{NetworkRepository, PathRepository, PoolRepository, TokenRepository};
use crate::config::MongoDbConfig;
use crate::models::path::PoolDirection;
use crate::services::repositories::ConfigRepository;

/// MongoDB service for managing database operations
///
/// This service acts as a thin facade over the repository layer:
/// - Provides convenience methods for common operations (with automatic network_id handling)
/// - Exposes repositories directly for complex queries and advanced operations
/// - Keeps the service file manageable as the database scales
///
/// **Usage Pattern:**
/// - Use service methods for simple, common operations: `service.add_token(...)`
/// - Use repository getters for complex queries: `service.get_token_repo().find_by_network_id(...)`
#[derive(Debug, Clone)]
pub struct MongoDbService {
    _client: Arc<MongoDbClient>,
    network_repo: NetworkRepository,
    token_repo: TokenRepository,
    pool_repo: PoolRepository,
    path_repo: PathRepository,
    config_repo: ConfigRepository,
}

impl MongoDbService {
    /// Create a new MongoDB service
    pub async fn new(config: &MongoDbConfig) -> Result<Self> {
        // Validate configuration
        config.validate()?;
        // Initialize MongoDB client
        let client = MongoDbClient::init(config).await?;

        // Create repositories
        let network_repo = NetworkRepository::new(client.clone());
        let token_repo = TokenRepository::new(client.clone());
        let pool_repo = PoolRepository::new(client.clone());
        let path_repo = PathRepository::new(client.clone());
        let config_repo = ConfigRepository::new(client.clone());

        Ok(Self {
            _client: client,
            network_repo,
            token_repo,
            pool_repo,
            path_repo,
            config_repo,
        })
    }

    // ========== Repository Getters ==========
    // Use these for complex queries and advanced operations

    /// Get network repository for advanced operations
    pub fn get_network_repo(&self) -> &NetworkRepository {
        &self.network_repo
    }

    /// Get token repository for advanced operations
    pub fn get_token_repo(&self) -> &TokenRepository {
        &self.token_repo
    }

    /// Get pool repository for advanced operations
    pub fn get_pool_repo(&self) -> &PoolRepository {
        &self.pool_repo
    }

    /// Get path repository for advanced operations
    pub fn get_path_repo(&self) -> &PathRepository {
        &self.path_repo
    }

    /// Get config repository for advanced operations
    pub fn get_config_repo(&self) -> &ConfigRepository {
        &self.config_repo
    }

    /// Find network by chain ID
    pub async fn find_network(&self, chain_id: u64) -> Result<Option<Network>> {
        self.network_repo.find_by_chain_id(chain_id).await
    }

    /// Update network RPCs
    pub async fn update_network_rpcs(&self, chain_id: u64, rpcs: Vec<String>) -> Result<()> {
        self.network_repo.update_rpcs(chain_id, rpcs).await
    }

    /// Update network websocket URLs
    pub async fn update_network_websocket_urls(
        &self,
        chain_id: u64,
        websocket_urls: Vec<String>,
    ) -> Result<()> {
        self.network_repo
            .update_websocket_urls(chain_id, websocket_urls)
            .await
    }

    // ========== Token Operations ==========

    /// Insert or update a token
    pub async fn add_token(
        &self,
        network_id: u64,
        address: &Address,
        name: Option<String>,
        symbol: Option<String>,
        decimals: Option<u8>,
    ) -> Result<(String, bool)> {
        let token = Token::new(
            network_id,
            address_to_string(address),
            name,
            symbol,
            decimals,
        );

        self.token_repo.insert_or_update(token).await
    }

    /// Find token by address
    pub async fn find_token(&self, network_id: u64, address: &Address) -> Result<Option<Token>> {
        self.token_repo.find_by_address(network_id, address).await
    }

    /// Update token metadata
    pub async fn update_token_metadata(
        &self,
        network_id: u64,
        address: &Address,
        name: Option<String>,
        symbol: Option<String>,
        decimals: Option<u8>,
    ) -> Result<bool> {
        self.token_repo
            .update_metadata(network_id, address, name, symbol, decimals)
            .await
    }

    /// Bulk add tokens (only inserts tokens that do not already exist)
    pub async fn bulk_add_tokens(
        &self,
        network_id: u64,
        tokens: Vec<(Address, Option<String>, Option<String>, Option<u8>)>,
    ) -> Result<(usize, usize)> {
        let token_models: Vec<Token> = tokens
            .into_iter()
            .map(|(addr, name, symbol, decimals)| {
                Token::new(network_id, address_to_string(&addr), name, symbol, decimals)
            })
            .collect();

        self.token_repo.bulk_insert_or_update(token_models).await
    }

    /// Get all tokens for this network
    pub async fn get_all_tokens(&self, network_id: u64) -> Result<Vec<Token>> {
        self.token_repo.find_by_network_id(network_id).await
    }

    /// Count tokens for this network
    pub async fn count_tokens(&self, network_id: u64) -> Result<u64> {
        self.token_repo.count_by_network_id(network_id).await
    }

    // ========== Pool Operations ==========

    /// Add a pool
    pub async fn add_pool(&self, network_id: u64, address: &Address) -> Result<(String, bool)> {
        let pool = Pool::new(network_id, address_to_string(address));
        self.pool_repo.insert_if_not_exists(pool).await
    }

    /// Find pool by address
    pub async fn find_pool(&self, network_id: u64, address: &Address) -> Result<Option<Pool>> {
        self.pool_repo.find_by_address(network_id, address).await
    }

    /// Bulk add pools (only inserts pools that do not already exist)
    pub async fn bulk_add_pools(
        &self,
        network_id: u64,
        addresses: Vec<Address>,
    ) -> Result<(usize, usize)> {
        let pool_models: Vec<Pool> = addresses
            .into_iter()
            .map(|addr| Pool::new(network_id, address_to_string(&addr)))
            .collect();

        self.pool_repo.bulk_insert_if_not_exists(pool_models).await
    }

    /// Get all pools for this network
    pub async fn get_all_pools(&self, network_id: u64) -> Result<Vec<Pool>> {
        self.pool_repo.find_by_network_id(network_id).await
    }

    /// Count pools for this network
    pub async fn count_pools(&self, network_id: u64) -> Result<u64> {
        self.pool_repo.count_by_network_id(network_id).await
    }

    /// Touch a pool (update its timestamp)
    pub async fn touch_pool(&self, network_id: u64, address: &Address) -> Result<bool> {
        self.pool_repo.touch(network_id, address).await
    }

    /// Delete a pool
    pub async fn delete_pool(&self, network_id: u64, address: &Address) -> Result<bool> {
        self.pool_repo.delete_by_address(network_id, address).await
    }

    // ========== Path Operations ==========
    // Note: Path operations are cross-chain, so they don't use network_id automatically
    // Use repository getter for advanced path queries

    /// Add a cross-chain path
    pub async fn add_path(
        &self,
        source_network_id: u64,
        target_network_id: u64,
        source_chain: Vec<PoolDirection>,
        target_chain: Vec<PoolDirection>,
    ) -> Result<Option<bson::oid::ObjectId>> {
        let path = Path::new(
            source_network_id,
            target_network_id,
            source_chain,
            target_chain,
        );

        self.path_repo.insert_if_not_exists(path).await
    }

    /// Find paths between this network and a target network
    pub async fn find_paths_to_network(
        &self,
        source_network_id: u64,
        target_network_id: u64,
    ) -> Result<Vec<Path>> {
        self.path_repo
            .find_by_networks(source_network_id, target_network_id)
            .await
    }

    /// Find all paths from this network
    pub async fn find_all_paths_from_network(&self, source_network_id: u64) -> Result<Vec<Path>> {
        self.path_repo
            .find_by_source_network(source_network_id)
            .await
    }

    /// Find all paths to this network
    pub async fn find_all_paths_to_network(&self, target_network_id: u64) -> Result<Vec<Path>> {
        self.path_repo
            .find_by_target_network(target_network_id)
            .await
    }
}
