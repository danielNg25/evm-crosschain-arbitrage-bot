use alloy::primitives::Address;
use anyhow::Result;
use std::collections::HashMap;

use super::{base::PriceFetcher, GeckoTerminalPriceFetcher, PriceSourceType, TokenPrice};

#[derive(Default)]
pub struct PriceUpdater {
    price_fetchers: Vec<Box<dyn PriceFetcher>>,
}

impl PriceUpdater {
    pub async fn new(network: String, tokens: Vec<Address>) -> Self {
        let gecko_terminal_price_fetcher = GeckoTerminalPriceFetcher::new(network);
        for token in tokens {
            gecko_terminal_price_fetcher.add_token(token).await;
        }

        Self {
            price_fetchers: vec![Box::new(gecko_terminal_price_fetcher)],
        }
    }

    pub async fn add_token(&self, token: Address, price_source: PriceSourceType) {
        for fetcher in &self.price_fetchers {
            if fetcher.get_price_source_type() == price_source {
                fetcher.add_token(token).await;
                break;
            }
        }
    }

    pub fn add_price_fetcher(&mut self, price_fetcher: Box<dyn PriceFetcher>) {
        self.price_fetchers.push(price_fetcher);
    }

    pub async fn update_prices(&self) -> Result<HashMap<Address, TokenPrice>> {
        let mut prices = HashMap::new();
        for fetcher in &self.price_fetchers {
            let returned_prices = fetcher.fetch_prices().await?;
            for (address, token_price) in returned_prices {
                prices.insert(address, token_price);
            }
        }
        Ok(prices)
    }
}
