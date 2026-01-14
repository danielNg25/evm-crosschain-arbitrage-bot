use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use crate::core::{ErrorLogger, MultichainNetworkRegistry, NetworkRegistry, PoolChange};
use crate::models::profit_token::price_updater::PriceUpdater;
use crate::models::profit_token::ProfitTokenRegistry;
use crate::utils::metrics::Metrics;
use alloy::primitives::Address;
use alloy::providers::{DynProvider, Provider, ProviderBuilder, MULTICALL3_ADDRESS};
use alloy::rpc::client::RpcClient;
use alloy::transports::http::Http;
use alloy::transports::layers::FallbackLayer;
use anyhow::Result;
use chrono::Utc;
use log::{error, info};
use std::num::NonZeroUsize;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{sleep, Duration};
use tower::ServiceBuilder;

use crate::{
    core::PoolUpdaterLatestBlock,
    models::{path::PathRegistry, pool::PoolRegistry, token::TokenRegistry},
    services::database::models::{Network, Path, Pool},
    services::MongoDbService,
};

pub struct NetworkCollector {
    pub mongodb_service: Arc<MongoDbService>,
    pub network_registries: Arc<MultichainNetworkRegistry>,
    pub metrics: Arc<RwLock<Metrics>>,
    pub swap_event_tx: mpsc::Sender<PoolChange>,
    pub error_loggers: Arc<Mutex<Vec<Arc<dyn ErrorLogger>>>>,
}

impl NetworkCollector {
    pub fn new(
        mongodb_service: Arc<MongoDbService>,
        metrics: Arc<RwLock<Metrics>>,
        swap_event_tx: mpsc::Sender<PoolChange>,
        error_loggers: Vec<Arc<dyn ErrorLogger>>,
    ) -> Self {
        Self {
            mongodb_service,
            network_registries: Arc::new(MultichainNetworkRegistry::new()),
            metrics,
            swap_event_tx,
            error_loggers: Arc::new(Mutex::new(error_loggers)),
        }
    }

    pub fn add_network(&self, network_id: u64, network_registry: NetworkRegistry) -> Result<()> {
        self.network_registries
            .network_registries
            .insert(network_id, Arc::new(RwLock::new(network_registry)));
        Ok(())
    }

    pub async fn add_error_logger(&self, error_logger: Arc<dyn ErrorLogger>) {
        info!("Adding error logger");
        self.error_loggers.lock().await.push(error_logger);
    }

