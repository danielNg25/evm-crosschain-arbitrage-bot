use alloy::primitives::utils::{format_units, parse_units};
use alloy::primitives::{Address, U256};
use alloy::providers::Provider;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

/// Represents a token on the blockchain
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Token {
    /// Token address
    pub address: Address,
    /// Network ID where this token exists
    pub network_id: u64,
    /// Token symbol
    pub symbol: String,
    /// Token name
    pub name: String,
    /// Token decimals
    pub decimals: u8,
}

impl Token {
    /// Create a new token
    pub fn new(
        address: Address,
        network_id: u64,
        symbol: String,
        name: String,
        decimals: u8,
    ) -> Self {
        Self {
            address,
            network_id,
            name,
            symbol,
            decimals,
        }
    }

    /// Convert a token amount from human-readable format to raw format (considering decimals)
    pub fn to_raw_amount(&self, amount: &str) -> Result<U256> {
        parse_units(amount, self.decimals)
            .map(Into::into)
            .map_err(|e| anyhow!("Failed to parse amount: {}", e))
    }

    /// Convert a token amount from raw format to human-readable format
    pub fn to_human_amount(&self, raw_amount: U256) -> Result<String> {
        format_units(raw_amount, self.decimals)
            .map_err(|e| anyhow!("Failed to format amount: {}", e))
    }

    /// Convert a token amount from human-readable float to raw format
    pub fn to_raw_amount_f64(&self, amount: f64) -> Result<U256> {
        let amount_str = format!("{}", amount);
        self.to_raw_amount(&amount_str)
    }

    /// Convert a token amount from raw format to human-readable float
    pub fn to_human_amount_f64(&self, raw_amount: U256) -> Result<f64> {
        let amount_str = self.to_human_amount(raw_amount)?;
        amount_str
            .parse::<f64>()
            .map_err(|e| anyhow!("Failed to parse float: {}", e))
    }

    pub async fn from_address<P: Provider + Send + Sync>(
        provider: Arc<P>,
        address: Address,
    ) -> Result<Self> {
        let token_instance = IERC20::new(address, &provider);
        let multicall = provider
            .multicall()
            .add(token_instance.name())
            .add(token_instance.symbol())
            .add(token_instance.decimals());

        let results = multicall.aggregate().await?;
        let (symbol, name, decimals) = results;

        Ok(Self::new(address, 0, symbol, name, decimals)) // Placeholder network_id
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} ({})", self.name, self.symbol, self.address)
    }
}

pub mod registry;

pub use registry::TokenRegistry;

use crate::blockchain::IERC20;
