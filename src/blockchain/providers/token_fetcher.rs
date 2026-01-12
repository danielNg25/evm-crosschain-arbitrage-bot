use crate::blockchain::IERC20;
use crate::models::token::{Token, TokenRegistry};
use alloy::primitives::Address;
use alloy::providers::Provider;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Fetches token data for an ERC20 token
pub async fn fetch_token_data<P: Provider + Send + Sync>(
    provider: &Arc<P>,
    token_address: Address,
    network_id: u64,
    multicall_address: Address,
) -> Result<Token> {
    let token_instance = IERC20::new(token_address, &provider);
    let multicall = provider
        .multicall()
        .address(multicall_address)
        .add(token_instance.name())
        .add(token_instance.symbol())
        .add(token_instance.decimals());

    let results = multicall.aggregate().await?;
    let (name, symbol, decimals) = results;

    Ok(Token::new(
        token_address,
        network_id,
        symbol,
        name,
        decimals as u8,
    ))
}

pub async fn get_or_fetch_token<P: Provider + Send + Sync>(
    token_registry: &Arc<RwLock<TokenRegistry>>,
    provider: &Arc<P>,
    address: Address,
    multicall_address: Address,
) -> Result<(Address, u8)> {
    // First check if token exists (quick check with minimal lock time)
    let registry = token_registry.read().await;
    if let Some(token) = registry.get_token(address) {
        return Ok((address, token.decimals));
    }

    // Get network_id from registry
    let network_id = registry.get_network_id();
    drop(registry); // Explicitly release the lock

    // Fetch token data (no lock held during this potentially long operation)
    let token_data = fetch_token_data(provider, address, network_id, multicall_address).await?;

    let token = Token::new(
        address,
        network_id,
        token_data.symbol,
        token_data.name,
        token_data.decimals,
    );

    // Add token to registry with automatic network_id assignment
    token_registry
        .write()
        .await
        .add_token_with_network_id(token)?;

    Ok((address, token_data.decimals))
}
