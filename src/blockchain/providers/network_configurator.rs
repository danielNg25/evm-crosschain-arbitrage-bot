use alloy::providers::Provider;
use anyhow::Result;
use log::{error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;

#[derive(Debug, Clone)]
pub struct GasInfo {
    pub gas_price: u128,
    pub last_updated: std::time::Instant,
}

pub struct NetworkConfigurator<P: Provider + Send + Sync + 'static> {
    provider: Arc<P>,
    update_interval: Duration,
    pub gas_info: Arc<RwLock<GasInfo>>,
}

impl<P: Provider + Send + Sync + 'static> NetworkConfigurator<P> {
    pub fn new(provider: Arc<P>, update_interval: Duration) -> Self {
        Self {
            provider,
            update_interval,
            gas_info: Arc::new(RwLock::new(GasInfo {
                gas_price: 0,
                last_updated: std::time::Instant::now(),
            })),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let provider = self.provider.clone();
        let gas_info = self.gas_info.clone();
        let update_interval = self.update_interval;

        // Initial update
        Self::update_gas_info(&gas_info, &provider).await?;

        info!("Initial gas info: {:?}", self.gas_info.read().await);

        // Spawn update task
        tokio::spawn(async move {
            info!("Starting network configurator");
            let mut interval = interval(update_interval);
            loop {
                interval.tick().await;
                if let Err(e) = Self::update_gas_info(&gas_info, &provider).await {
                    error!("Failed to update gas info: {}", e);
                }
            }
        });

        Ok(())
    }

    async fn update_gas_info(gas_info: &Arc<RwLock<GasInfo>>, provider: &Arc<P>) -> Result<()> {
        let gas_price = provider.get_gas_price().await?;
        let mut info = gas_info.write().await;
        info.gas_price = gas_price;
        info.last_updated = std::time::Instant::now();
        drop(info);
        info!("Updated gas info: {:?}", gas_price);
        Ok(())
    }

    pub async fn get_gas_info(&self) -> GasInfo {
        self.gas_info.read().await.clone()
    }

    pub async fn get_gas_price(&self) -> Result<u128> {
        let guard = self.gas_info.read().await;
        let gas_price = guard.gas_price;
        Ok(gas_price)
    }
}
