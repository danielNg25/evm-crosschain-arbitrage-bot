use crate::{
    core::{create_provider, PoolUpdaterLatestBlock},
    models::{
        path::{PathRegistry, SingleChainPathsWithAnchorToken},
        pool::PoolRegistry,
        profit_token::{price_updater::PriceSourceType, ProfitToken, ProfitTokenRegistry},
        token::TokenRegistry,
    },
    services::models::Network,
    PoolInterface,
};
use alloy::{
    primitives::Address,
    providers::{DynProvider, MULTICALL3_ADDRESS},
};
use anyhow::Result;
use dashmap::DashMap;
use log::info;
use std::{collections::HashMap, str::FromStr, sync::Arc};
use tokio::sync::RwLock;

use alloy::primitives::U256;

pub struct MultichainNetworkRegistry {
    pub network_registries: DashMap<u64, Arc<RwLock<NetworkRegistry>>>,
}

pub struct NetworkRegistry {
    pub network_id: u64,
    pub network_name: String,
    pub rpcs: Vec<String>,
    pub provider: Arc<DynProvider>,
    pub pool_registry: Arc<RwLock<PoolRegistry>>,
    pub token_registry: Arc<RwLock<TokenRegistry>>,
    pub path_registry: Arc<RwLock<PathRegistry>>,
    pub pool_updater: Arc<RwLock<PoolUpdaterLatestBlock>>,
    pub profit_token_registry: Arc<RwLock<ProfitTokenRegistry>>,
}

impl NetworkRegistry {
    pub fn new(
        network_id: u64,
        network_name: String,
        rpcs: Vec<String>,
        provider: Arc<DynProvider>,
        pool_registry: Arc<RwLock<PoolRegistry>>,
        token_registry: Arc<RwLock<TokenRegistry>>,
        path_registry: Arc<RwLock<PathRegistry>>,
        pool_updater: Arc<RwLock<PoolUpdaterLatestBlock>>,
        profit_token_registry: Arc<RwLock<ProfitTokenRegistry>>,
    ) -> Self {
        Self {
            network_id,
            network_name,
            rpcs,
            provider,
            pool_registry,
            token_registry,
            path_registry,
            pool_updater,
            profit_token_registry,
        }
    }

    pub async fn update_network(&mut self, network: Network) {
        self.update_network_name(network.name.clone()).await;

        self.update_rpcs(network.rpcs.clone()).await;

        self.update_wrap_native(Address::from_str(&network.wrap_native).unwrap())
            .await;

        self.update_min_profit_usd(network.min_profit_usd).await;

        self.update_factory(
            network.v2_factory_to_fee.clone().unwrap_or_default(),
            network
                .aero_factory_addresses
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|a| Address::from_str(a).unwrap())
                .collect(),
        )
        .await;

        self.update_multicall_address(
            network
                .multicall_address
                .clone()
                .map(|s| Address::from_str(&s).unwrap())
                .unwrap_or(MULTICALL3_ADDRESS),
        )
        .await;

        self.update_wait_time_fetch(network.wait_time_fetch).await;

        self.update_max_blocks_per_batch(network.max_blocks_per_batch)
            .await;

