use crate::models::token::TokenRegistry;
use alloy::primitives::{Address, U256};
use anyhow::Result;
use dashmap::DashMap;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

use super::price_updater::{PriceSourceType, PriceUpdater};
/// Configuration for a profit token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitToken {
    /// Token address
    pub address: Address,
    /// Minimum profit amount in token units
    pub min_profit: U256,
    /// Price source
    pub price_source: Option<PriceSourceType>,
    /// Price
    pub price: Option<f64>,
    /// default price
    pub default_price: f64,
}

/// Global registry for profit tokens
#[derive(Default)]
pub struct ProfitTokenRegistry {
    /// Map of profit token configurations
    tokens: Arc<RwLock<std::collections::HashMap<Address, ProfitToken>>>,
    /// Token registry
    token_registry: Arc<RwLock<TokenRegistry>>,
    /// Price updater
    price_updater: Arc<RwLock<PriceUpdater>>,
    /// wrap native
    pub wrap_native: Arc<RwLock<Address>>,
    /// minimum profit usd
    pub min_profit_usd: f64,
}

#[derive(Default)]
pub struct MultichainProfitTokenRegistry {
    profit_token_registries: Arc<RwLock<DashMap<u64, ProfitTokenRegistry>>>,
    /// minimum profit usd
    pub min_profit_usd: f64,
}

impl MultichainProfitTokenRegistry {
    pub fn new(min_profit_usd: f64) -> Self {
        Self {
            profit_token_registries: Arc::new(RwLock::new(DashMap::new())),
            min_profit_usd,
        }
    }

    pub async fn new_profit_token_registry(
        &self,
        chain_id: u64,
        chain_name: String,
        wrap_native: Address,
        token_registry: Arc<RwLock<TokenRegistry>>,
    ) -> Result<()> {
        let price_updater = Arc::new(RwLock::new(PriceUpdater::new(chain_name, vec![]).await));
        let profit_token_registry = ProfitTokenRegistry::new(
            wrap_native,
            token_registry,
            price_updater,
            self.min_profit_usd,
        );
        if let Err(e) = profit_token_registry.start() {
            error!("Failed to start profit token registry: {}", e);
            return Err(anyhow::anyhow!(
                "Failed to start profit token registry: {}",
                e
            ));
        }
        self.profit_token_registries
            .write()
            .await
            .insert(chain_id, profit_token_registry);

        Ok(())
    }

    pub async fn add_token(&self, chain_id: u64, token: Address) {
        let config = ProfitToken {
            address: token,
            min_profit: U256::ZERO,
            price_source: Some(PriceSourceType::GeckoTerminal),
            price: None,
            default_price: 1.0,
        };
        self.profit_token_registries
            .write()
            .await
            .get_mut(&chain_id)
            .unwrap()
            .add_token(token, config)
            .await;
    }

    pub async fn get_amount_for_value(
        &self,
        chain_id: u64,
        token: Address,
        value: f64,
    ) -> Option<U256> {
        self.profit_token_registries
            .read()
            .await
            .get(&chain_id)
            .unwrap()
            .get_amount_for_value(&token, value)
            .await
    }

    pub async fn get_value(&self, chain_id: u64, token: Address, amount: U256) -> Option<f64> {
        self.profit_token_registries
            .read()
            .await
            .get(&chain_id)
            .unwrap()
            .get_value(&token, amount)
            .await
    }

    pub async fn to_raw_amount(&self, chain_id: u64, token: Address, amount: &str) -> Result<U256> {
        self.profit_token_registries
            .read()
            .await
            .get(&chain_id)
            .unwrap()
            .to_raw_amount(&token, amount)
            .await
    }

    pub async fn to_human_amount(
        &self,
        chain_id: u64,
        token: Address,
        amount: U256,
    ) -> Result<String> {
        self.profit_token_registries
            .read()
            .await
            .get(&chain_id)
            .unwrap()
            .to_human_amount(&token, amount)
            .await
    }

    pub async fn to_raw_amount_f64(
        &self,
        chain_id: u64,
        token: Address,
        amount: f64,
    ) -> Result<U256> {
        self.profit_token_registries
            .read()
            .await
            .get(&chain_id)
            .unwrap()
            .to_raw_amount_f64(&token, amount)
            .await
    }

    pub async fn to_human_amount_f64(
        &self,
        chain_id: u64,
        token: Address,
        amount: U256,
    ) -> Result<f64> {
        self.profit_token_registries
            .read()
            .await
            .get(&chain_id)
            .unwrap()
            .to_human_amount_f64(&token, amount)
            .await
    }
}

impl ProfitTokenRegistry {
    pub fn new(
        wrap_native: Address,
        token_registry: Arc<RwLock<TokenRegistry>>,
        price_updater: Arc<RwLock<PriceUpdater>>,
        min_profit_usd: f64,
    ) -> Self {
        Self {
            tokens: Arc::new(RwLock::new(std::collections::HashMap::new())),
            wrap_native: Arc::new(RwLock::new(wrap_native)),
            token_registry,
            price_updater,
            min_profit_usd,
        }
    }

