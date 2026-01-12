use std::str::FromStr;
use std::sync::Arc;

use crate::core::{MultichainNetworkRegistry, NetworkRegistry, PoolChange};
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
use log::{error, info, warn};
use std::num::NonZeroUsize;
use tokio::sync::{mpsc, RwLock};
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
}

impl NetworkCollector {
    pub fn new(
        mongodb_service: Arc<MongoDbService>,
        metrics: Arc<RwLock<Metrics>>,
        swap_event_tx: mpsc::Sender<PoolChange>,
    ) -> Self {
        Self {
            mongodb_service,
            network_registries: Arc::new(MultichainNetworkRegistry::new()),
            metrics,
            swap_event_tx,
        }
    }

    pub fn add_network(&self, network_id: u64, network_registry: NetworkRegistry) -> Result<()> {
        self.network_registries
            .network_registries
            .insert(network_id, Arc::new(RwLock::new(network_registry)));
        Ok(())
    }

    /// Start polling MongoDB for changes in networks, pools, and paths collections
    /// Uses incremental sync - only fetches documents updated since last sync
    /// Polls in order: networks -> pools -> paths
    pub fn start_mongodb_polling(&self, poll_interval_secs: u64) {
        let mongodb_service = Arc::clone(&self.mongodb_service);
        let network_registries = Arc::clone(&self.network_registries);
        let metrics = Arc::clone(&self.metrics);
        let swap_event_tx = self.swap_event_tx.clone();

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
                                &mongodb_service,
                                &metrics,
                                &swap_event_tx,
                            )
                            .await
                            {
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
                            Self::handle_pools_update(pools, &network_registries).await;
                        }
                        // Update timestamp after successful sync
                        last_pool_sync = Some(Utc::now().timestamp() as u64);
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
                            Self::handle_paths_update(paths, &network_registries).await;
                        }
                        // Update timestamp after successful sync
                        last_path_sync = Some(Utc::now().timestamp() as u64);
                    }
                    Err(e) => {
                        error!("Failed to fetch paths from MongoDB: {}", e);
                    }
                }

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
        mongodb_service: &Arc<MongoDbService>,
        metrics: &Arc<RwLock<Metrics>>,
        swap_event_tx: &mpsc::Sender<PoolChange>,
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
                let latest_block = provider.get_block_number().await.unwrap();

                let pool_registry =
                    Arc::new(RwLock::new(PoolRegistry::new(provider.clone(), network_id)));
                let token_registry = Arc::new(RwLock::new(TokenRegistry::new(network_id)));
                let path_registry = Arc::new(RwLock::new(PathRegistry::new(network_id)));
                let price_updater =
                    Arc::new(RwLock::new(PriceUpdater::new(network.name, vec![]).await));

                let profit_token_registry = Arc::new(RwLock::new(ProfitTokenRegistry::new(
                    network.wrap_native.parse().unwrap(),
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
                    )
                    .await,
                ));

                let network_pools = mongodb_service
                    .get_pool_repo()
                    .find_by_network_id(network_id)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|p| p.address.parse().unwrap())
                    .collect();

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
                    if let Err(e) = pool_updater
                        .clone()
                        .write()
                        .await
                        .start(latest_block, network_pools)
                        .await
                    {
                        error!("Error starting pool updater: {}", e);
                    }
                });
            } else {
                // Checking changes in the network
                let network_registry = network_registries
                    .get_network_registry(network_id)
                    .await
                    .unwrap();

                network_registry.write().await.update_network(network).await;
            }
        }
        info!("Handled {} networks update", network_len);
        Ok(())
    }

    /// Handle pools update - PLACEHOLDER for implementation
    async fn handle_pools_update(
        pools: Vec<Pool>,
        _network_registries: &Arc<MultichainNetworkRegistry>,
    ) {
        // TODO: Implement pool update logic
        // - Group pools by network_id
        // - Compare with existing pools in each network_registry
        // - Fetch and add new pools using identify_and_fetch_pool
        // - Remove pools that no longer exist
        warn!(
            "handle_pools_update: PLACEHOLDER - {} pools received",
            pools.len()
        );
    }

    /// Handle paths update - PLACEHOLDER for implementation
    async fn handle_paths_update(
        paths: Vec<Path>,
        _network_registries: &Arc<MultichainNetworkRegistry>,
    ) {
        // TODO: Implement path update logic
        // - Compare with existing paths in path_registry for each network
        // - Add new paths
        // - Update existing paths
        // - Remove paths that no longer exist
        warn!(
            "handle_paths_update: PLACEHOLDER - {} paths received",
            paths.len()
        );
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