        info!("Updated network {}", self.network_name);
    }

    pub async fn update_network_name(&mut self, network_name: String) {
        if self.network_name == network_name {
            return;
        }
        info!("Updating network name to {}", network_name);
        self.network_name = network_name.clone();
        self.profit_token_registry
            .read()
            .await
            .update_chain_name(network_name)
            .await;
    }

    pub async fn update_rpcs(&mut self, rpcs: Vec<String>) {
        if self.rpcs == rpcs {
            return;
        }
        info!(
            "Updating RPCs to {:?} for network {}",
            rpcs, self.network_name
        );
        let provider = create_provider(rpcs.clone());
        self.rpcs = rpcs;
        self.provider = Arc::new(provider.clone());
        self.pool_registry
            .write()
            .await
            .set_provider(provider.clone())
            .await;

        self.pool_updater.write().await.set_provider(provider).await;
    }

    pub async fn update_wrap_native(&mut self, wrap_native: Address) {
        if wrap_native
            == self
                .profit_token_registry
                .read()
                .await
                .get_wrap_native()
                .await
        {
            return;
        }
        info!(
            "Updating wrap native to {:?} for network {}",
            wrap_native, self.network_name
        );
        self.profit_token_registry
            .read()
            .await
            .set_wrap_native(wrap_native)
            .await;
    }

    pub async fn update_min_profit_usd(&mut self, min_profit_usd: f64) {
        if min_profit_usd == self.profit_token_registry.read().await.min_profit_usd {
            return;
        }
        info!(
            "Updating min profit usd to {} for network {}",
            min_profit_usd, self.network_name
        );
        self.profit_token_registry
            .write()
            .await
            .set_min_profit_usd(min_profit_usd)
            .await;
    }

    pub async fn update_factory(
        &mut self,
        v2_factory_to_fee: HashMap<String, u64>,
        aero_factory_addresses: Vec<Address>,
    ) {
        let pool_registry = self.pool_registry.read().await;

        let aero_unchanged =
            aero_factory_addresses == pool_registry.get_aero_factory_addresses().await;
        let v2_unchanged = v2_factory_to_fee == pool_registry.get_factory_to_fee().await;

        drop(pool_registry);

        if aero_unchanged && v2_unchanged {
            return;
        }

        if !aero_unchanged {
            info!(
                "Updating aero factory addresses to {:?} for network {}",
                aero_factory_addresses, self.network_name
            );
            self.pool_registry
                .read()
                .await
                .set_aero_factory_addresses(aero_factory_addresses)
                .await;
        }

        if !v2_unchanged {
            info!(
                "Updating v2 factory to fee to {:?} for network {}",
                v2_factory_to_fee, self.network_name
            );
            self.pool_registry
                .read()
                .await
                .set_factory_to_fee(v2_factory_to_fee)
                .await;
        }
    }

    pub async fn update_multicall_address(&mut self, multicall_address: Address) {
        let multicall_unchanged =
            multicall_address == self.pool_updater.read().await.multicall_address;

        if multicall_unchanged {
            return;
        }
        info!(
            "Updating multicall address to {:?} for network {}",
            multicall_address, self.network_name
        );
        self.pool_updater
            .write()
            .await
            .set_multicall_address(multicall_address)
            .await;
    }

    pub async fn update_wait_time_fetch(&mut self, wait_time_fetch: u64) {
        let wait_time_fetch_unchanged =
            wait_time_fetch == self.pool_updater.read().await.wait_time_fetch;
        if wait_time_fetch_unchanged {
            return;
        }
        info!(
            "Updating wait time fetch to {} for network {}",
            wait_time_fetch, self.network_name
        );
        self.pool_updater
            .write()
            .await
            .set_wait_time_fetch(wait_time_fetch)
            .await;
    }

    pub async fn update_max_blocks_per_batch(&mut self, max_blocks_per_batch: u64) {
        let max_blocks_per_batch_unchanged =
            max_blocks_per_batch == self.pool_updater.read().await.max_blocks_per_batch;

        if max_blocks_per_batch_unchanged {
            return;
        }
        info!(
            "Updating max blocks per batch to {} for network {}",
            max_blocks_per_batch, self.network_name
        );
        self.pool_updater
            .write()
            .await
            .set_max_blocks_per_batch(max_blocks_per_batch)
            .await;
    }
}

impl MultichainNetworkRegistry {
    pub fn new() -> Self {
        Self {
            network_registries: DashMap::new(),
        }
    }

    pub async fn get_network_info(&self, chain_id: u64) -> Option<(u64, String)> {
        let network_registry = self.get_network_registry(chain_id).await.unwrap();
        let guard = network_registry.read().await;
        Some((guard.network_id, guard.network_name.clone()))
    }

    pub async fn get_network_registry(
        &self,
        chain_id: u64,
    ) -> Option<Arc<RwLock<NetworkRegistry>>> {
        self.network_registries
            .get(&chain_id)
            .map(|r| Arc::clone(&r.value()))
    }

    // PATH
    pub async fn get_path_registry(&self, chain_id: u64) -> Option<Arc<RwLock<PathRegistry>>> {
        let network_registry = self.get_network_registry(chain_id).await.unwrap();
        let guard = network_registry.read().await;
        Some(guard.path_registry.clone())
    }

    pub async fn remove_network_registry(&mut self, chain_id: u64) -> Result<()> {
        self.network_registries.remove(&chain_id);
        Ok(())
    }