    pub fn start(&self) -> Result<()> {
        info!("Starting price updater with 1-hour interval");
        let price_updater = self.price_updater.clone();
        let tokens = self.tokens.clone();
        let token_registry = self.token_registry.clone();
        let min_profit_usd = self.min_profit_usd;

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(3600)); // 1 hour
            loop {
                match price_updater.read().await.update_prices().await {
                    Ok(prices) => {
                        for (token, price) in prices {
                            if let Some(config) = tokens.write().await.get_mut(&token) {
                                config.price = Some(price.price);
                                if let Some(token_info) =
                                    token_registry.read().await.get_token(token)
                                {
                                    let min_profit = min_profit_usd / price.price;
                                    let scaled = (min_profit
                                        * 10_f64.powi(token_info.decimals as i32))
                                    .round();
                                    config.min_profit = U256::from(scaled as u128);
                                }
                            }
                            info!("Updated price for token: {} {}", token, price.price);
                        }
                    }
                    Err(e) => {
                        error!("Failed to update prices: {:?}", e);
                    }
                }
                interval.tick().await;
            }
        });
        Ok(())
    }

    /// Add a profit token with its configuration
    pub async fn add_token(&self, token: Address, config: ProfitToken) {
        if self.tokens.read().await.contains_key(&token) {
            return;
        }
        let price_source = config.price_source.clone();
        self.tokens.write().await.insert(token, config);
        if let Some(price_source) = price_source {
            self.price_updater
                .write()
                .await
                .add_token(token, price_source)
                .await;
        }
    }

    /// Set the wrap native
    pub async fn set_wrap_native(&self, wrap_native: Address) {
        *self.wrap_native.write().await = wrap_native;
    }

    /// Get the wrap native
    pub async fn get_wrap_native(&self) -> Address {
        self.wrap_native.read().await.clone()
    }

    /// Remove a profit token
    pub async fn remove_token(&self, token: &Address) {
        self.tokens.write().await.remove(token);
    }

    /// Check if a token is a profit token
    pub async fn is_profit_token(&self, token: &Address) -> bool {
        self.tokens.read().await.contains_key(token)
    }

    /// Get the configuration for a profit token
    pub async fn get_config(&self, token: &Address) -> Option<ProfitToken> {
        self.tokens.read().await.get(token).cloned()
    }

    /// Get the minimum profit for a profit token
    pub async fn get_min_profit(&self, token: &Address) -> Option<U256> {
        self.tokens
            .read()
            .await
            .get(token)
            .map(|config| config.min_profit)
    }

    /// Get all profit tokens
    pub async fn get_tokens(&self) -> Vec<Address> {
        self.tokens.read().await.keys().cloned().collect()
    }

    /// Get the price for a profit token
    pub async fn get_price(&self, token: &Address) -> Option<f64> {
        if let Some(config) = self.tokens.read().await.get(token) {
            Some(config.price.clone().unwrap_or(config.default_price))
        } else {
            None
        }
    }

    pub async fn get_value(&self, token: &Address, amount: U256) -> Option<f64> {
        if let Some(price) = self.get_price(token).await {
            if let Some(token_info) = self.token_registry.read().await.get_token(*token) {
                let amount_f64 = token_info.to_human_amount_f64(amount).ok()?;
                Some(price * amount_f64)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub async fn get_native_value(&self, amount: U256) -> Option<f64> {
        self.get_value(&self.get_wrap_native().await, amount).await
    }

    /// Set the price for a profit token
    pub async fn set_price(&self, token: &Address, price: f64) {
        if let Some(config) = self.tokens.write().await.get_mut(token) {
            config.price = Some(price);
            if let Some(token_info) = self.token_registry.read().await.get_token(*token) {
                let min_profit = self.min_profit_usd / price;
                let scaled = (min_profit * 10_f64.powi(token_info.decimals as i32)).round();
                config.min_profit = U256::from(scaled as u128);
                info!(
                    "Set min profit for token {} to {} {}, min profit usd {}",
                    token, config.min_profit, scaled, self.min_profit_usd
                );
            }
        }
    }

    /// Get all profit token configurations
    pub async fn get_configs(&self) -> Vec<(Address, ProfitToken)> {
        self.tokens
            .read()
            .await
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    pub async fn get_tokens_by_price_source(
        &self,
        price_source: PriceSourceType,
    ) -> Vec<ProfitToken> {
        self.tokens
            .read()
            .await
            .values()
            .filter(|token| {
                token.price_source.as_ref().map_or(false, |ps| {
                    std::mem::discriminant(ps) == std::mem::discriminant(&price_source)
                })
            })
            .cloned()
            .collect()
    }

    pub async fn update_token_price(&self, address: Address, price: f64) {
        if let Some(token) = self.tokens.write().await.get_mut(&address) {
            token.price = Some(price);
        }
    }

    pub async fn get_amount_for_value(&self, token: &Address, usd_value: f64) -> Option<U256> {
        if let Some(price) = self.get_price(token).await {
            if let Some(token_info) = self.token_registry.read().await.get_token(*token) {
                let amount_f64 = usd_value / price;
                Some(U256::from(
                    (amount_f64 * 10_f64.powi(token_info.decimals as i32)).round() as u128,
                ))
            } else {
                None
            }
        } else {
            None
        }
    }

    pub async fn to_raw_amount(&self, token: &Address, amount: &str) -> Result<U256> {
        self.token_registry
            .read()
            .await
            .to_raw_amount(*token, amount)
    }

    pub async fn to_human_amount(&self, token: &Address, amount: U256) -> Result<String> {
        self.token_registry
            .read()
            .await
            .to_human_amount(*token, amount)
    }

    pub async fn to_raw_amount_f64(&self, token: &Address, amount: f64) -> Result<U256> {
        self.token_registry
            .read()
            .await
            .to_raw_amount_f64(*token, amount)
    }

    pub async fn to_human_amount_f64(&self, token: &Address, amount: U256) -> Result<f64> {
        self.token_registry
            .read()
            .await
            .to_human_amount_f64(*token, amount)
    }
}