    /// Start polling MongoDB for changes in networks, pools, and paths collections
    /// Uses incremental sync - only fetches documents updated since last sync
    /// Polls in order: networks -> pools -> paths
    pub fn start_mongodb_polling(&self, poll_interval_secs: u64) {
        let mongodb_service = Arc::clone(&self.mongodb_service);
        let network_registries = Arc::clone(&self.network_registries);
        let metrics = Arc::clone(&self.metrics);
        let swap_event_tx = self.swap_event_tx.clone();
        let error_loggers = Arc::clone(&self.error_loggers);
        tokio::spawn(async move {
            let poll_duration = Duration::from_secs(poll_interval_secs);

            // Track last sync timestamps for each collection
            let mut last_network_sync: Option<u64> = None;
            let mut last_pool_sync: Option<u64> = None;
            let mut last_path_sync: Option<u64> = None;

            info!(
                "Starting MongoDB incremental polling with interval: {} seconds",
                poll_interval_secs
            );

            loop {
                info!("Polling ...");
                // Poll networks (incremental)
                match Self::fetch_networks_since(&mongodb_service, last_network_sync).await {
                    Ok(networks) => {
                        if !networks.is_empty() {
                            info!(
                                "Fetched {} new/updated networks from MongoDB",
                                networks.len()
                            );
                            if let Err(e) = Self::handle_networks_update(
                                networks,
                                &network_registries,
                                &metrics,
                                &swap_event_tx,
                                &error_loggers,
                            )
                            .await
                            {
                                for logger in error_loggers.lock().await.iter() {
                                    let logger = Arc::clone(logger);
                                    let error_msg = e.to_string();
                                    tokio::spawn(async move {
                                        if let Err(e) = logger.log_error(&error_msg).await {
                                            error!("Error logging error: {:?}", e);
                                        }
                                    });
                                }
                                error!("Failed to handle networks update: {}", e);
                            } else {
                                // Update timestamp after successful sync
                                last_network_sync = Some(Utc::now().timestamp() as u64);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch networks from MongoDB: {}", e);
                    }
                }

                // Poll pools (incremental)
                match Self::fetch_pools_since(&mongodb_service, last_pool_sync).await {
                    Ok(pools) => {
                        if !pools.is_empty() {
                            info!("Fetched {} new/updated pools from MongoDB", pools.len());
                            if let Err(e) =
                                Self::handle_pools_update(pools, &network_registries).await
                            {
                                for logger in error_loggers.lock().await.iter() {
                                    let logger = Arc::clone(logger);
                                    let error_msg = e.to_string();
                                    tokio::spawn(async move {
                                        if let Err(e) = logger.log_error(&error_msg).await {
                                            error!("Error logging error: {:?}", e);
                                        }
                                    });
                                }
                                error!("Failed to handle pools update: {}", e);
                            } else {
                                // Update timestamp after successful sync
                                last_pool_sync = Some(Utc::now().timestamp() as u64);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch pools from MongoDB: {}", e);
                    }
                }

                // Poll paths (incremental)
                match Self::fetch_paths_since(&mongodb_service, last_path_sync).await {
                    Ok(paths) => {
                        if !paths.is_empty() {
                            info!("Fetched {} new/updated paths from MongoDB", paths.len());
                            if let Err(e) =
                                Self::handle_paths_update(paths, &network_registries).await
                            {
                                for logger in error_loggers.lock().await.iter() {
                                    let logger = Arc::clone(logger);
                                    let error_msg = e.to_string();
                                    tokio::spawn(async move {
                                        if let Err(e) = logger.log_error(&error_msg).await {
                                            error!("Error logging error: {:?}", e);
                                        }
                                    });
                                }
                                error!("Failed to handle paths update: {}", e);
                            } else {
                                // Update timestamp after successful sync
                                last_path_sync = Some(Utc::now().timestamp() as u64);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch paths from MongoDB: {}", e);
                    }
                }

                info!(
                    "last_network_sync: {:?}, last_pool_sync: {:?}, last_path_sync: {:?}",
                    last_network_sync, last_pool_sync, last_path_sync
                );

                sleep(poll_duration).await;
            }
        });
    }

    /// Fetch networks updated since the given timestamp
    /// If timestamp is None, fetches all networks (initial sync)
    async fn fetch_networks_since(
        mongodb_service: &Arc<MongoDbService>,
        since_timestamp: Option<u64>,
    ) -> Result<Vec<Network>> {
        match since_timestamp {
            Some(ts) => {
                mongodb_service
                    .get_network_repo()
                    .find_updated_since(ts)
                    .await
            }
            None => {
                // Initial sync - fetch all
                mongodb_service.get_network_repo().find_all().await
            }
        }
    }

    /// Fetch pools updated since the given timestamp
    /// If timestamp is None, fetches all pools (initial sync)
    async fn fetch_pools_since(
        mongodb_service: &Arc<MongoDbService>,
        since_timestamp: Option<u64>,
    ) -> Result<Vec<Pool>> {
        match since_timestamp {
            Some(ts) => mongodb_service.get_pool_repo().find_updated_since(ts).await,
            None => {
                // Initial sync - fetch all
                mongodb_service.get_pool_repo().find_all().await
            }
        }
    }

    /// Fetch paths updated since the given timestamp
    /// If timestamp is None, fetches all paths (initial sync)
    async fn fetch_paths_since(
        mongodb_service: &Arc<MongoDbService>,
        since_timestamp: Option<u64>,
    ) -> Result<Vec<Path>> {
        match since_timestamp {
            Some(ts) => mongodb_service.get_path_repo().find_updated_since(ts).await,
            None => {
                // Initial sync - fetch all
                mongodb_service.get_path_repo().find_all().await
            }
        }
    }

    /// Handle networks update - PLACEHOLDER for implementation
    async fn handle_networks_update(
        networks: Vec<Network>,
        network_registries: &Arc<MultichainNetworkRegistry>,
        metrics: &Arc<RwLock<Metrics>>,
        swap_event_tx: &mpsc::Sender<PoolChange>,
        error_loggers: &Arc<Mutex<Vec<Arc<dyn ErrorLogger>>>>,
    ) -> Result<()> {
        let network_len = networks.len();
        for network in networks {
            let network_id = network.chain_id;
            let network_name = network.name.clone();
            if !network_registries
                .network_registries
                .contains_key(&network.chain_id)
            {
                let provider = create_provider(network.rpcs.clone());
                let chain_id = provider.get_chain_id().await.unwrap();
                if chain_id != network_id {
                    error!("Chain ID mismatch for network {}", network_id);
                    return Err(anyhow::anyhow!(
                        "Chain ID mismatch for network {}",
                        network_id
                    ));
                }
                let latest_block = provider.get_block_number().await.unwrap();

                let pool_registry =
                    Arc::new(RwLock::new(PoolRegistry::new(provider.clone(), network_id)));
                if let Some(aero_factory_addresses) = network.aero_factory_addresses.clone() {
                    let aero_factory_addresses_parsed: Vec<Address> = aero_factory_addresses
                        .into_iter()
                        .map(|a| {
                            Address::from_str(&a).map_err(|e| {
                                anyhow::anyhow!("Failed to parse address '{}': {}", a, e)
                            })
                        })
                        .collect::<Result<Vec<Address>, _>>()
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to parse aero factory addresses: {}", e)
                        })?;
                    pool_registry
                        .read()
                        .await
                        .set_aero_factory_addresses(aero_factory_addresses_parsed)
                        .await;
                }

                if let Some(v2_factory_to_fee) = network.v2_factory_to_fee.clone() {
                    pool_registry
                        .read()
                        .await
                        .set_factory_to_fee(v2_factory_to_fee)
                        .await;
                }

                let token_registry = Arc::new(RwLock::new(TokenRegistry::new(network_id)));
                let path_registry = Arc::new(RwLock::new(PathRegistry::new(network_id)));
                let price_updater =
                    Arc::new(RwLock::new(PriceUpdater::new(network.name, vec![]).await));

                let profit_token_registry = Arc::new(RwLock::new(ProfitTokenRegistry::new(
                    Address::from_str(&network.wrap_native).map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to parse wrap native '{}': {}",
                            network.wrap_native,
                            e
                        )
                    })?,
                    token_registry.clone(),
                    price_updater.clone(),
                    network.min_profit_usd,
                )));
                let pool_updater = Arc::new(RwLock::new(
                    PoolUpdaterLatestBlock::new(
                        Arc::new(provider.clone()),
                        pool_registry.clone(),
                        token_registry.clone(),
                        network
                            .multicall_address
                            .as_ref()
                            .map(|s| s.parse().expect("Invalid multicall_address"))
                            .unwrap_or(MULTICALL3_ADDRESS),
                        metrics.clone(),
                        swap_event_tx.clone(),
                        latest_block,
                        network.max_blocks_per_batch,
                        network.wait_time_fetch,
                        network_id,
                        error_loggers.lock().await.clone(),
                    )
                    .await,
                ));

                let network_registry = NetworkRegistry::new(
                    network_id,
                    network_name,
                    network.rpcs.clone(),
                    Arc::new(provider.clone().erased()),
                    pool_registry,
                    token_registry,
                    path_registry,
                    pool_updater.clone(),
                    profit_token_registry.clone(),
                );
                network_registries
                    .network_registries
                    .insert(network_id, Arc::new(RwLock::new(network_registry)));
                tokio::spawn(async move {
                    info!("Starting pool updater for network {}", network_id);
                    if let Err(e) = pool_updater.read().await.start(latest_block, vec![]).await {
                        error!("Error starting pool updater: {}", e);
                    }
                });
            } else {
                // Checking changes in the network
                let network_registry = network_registries
                    .get_network_registry(network_id)
                    .await
                    .ok_or_else(|| {
                        error!("Network registry not found for network {}", network_id);
                        anyhow::anyhow!("Network registry not found for network {}", network_id)
                    })?;

                let result = network_registry.write().await.update_network(network).await;
                if let Err(e) = result {
                    for logger in error_loggers.lock().await.iter() {
                        let logger = Arc::clone(logger);
                        let error_msg = e.to_string();
                        tokio::spawn(async move {
                            if let Err(e) = logger.log_error(&error_msg).await {
                                error!("Error logging error: {:?}", e);
                            }
                        });
                    }
                    error!("Failed to update network: {}", e);
                }
            }
        }
        info!("Handled {} networks update", network_len);
        Ok(())
    }

    /// Handle pools update - PLACEHOLDER for implementation
    async fn handle_pools_update(
        pools: Vec<Pool>,
        network_registries: &Arc<MultichainNetworkRegistry>,
    ) -> Result<()> {
        let mut network_to_pools = HashMap::new();
        for pool in pools {
            network_to_pools
                .entry(pool.network_id)
                .or_insert(Vec::new())
                .push(pool);
        }
        for (network_id, pools) in network_to_pools {
            info!(
                "Adding {} pools to pending new pools for network {}",
                pools.len(),
                network_id
            );
            let network_registry = network_registries
                .get_network_registry(network_id)
                .await
                .ok_or_else(|| {
                    error!("Network registry not found for network {}", network_id);
                    anyhow::anyhow!("Network registry not found for network {}", network_id)
                })?;
            network_registry
                .read()
                .await
                .pool_updater
                .read()
                .await
                .add_pending_new_pools(
                    pools
                        .into_iter()
                        .map(|p| {
                            Address::from_str(&p.address).map_err(|e| {
                                anyhow::anyhow!(
                                    "Failed to parse pool address '{}': {}",
                                    p.address,
                                    e
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| anyhow::anyhow!("Failed to parse pool addresses: {}", e))?,
                )
                .await?;
        }

        Ok(())
    }

    /// Handle paths update - PLACEHOLDER for implementation
    async fn handle_paths_update(
        paths: Vec<Path>,
        network_registries: &Arc<MultichainNetworkRegistry>,
    ) -> Result<()> {
        for path in paths {
            if let Err(e) = network_registries.set_paths(path.paths).await {
                return Err(e);
            }
        }
        Ok(())
    }
}

pub fn create_provider(rpcs: Vec<String>) -> DynProvider {
    let rpc_len = rpcs.len();
    let fallback_layer =
        FallbackLayer::default().with_active_transport_count(NonZeroUsize::new(rpc_len).unwrap());

    // Define your list of transports to use
    let transports = rpcs
        .iter()
        .map(|url| Http::new(url.parse().unwrap()))
        .collect::<Vec<_>>();

    // Apply the FallbackLayer to the transports
    let transport = ServiceBuilder::new()
        .layer(fallback_layer)
        .service(transports);
    let client = RpcClient::builder().transport(transport, false);
    let provider = ProviderBuilder::new().connect_client(client.clone());
    provider.clone().erased()
}