    pub async fn set_paths(&self, paths: Vec<SingleChainPathsWithAnchorToken>) -> Result<()> {
        for path in &paths {
            // verify path is valid
            let anchor_token = path.anchor_token;
            let mut pool_to_path = HashMap::new();
            for single_path in &path.paths {
                let profit_token = single_path.last().unwrap().token_out;
                self.get_profit_token_registry(path.chain_id)
                    .await
                    .unwrap()
                    .read()
                    .await
                    .add_token_if_not_exists(profit_token)
                    .await;
                if single_path.first().unwrap().token_in != anchor_token {
                    return Err(anyhow::anyhow!(
                        "First token in path is not the anchor token"
                    ));
                }

                let len = single_path.len();
                if len >= 2 {
                    for i in 0..len - 2 {
                        if single_path[i].token_out != single_path[i + 1].token_in {
                            return Err(anyhow::anyhow!("Path is not valid"));
                        }
                    }
                }

                let pool = single_path[0].pool;
                if !pool_to_path.contains_key(&pool) {
                    pool_to_path.insert(pool, Vec::new());
                }
                pool_to_path
                    .get_mut(&pool)
                    .unwrap()
                    .push(single_path.clone());
            }
            let path_registry = self.get_path_registry(path.chain_id).await.unwrap();
            // Filter out paths for the current chain_id - only include paths for other chains
            let other_paths: Vec<SingleChainPathsWithAnchorToken> = paths
                .iter()
                .filter(|p| p.chain_id != path.chain_id)
                .cloned()
                .collect();

            for (pool, paths) in pool_to_path {
                path_registry
                    .read()
                    .await
                    .set_paths(
                        pool,
                        SingleChainPathsWithAnchorToken {
                            paths: paths,
                            chain_id: path.chain_id,
                            anchor_token: path.anchor_token,
                        },
                        other_paths.clone(),
                    )
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn get_paths_for_pool(
        &self,
        chain_id: u64,
        pool: Address,
    ) -> Option<(
        SingleChainPathsWithAnchorToken,
        Vec<SingleChainPathsWithAnchorToken>,
    )> {
        let path_registry = self.get_path_registry(chain_id).await.unwrap();
        let guard = path_registry.read().await;
        guard.get_paths_for_pool(pool).await
    }

    // POOL
    pub async fn get_pool_registry(&self, network_id: u64) -> Option<Arc<RwLock<PoolRegistry>>> {
        let network_registry = self.get_network_registry(network_id).await;
        if let Some(network_registry) = network_registry {
            let guard = network_registry.read().await;
            Some(guard.pool_registry.clone())
        } else {
            None
        }
    }

    pub async fn get_pool(
        &self,
        network_id: u64,
        address: Address,
    ) -> Option<Arc<RwLock<Box<dyn PoolInterface + Send + Sync>>>> {
        let pool_registry = self.get_pool_registry(network_id).await;
        if let Some(pool_registry) = pool_registry {
            pool_registry.read().await.get_pool(&address).await
        } else {
            None
        }
    }

    // TOKEN
    pub async fn get_token_registry(&self, network_id: u64) -> Option<Arc<RwLock<TokenRegistry>>> {
        let network_registry = self.get_network_registry(network_id).await;
        if let Some(network_registry) = network_registry {
            let guard = network_registry.read().await;
            Some(guard.token_registry.clone())
        } else {
            None
        }
    }

    pub async fn get_token_symbol(&self, network_id: u64, address: Address) -> Option<String> {
        let token_registry = self.get_token_registry(network_id).await;
        if let Some(token_registry) = token_registry {
            let guard = token_registry.read().await;
            let token = guard.get_token(address);
            token.map(|t| t.symbol.clone())
        } else {
            None
        }
    }

    // PROFIT TOKEN
    pub async fn get_profit_token_registry(
        &self,
        network_id: u64,
    ) -> Option<Arc<RwLock<ProfitTokenRegistry>>> {
        let network_registry = self.get_network_registry(network_id).await;
        if let Some(network_registry) = network_registry {
            let guard = network_registry.read().await;
            Some(guard.profit_token_registry.clone())
        } else {
            None
        }
    }

    pub async fn add_token(&self, chain_id: u64, token: Address) {
        let config = ProfitToken {
            address: token,
            min_profit: U256::ZERO,
            price_source: Some(PriceSourceType::GeckoTerminal),
            price: None,
            default_price: 1.0,
        };
        let profit_token_registry = self.get_profit_token_registry(chain_id).await.unwrap();
        profit_token_registry
            .read()
            .await
            .add_token(token, config)
            .await;
    }

    pub async fn get_amount_for_value(
        &self,
        chain_id: u64,
        token: Address,
        value: f64,
    ) -> Option<U256> {
        let registry = self.get_profit_token_registry(chain_id).await?;
        let guard = registry.read().await;

        guard.get_amount_for_value(&token, value).await
    }

    pub async fn get_value(&self, chain_id: u64, token: Address, amount: U256) -> Option<f64> {
        let registry = self.get_profit_token_registry(chain_id).await?;
        let guard = registry.read().await;

        guard.get_value(&token, amount).await
    }

    pub async fn to_raw_amount(&self, chain_id: u64, token: Address, amount: &str) -> Result<U256> {
        let registry = self
            .get_profit_token_registry(chain_id)
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("Network registry not found for chain_id: {}", chain_id)
            })?;
        let guard = registry.read().await;

        Ok(guard.to_raw_amount(&token, amount).await?)
    }

    pub async fn to_human_amount(
        &self,
        chain_id: u64,
        token: Address,
        amount: U256,
    ) -> Result<String> {
        let registry = self
            .get_profit_token_registry(chain_id)
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("Network registry not found for chain_id: {}", chain_id)
            })?;
        let guard = registry.read().await;

        guard.to_human_amount(&token, amount).await
    }

    pub async fn to_raw_amount_f64(
        &self,
        chain_id: u64,
        token: Address,
        amount: f64,
    ) -> Result<U256> {
        let registry = self
            .get_profit_token_registry(chain_id)
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("Network registry not found for chain_id: {}", chain_id)
            })?;
        let guard = registry.read().await;

        Ok(guard.to_raw_amount_f64(&token, amount).await?)
    }

    pub async fn to_human_amount_f64(
        &self,
        chain_id: u64,
        token: Address,
        amount: U256,
    ) -> Result<f64> {
        let registry = self
            .get_profit_token_registry(chain_id)
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("Network registry not found for chain_id: {}", chain_id)
            })?;
        let guard = registry.read().await;

        Ok(guard.to_human_amount_f64(&token, amount).await?)
    }
}
