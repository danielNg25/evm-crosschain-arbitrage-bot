use alloy::primitives::Address;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PriceSourceType {
    /// Geckoterminal
    GeckoTerminal,
}

#[derive(Debug, Clone)]
pub struct TokenPrice {
    pub address: Address,
    pub price: f64,
}

#[async_trait::async_trait]
pub trait PriceFetcher: Send + Sync {
    async fn update_chain_name(&mut self, chain_name: String);

    async fn fetch_prices(&self) -> Result<HashMap<Address, TokenPrice>>;

    fn get_price_source_type(&self) -> PriceSourceType;

    async fn add_token(&self, token: Address);

    async fn remove_token(&self, token: Address);
}
