use alloy::primitives::Address;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::models::pool::registry::PoolRegistry;
use crate::PoolInterface;

#[derive(Debug, Default)]
pub struct MultichainPoolRegistry {
    pools: Arc<RwLock<HashMap<u64, Arc<PoolRegistry>>>>,
}

impl MultichainPoolRegistry {
    pub fn new() -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_pool_registry(&self, network_id: u64, pool_registry: Arc<PoolRegistry>) {
        let mut pools = self.pools.write().await;
        pools.insert(network_id, pool_registry);
    }

    pub async fn get_pool_registry(&self, network_id: u64) -> Option<Arc<PoolRegistry>> {
        let pools = self.pools.read().await;
        pools.get(&network_id).map(Arc::clone)
    }

    pub async fn get_all_network_ids(&self) -> Vec<u64> {
        let pools = self.pools.read().await;
        pools.keys().cloned().collect()
    }

    pub async fn remove_pool_registry(&self, network_id: u64) -> Option<Arc<PoolRegistry>> {
        let mut pools = self.pools.write().await;
        pools.remove(&network_id)
    }

    pub async fn pool_count(&self) -> usize {
        let pools = self.pools.read().await;
        pools.len()
    }

    pub async fn total_pools_across_networks(&self) -> usize {
        let pools = self.pools.read().await;
        let mut total = 0;
        for registry in pools.values() {
            total += registry.pool_count().await;
        }
        total
    }

    pub async fn contains_pool_registry(&self, network_id: u64) -> bool {
        let pools = self.pools.read().await;
        pools.contains_key(&network_id)
    }

    pub async fn contains_pool(&self, network_id: u64, pool_address: Address) -> bool {
        let pools = self.pools.read().await;
        if let Some(registry) = pools.get(&network_id) {
            registry.get_pool(&pool_address).await.is_some()
        } else {
            false
        }
    }

    pub async fn get_pool(
        &self,
        network_id: u64,
        address: Address,
    ) -> Option<Arc<RwLock<Box<dyn PoolInterface + Send + Sync>>>> {
        let pools = self.pools.read().await;
        if let Some(registry) = pools.get(&network_id) {
            registry.get_pool(&address).await
        } else {
            None
        }
    }
}

impl Clone for MultichainPoolRegistry {
    fn clone(&self) -> Self {
        Self {
            pools: Arc::clone(&self.pools),
        }
    }
}
