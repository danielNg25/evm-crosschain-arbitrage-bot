use crate::config::MongoDbConfig;
use anyhow::{anyhow, Result};
use log::{error, info};
use mongodb::{
    bson::doc,
    options::{ClientOptions, IndexOptions, ServerApi, ServerApiVersion},
    Client, Collection, Database as MongoDatabase, IndexModel,
};
use std::sync::Arc;

/// MongoDB client wrapper for managing database connections and operations
#[derive(Debug, Clone)]
pub struct MongoDbClient {
    _client: Client,
    database: MongoDatabase,
}

impl MongoDbClient {
    /// Initialize the MongoDB client with configuration
    pub async fn init(config: &MongoDbConfig) -> Result<Arc<Self>> {
        // Get connection string from config or use default
        let connection_string = config.uri.clone();

        // Get database name from config or use default
        let database_name = config.database.clone();

        info!(
            "Connecting to MongoDB at {} with database {}",
            connection_string, database_name
        );

        // Create client options
        let mut client_options = ClientOptions::parse(&connection_string)
            .await
            .map_err(|e| anyhow!("Failed to parse MongoDB connection string: {}", e))?;

        // Set server API version if using MongoDB Atlas
        let server_api = ServerApi::builder().version(ServerApiVersion::V1).build();
        client_options.server_api = Some(server_api);

        // Create client
        let client = Client::with_options(client_options)
            .map_err(|e| anyhow!("Failed to create MongoDB client: {}", e))?;

        // Get database
        let database = client.database(&database_name);

        // Test connection
        match database.run_command(doc! { "ping": 1 }).await {
            Ok(_) => info!(
                "Successfully connected to MongoDB database: {}",
                database_name
            ),
            Err(e) => {
                error!("Failed to connect to MongoDB: {}", e);
                return Err(anyhow!("Failed to connect to MongoDB: {}", e));
            }
        }

        let db_client = Arc::new(Self {
            _client: client,
            database,
        });

        db_client.create_indexes().await?;

        Ok(db_client)
    }

    /// Get a collection with the given name
    pub fn collection<T: Send + Sync>(&self, name: &str) -> Collection<T> {
        self.database.collection(name)
    }

    /// Get database reference
    pub fn database(&self) -> &MongoDatabase {
        &self.database
    }

    /// Create required indexes for all collections
    pub async fn create_indexes(&self) -> Result<()> {
        info!("Creating MongoDB indexes...");

        // Network indexes
        self.create_network_indexes().await?;

        // Token indexes
        self.create_token_indexes().await?;

        // Pool indexes
        self.create_pool_indexes().await?;

        // Path indexes
        self.create_path_indexes().await?;

        // Opportunity indexes
        self.create_opportunity_indexes().await?;

        info!("MongoDB indexes created successfully");
        Ok(())
    }

    /// Create indexes for networks collection
    async fn create_network_indexes(&self) -> Result<()> {
        let collection = self
            .database
            .collection::<mongodb::bson::Document>("networks");

        // Unique index on chain_id
        let chain_id_index = IndexModel::builder()
            .keys(doc! { "chain_id": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();

        collection
            .create_index(chain_id_index)
            .await
            .map_err(|e| anyhow!("Failed to create network chain_id index: {}", e))?;

        // Index on name for fast lookups
        let name_index = IndexModel::builder().keys(doc! { "name": 1 }).build();

        collection
            .create_index(name_index)
            .await
            .map_err(|e| anyhow!("Failed to create network name index: {}", e))?;

        Ok(())
    }

    /// Create indexes for tokens collection
    async fn create_token_indexes(&self) -> Result<()> {
        let collection = self
            .database
            .collection::<mongodb::bson::Document>("tokens");

        let indexes = [
            // Compound unique index for network_id + address
            (
                doc! { "network_id": 1, "address": 1 },
                IndexOptions::builder().unique(true).build(),
            ),
            // Index on symbol for fast searches
            (doc! { "symbol": 1 }, IndexOptions::default()),
            // Index on network_id for filtering
            (doc! { "network_id": 1 }, IndexOptions::default()),
        ];

        for (keys, options) in indexes {
            let index = IndexModel::builder().keys(keys).options(options).build();
            collection
                .create_index(index)
                .await
                .map_err(|e| anyhow!("Failed to create token index: {}", e))?;
        }

        Ok(())
    }

    /// Create indexes for pools collection
    async fn create_pool_indexes(&self) -> Result<()> {
        let collection = self.database.collection::<mongodb::bson::Document>("pools");

        // Compound unique index for network_id + address
        let unique_index = IndexModel::builder()
            .keys(doc! { "network_id": 1, "address": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();

        collection
            .create_index(unique_index)
            .await
            .map_err(|e| anyhow!("Failed to create pool unique index: {}", e))?;

        // Index on network_id for filtering
        let network_index = IndexModel::builder().keys(doc! { "network_id": 1 }).build();

        collection
            .create_index(network_index)
            .await
            .map_err(|e| anyhow!("Failed to create pool network_id index: {}", e))?;

        Ok(())
    }

    /// Create indexes for paths collection
    async fn create_path_indexes(&self) -> Result<()> {
        let collection = self.database.collection::<mongodb::bson::Document>("paths");

        let indexes = [
            // Index on source_network_id for filtering
            (doc! { "source_network_id": 1 }, IndexOptions::default()),
            // Index on target_network_id for filtering
            (doc! { "target_network_id": 1 }, IndexOptions::default()),
            // Compound index for source + target network lookups
            (
                doc! { "source_network_id": 1, "target_network_id": 1 },
                IndexOptions::default(),
            ),
            // Index on created_at for chronological queries
            (doc! { "created_at": -1 }, IndexOptions::default()),
        ];

        for (keys, options) in indexes {
            let index = IndexModel::builder().keys(keys).options(options).build();
            collection
                .create_index(index)
                .await
                .map_err(|e| anyhow!("Failed to create path index: {}", e))?;
        }

        Ok(())
    }

    /// Create indexes for opportunities collection
    async fn create_opportunity_indexes(&self) -> Result<()> {
        let collection = self
            .database
            .collection::<mongodb::bson::Document>("opportunities");

        let indexes = [
            // Index on network_id for filtering
            (doc! { "network_id": 1 }, IndexOptions::default()),
            // Index on status for filtering
            (doc! { "status": 1 }, IndexOptions::default()),
            // Index on profit_token for token-based queries
            (doc! { "profit_token": 1 }, IndexOptions::default()),
            // Index on created_at for chronological ordering (newest first)
            (doc! { "created_at": -1 }, IndexOptions::default()),
            // Index on execute_block_number for block-based queries
            (doc! { "execute_block_number": 1 }, IndexOptions::default()),
            // Index on profit_usd for profit ranking (highest first)
            (doc! { "profit_usd": -1 }, IndexOptions::default()),
            // Index on source_pool for pool-based queries
            (doc! { "source_pool": 1 }, IndexOptions::default()),
            // Compound index for network + status queries
            (
                doc! { "network_id": 1, "status": 1 },
                IndexOptions::default(),
            ),
        ];

        for (keys, options) in indexes {
            let index = IndexModel::builder().keys(keys).options(options).build();
            collection
                .create_index(index)
                .await
                .map_err(|e| anyhow!("Failed to create opportunity index: {}", e))?;
        }

        Ok(())
    }
}
