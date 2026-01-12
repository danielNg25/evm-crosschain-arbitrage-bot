use alloy::primitives::Address;
use anyhow::Result;
use log::info;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{collections::HashMap, str::FromStr};
use tokio::sync::RwLock;

use super::base::{PriceFetcher, PriceSourceType, TokenPrice};

#[derive(Debug, Serialize, Deserialize)]
struct GeckoTerminalResponse {
    data: GeckoTerminalData,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeckoTerminalData {
    id: String,
    #[serde(rename = "type")]
    type_field: String,
    attributes: GeckoTerminalAttributes,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeckoTerminalAttributes {
    token_prices: HashMap<String, String>,
}

pub struct GeckoTerminalPriceFetcher {
    network: String,
    token_addresses: Arc<RwLock<Vec<Address>>>,
    client: Client,
}

impl GeckoTerminalPriceFetcher {
    pub fn new(network: String) -> Self {
        Self {
            network,
            token_addresses: Arc::new(RwLock::new(Vec::new())),
            client: Client::new(),
        }
    }

    pub async fn get_token_addresses(&self) -> Vec<Address> {
        self.token_addresses.read().await.clone()
    }
}

#[async_trait::async_trait]
impl PriceFetcher for GeckoTerminalPriceFetcher {
    async fn update_chain_name(&mut self, chain_name: String) {
        self.network = chain_name;
    }

    async fn fetch_prices(&self) -> Result<HashMap<Address, TokenPrice>> {
        let addresses = self.token_addresses.read().await;
        if addresses.is_empty() {
            return Ok(HashMap::new());
        }

        // Convert addresses to comma-separated string
        let address_str = addresses
            .iter()
            .map(|addr| addr.to_string())
            .collect::<Vec<_>>()
            .join(",");
        info!(
            "Fetching prices on {} for {} addresses",
            self.network,
            addresses.len()
        );

        let url = format!(
            "https://api.geckoterminal.com/api/v2/simple/networks/{}/token_price/{}",
            self.network, address_str
        );

        let response = self.client.get(&url).send().await?;
        let data: GeckoTerminalResponse = response.json().await?;
        let mut prices = HashMap::with_capacity(addresses.len());
        for address in addresses.iter() {
            if let Some(price_str) = data
                .data
                .attributes
                .token_prices
                .get(&address.to_string().to_lowercase())
            {
                if let Ok(price) = f64::from_str(price_str) {
                    prices.insert(
                        *address,
                        TokenPrice {
                            address: *address,
                            price,
                        },
                    );
                }
            }
        }

        Ok(prices)
    }

    fn get_price_source_type(&self) -> PriceSourceType {
        PriceSourceType::GeckoTerminal
    }

    async fn add_token(&self, address: Address) {
        let mut addresses = self.token_addresses.write().await;
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }

    async fn remove_token(&self, address: Address) {
        let mut addresses = self.token_addresses.write().await;
        addresses.retain(|&addr| addr != address);
    }
}
