use crate::{
    blockchain::{
        get_or_fetch_token, IUniswapV2Pair, IV2PairUint256, IVeloPoolFactory,
        UniswapV2FactoryGetFeeOnlyPair, UniswapV2FactoryGetFeePool, UniswapV2FactoryPairFee,
        VolatileStableFeeInFactory, VolatileStableGetFee,
    },
    models::pool::{
        base::{EventApplicable, PoolInterface, PoolTypeTrait, TopicList},
        v2::{default_factory_fee_by_chain_id, get_v2_factory_fee},
    },
    services::Database,
    PoolType,
};
use alloy::{
    eips::BlockId,
    primitives::U160,
    providers::{MulticallBuilder, Provider},
};
use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

use crate::models::token::TokenRegistry;
use alloy::sol_types::SolEvent;
use alloy::{
    primitives::{Address, FixedBytes, U256},
    rpc::types::Log,
};
use anyhow::{anyhow, Result};
use log::{debug, error, info, trace};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt;
const FEE_DENOMINATOR: u128 = 1000000;
const EXP18: u128 = 1_000_000_000_000_000_000;
const FACTORY_STORAGE_SLOT: u64 = 0xb;
const GET_FEE_MULTIPLIER: u128 = 100;
const GET_FEE_MAX: u128 = 10000;
const REVERSE_FEE_MAX: u128 = 5000;
/// Enum representing the type of V3 pool
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum V2PoolType {
    UniswapV2,
    Stable,
}

/// UniswapV2 Pool implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV2Pool {
    /// Pool type
    pub pool_type: V2PoolType,
    /// Pool address
    pub address: Address,
    /// First token address in the pool
    pub token0: Address,
    /// Second token address in the pool
    pub token1: Address,
    /// Decimals of token0
    pub decimals0: u128,
    /// Decimals of token1
    pub decimals1: u128,
    /// Reserve of token0
    pub reserve0: U256,
    /// Reserve of token1
    pub reserve1: U256,
    /// Pool fee (e.g., 0.003 for 0.3%)
    pub fee: U256,
    /// Last update timestamp
    pub last_updated: u64,
    /// Creation timestamp or block
    pub created_at: u64,
}

impl UniswapV2Pool {
    /// Create a new V2 pool
    pub fn new(
        pool_type: V2PoolType,
        address: Address,
        token0: Address,
        token1: Address,
        decimals_0: u8,
        decimals_1: u8,
        reserve0: U256,
        reserve1: U256,
        fee: U256,
    ) -> Self {
        let current_time = chrono::Utc::now().timestamp() as u64;
        Self {
            pool_type,
            address,
            token0,
            token1,
            decimals0: 10_u128.pow(decimals_0 as u32),
            decimals1: 10_u128.pow(decimals_1 as u32),
            reserve0,
            reserve1,
            fee,
            last_updated: current_time,
            created_at: current_time,
        }
    }

    /// Update pool reserves
    pub fn update_reserves(&mut self, reserve0: U256, reserve1: U256) -> Result<()> {
        self.reserve0 = reserve0;
        self.reserve1 = reserve1;
        self.last_updated = chrono::Utc::now().timestamp() as u64;
        Ok(())
    }

    /// Calculate the constant product k = x * y
    pub fn constant_product(&self) -> U256 {
        self.reserve0 * self.reserve1
    }

    /// Get the exp18 value
    pub fn exp18() -> U256 {
        U256::from(EXP18)
    }

    /// Check if the pool is valid (has non-zero reserves)
    pub fn is_valid(&self) -> bool {
        !self.reserve0.is_zero() && !self.reserve1.is_zero()
    }

    /// Calculate the output amount for a swap (token0 -> token1)
    fn calculate_output_0_to_1(&self, amount_in: U256) -> Result<U256> {
        if amount_in.is_zero() {
            return Err(anyhow!("Input amount cannot be zero"));
        }

        if !self.is_valid() {
            return Err(anyhow!("Pool reserves are invalid"));
        }
        match self.pool_type {
            V2PoolType::UniswapV2 => {
                let amount_in_with_fee: alloy::primitives::Uint<256, 4> =
                    amount_in.saturating_mul(U256::from(U256::from(FEE_DENOMINATOR) - (self.fee)));
                let numerator = amount_in_with_fee * self.reserve1;
                let denominator = self.reserve0 * U256::from(FEE_DENOMINATOR) + amount_in_with_fee;
                // Can't return more than all reserves
                let output = numerator / denominator;
                if output >= self.reserve1 {
                    return Err(anyhow!("Insufficient liquidity for swap"));
                }

                Ok(output)
            }
            V2PoolType::Stable => {
                let amount_in_with_fee: alloy::primitives::Uint<256, 4> = amount_in
                    .saturating_mul(U256::from(U256::from(FEE_DENOMINATOR) - (self.fee)))
                    .checked_div(U256::from(FEE_DENOMINATOR))
                    .unwrap();
                let exp18 = Self::exp18();
                let xy = self.k(self.reserve0, self.reserve1);
                let reserve0 = (self.reserve0 * exp18) / U256::from(self.decimals0);
                let reserve1 = (self.reserve1 * exp18) / U256::from(self.decimals1);
                let (reserve_a, reserve_b) = (reserve0, reserve1); // Swap token0 for token1
                let amount_in_parsed = (amount_in_with_fee * exp18) / U256::from(self.decimals0);
                let y = reserve_b - self.get_y(amount_in_parsed + reserve_a, xy, reserve_b)?;
                let output = (y * U256::from(self.decimals1)) / exp18;

                Ok(output)
            }
        }
    }

    /// Calculate the output amount for a swap (token1 -> token0)
    fn calculate_output_1_to_0(&self, amount_in: U256) -> Result<U256> {
        if amount_in.is_zero() {
            return Err(anyhow!("Input amount cannot be zero"));
        }

        if !self.is_valid() {
            return Err(anyhow!("Pool reserves are invalid"));
        }

        match self.pool_type {
            V2PoolType::UniswapV2 => {
                let amount_in_with_fee =
                    amount_in.saturating_mul(U256::from(U256::from(FEE_DENOMINATOR) - (self.fee)));
                let numerator = amount_in_with_fee * self.reserve0;
                let denominator = self.reserve1 * U256::from(FEE_DENOMINATOR) + amount_in_with_fee;

                // Can't return more than all reserves
                let output = numerator / denominator;
                if output >= self.reserve0 {
                    return Err(anyhow!("Insufficient liquidity for swap"));
                }

                Ok(output)
            }
            V2PoolType::Stable => {
                let amount_in_with_fee: alloy::primitives::Uint<256, 4> = amount_in
                    .saturating_mul(U256::from(U256::from(FEE_DENOMINATOR) - (self.fee)))
                    .checked_div(U256::from(FEE_DENOMINATOR))
                    .unwrap();
                let exp18 = Self::exp18();
                let xy = self.k(self.reserve0, self.reserve1);
                let reserve0 = (self.reserve0 * exp18) / U256::from(self.decimals0);
                let reserve1 = (self.reserve1 * exp18) / U256::from(self.decimals1);
                let (reserve_a, reserve_b) = (reserve1, reserve0); // Swap token0 for token1
                let amount_in_parsed = (amount_in_with_fee * exp18) / U256::from(self.decimals1);
                let y = reserve_b - self.get_y(amount_in_parsed + reserve_a, xy, reserve_b)?;
                let output = (y * U256::from(self.decimals0)) / exp18;

                Ok(output)
            }
        }
    }

    fn k(&self, x: U256, y: U256) -> U256 {
        if self.pool_type == V2PoolType::Stable {
            let exp18 = Self::exp18();
            let _x = (x * exp18) / U256::from(self.decimals0);
            let _y = (y * exp18) / U256::from(self.decimals1);
            let _a = (_x * _y) / exp18;
            let _b = (_x * _x) / exp18 + (_y * _y) / exp18;
            return (_a * _b) / exp18; // x3y+y3x >= k
        } else {
            return x * y;
        }
    }

    pub fn f(x0: U256, y: U256) -> U256 {
        let exp18 = Self::exp18();
        let _a = (x0 * y) / exp18;
        let _b = (x0 * x0) / exp18 + (y * y) / exp18;
        return (_a * _b) / exp18;
    }

    fn d(x0: U256, y: U256) -> U256 {
        let exp18 = Self::exp18();
        return (U256::from(3) * x0 * ((y * y) / exp18)) / exp18
            + ((((x0 * x0) / exp18) * x0) / exp18);
    }

    fn get_y(&self, x0: U256, xy: U256, mut y: U256) -> Result<U256> {
        let exp18 = Self::exp18();
        for _ in 0..255 {
            let k = Self::f(x0, y);
            if k < xy {
                // there are two cases where dy == 0
                // case 1: The y is converged and we find the correct answer
                // case 2: _d(x0, y) is too large compare to (xy - k) and the rounding error
                //         screwed us.
                //         In this case, we need to increase y by 1
                let mut dy = ((xy - k) * exp18) / Self::d(x0, y);
                if dy.is_zero() {
                    if k == xy {
                        // We found the correct answer. Return y
                        return Ok(y);
                    }
                    if self.k(x0, y + U256::ONE) > xy {
                        // If _k(x0, y + 1) > xy, then we are close to the correct answer.
                        // There's no closer answer than y + 1
                        return Ok(y + U256::ONE);
                    }
                    dy = U256::ONE;
                }
                y = y + dy;
            } else {
                let mut dy = ((k - xy) * exp18) / Self::d(x0, y);
                if dy.is_zero() {
                    if k == xy || Self::f(x0, y - U256::ONE) < xy {
                        // Likewise, if k == xy, we found the correct answer.
                        // If _f(x0, y - 1) < xy, then we are close to the correct answer.
                        // There's no closer answer than "y"
                        // It's worth mentioning that we need to find y where f(x0, y) >= xy
                        // As a result, we can't return y - 1 even it's closer to the correct answer
                        return Ok(y);
                    }
                    dy = U256::ONE;
                }
                y = y - dy;
            }
        }
        return Err(anyhow!("!y"));
    }

    fn calculate_input_0_to_1(&self, amount_out: U256) -> Result<U256> {
        if amount_out.is_zero() {
            return Err(anyhow!("Output amount cannot be zero"));
        }

        if !self.is_valid() {
            return Err(anyhow!("Pool reserves are invalid"));
        }

        if amount_out >= self.reserve1 {
            return Err(anyhow!("Insufficient liquidity for swap"));
        }

        let numerator = self.reserve0 * amount_out * U256::from(FEE_DENOMINATOR);
        let denominator = (self.reserve1 - amount_out) * (U256::from(FEE_DENOMINATOR) - self.fee);

        // Add 1 to round up
        let input = (numerator / denominator) + U256::from(1);

        Ok(input)
    }

    fn calculate_input_1_to_0(&self, amount_out: U256) -> Result<U256> {
        if amount_out.is_zero() {
            return Err(anyhow!("Output amount cannot be zero"));
        }

        if !self.is_valid() {
            return Err(anyhow!("Pool reserves are invalid"));
        }

        if amount_out >= self.reserve0 {
            return Err(anyhow!("Insufficient liquidity for swap"));
        }

        let numerator = self.reserve1 * amount_out * U256::from(FEE_DENOMINATOR);
        let denominator = (self.reserve0 - amount_out) * (U256::from(FEE_DENOMINATOR) - self.fee);

        // Add 1 to round up
        let input = (numerator / denominator) + U256::from(1);

        Ok(input)
    }

    /// Save pool data to database
    pub fn save_to_db(&self, db: &Database) -> Result<()> {
        let key = self.address.to_string();
        db.insert("v2_pools", key, self)?;
        debug!("Saved V2 pool {} to database", self.address);
        Ok(())
    }

    /// Load pool data from database
    pub fn load_from_db(db: &Database, address: &Address) -> Result<Option<Self>> {
        let key = address.to_string();
        let pool = db.get::<_, Self>("v2_pools", key)?;
        if let Some(ref _loaded_pool) = pool {
            debug!("Loaded V2 pool {} from database", address);
        }
        Ok(pool)
    }

    /// Load all V2 pools from database
    pub fn load_all_from_db(db: &Database) -> Result<Vec<Self>> {
        let mut pools = Vec::new();
        let iter = db.iter::<Self>("v2_pools")?;

        for result in iter {
            match result {
                Ok((_, pool)) => pools.push(pool),
                Err(e) => error!("Error loading V2 pool: {}", e),
            }
        }

        info!("Loaded {} V2 pools from database", pools.len());
        Ok(pools)
    }
}

impl PoolInterface for UniswapV2Pool {
    fn calculate_output(&self, token_in: &Address, amount_in: U256) -> Result<U256> {
        if token_in == &self.token0 {
            self.calculate_output_0_to_1(amount_in)
        } else if token_in == &self.token1 {
            self.calculate_output_1_to_0(amount_in)
        } else {
            Err(anyhow!("Token not in pool"))
        }
    }

    fn calculate_input(&self, token_out: &Address, amount_out: U256) -> Result<U256> {
        if token_out == &self.token0 {
            self.calculate_input_1_to_0(amount_out)
        } else if token_out == &self.token1 {
            self.calculate_input_0_to_1(amount_out)
        } else {
            Err(anyhow!("Token not in pool"))
        }
    }

    fn apply_swap(&mut self, token_in: &Address, amount_in: U256, amount_out: U256) -> Result<()> {
        if token_in == &self.token0 {
            // Token0 -> Token1 swap
            if amount_out >= self.reserve1 {
                return Err(anyhow!("Insufficient liquidity for swap"));
            }
            self.reserve0 += amount_in;
            self.reserve1 -= amount_out;
        } else if token_in == &self.token1 {
            // Token1 -> Token0 swap
            if amount_out >= self.reserve0 {
                return Err(anyhow!("Insufficient liquidity for swap"));
            }
            self.reserve1 += amount_in;
            self.reserve0 -= amount_out;
        } else {
            return Err(anyhow!("Token not in pool"));
        }

        self.last_updated = chrono::Utc::now().timestamp() as u64;
        Ok(())
    }

    fn address(&self) -> Address {
        self.address
    }

    fn tokens(&self) -> (Address, Address) {
        (self.token0, self.token1)
    }

    fn fee(&self) -> f64 {
        self.fee.to::<u128>() as f64 / FEE_DENOMINATOR as f64
    }

    fn fee_raw(&self) -> u64 {
        self.fee.to::<u128>() as u64
    }

    fn id(&self) -> String {
        format!("v2-{}-{}-{}", self.address, self.token0, self.token1)
    }

    fn log_summary(&self) -> String {
        format!(
            "V2 Pool {} - {} <> {} (reserves: {}, {})",
            self.address, self.token0, self.token1, self.reserve0, self.reserve1
        )
    }

    fn contains_token(&self, token: &Address) -> bool {
        *token == self.token0 || *token == self.token1
    }

    fn clone_box(&self) -> Box<dyn PoolInterface + Send + Sync> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl EventApplicable for UniswapV2Pool {
    fn apply_log(&mut self, log: &Log) -> Result<()> {
        match log.topic0() {
            Some(&IUniswapV2Pair::Sync::SIGNATURE_HASH) => {
                let sync_data: IUniswapV2Pair::Sync = log.log_decode()?.inner.data;
                debug!(
                    "Applying V2Sync event to pool {}: reserve0={}, reserve1={}",
                    self.address, sync_data.reserve0, sync_data.reserve1
                );
                self.update_reserves(
                    U256::from(sync_data.reserve0),
                    U256::from(sync_data.reserve1),
                )?;
                Ok(())
            }
            Some(&IV2PairUint256::Sync::SIGNATURE_HASH) => {
                let sync_data: IV2PairUint256::Sync = log.log_decode()?.inner.data;
                debug!(
                    "Applying V2Sync event to pool {}: reserve0={}, reserve1={}",
                    self.address, sync_data.reserve0, sync_data.reserve1
                );
                self.update_reserves(sync_data.reserve0, sync_data.reserve1)?;
                Ok(())
            }
            Some(&IUniswapV2Pair::Swap::SIGNATURE_HASH) => Ok(()),
            _ => {
                trace!("Ignoring unknown event for V2 pool");
                Ok(())
            }
        }
    }
}

impl TopicList for UniswapV2Pool {
    fn topics() -> Vec<FixedBytes<32>> {
        vec![
            IUniswapV2Pair::Swap::SIGNATURE_HASH,
            IUniswapV2Pair::Sync::SIGNATURE_HASH,
            IV2PairUint256::Sync::SIGNATURE_HASH,
        ]
    }

    fn profitable_topics() -> Vec<FixedBytes<32>> {
        vec![IUniswapV2Pair::Swap::SIGNATURE_HASH]
    }
}

impl fmt::Display for UniswapV2Pool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "V2 Pool {} - {} <> {} (reserves: {}, {})",
            self.address, self.token0, self.token1, self.reserve0, self.reserve1
        )
    }
}

impl PoolTypeTrait for UniswapV2Pool {
    fn pool_type(&self) -> PoolType {
        PoolType::UniswapV2
    }
}

/// Fetches pool data for a V2 pool
pub async fn fetch_v2_pool<P: Provider + Send + Sync>(
    provider: &Arc<P>,
    pool_address: Address,
    block_number: BlockId,
    token_registry: &Arc<RwLock<TokenRegistry>>,
    multicall_address: Address,
    factory_to_fee: &HashMap<String, u64>,
    aero_factories: &Vec<Address>,
) -> Result<UniswapV2Pool> {
    let pair_instance = IUniswapV2Pair::new(pool_address, &provider);
    let uint256_pair_instance = IV2PairUint256::new(pool_address, &provider);
    let volatile_stable_fee_in_factory_instance =
        VolatileStableFeeInFactory::new(pool_address, &provider);
    let volatile_stable_get_fee_instance = VolatileStableGetFee::new(pool_address, &provider);
    let multicall_result = provider
        .multicall()
        .address(multicall_address)
        .add(pair_instance.token0()) // 0
        .add(pair_instance.token1()) // 1
        .add(pair_instance.getReserves()) // 2
        .add(pair_instance.factory()) // 3
        .add(pair_instance.fee()) // 4
        .add(uint256_pair_instance.getReserves()) // 5
        .add(volatile_stable_fee_in_factory_instance.stable()) // 6
        .add(volatile_stable_get_fee_instance.getFee()) // 7
        .add(volatile_stable_get_fee_instance.isStable()) // 8
        .add(pair_instance.swapFee()) // 9
        .block(block_number)
        .try_aggregate(false)
        .await?;
    // Volatile Stable Fee In Factory: https://blockscout.lisk.com/address/0x10499d88Bd32AF443Fc936F67DE32bE1c8Bb374C?tab=contract_abi
    // Volatile Stable Get Fee: https://explorer.zircuit.com/address/0x951184b8adcf92d6e3594abe0cdcf21b46704412?activeTab=3

    // Tokens
    let (token0_address, token1_address) =
        (multicall_result.0.unwrap(), multicall_result.1.unwrap());
    // Factory
    let mut factory = multicall_result.3.unwrap_or(Address::ZERO);
    // Reserves
    let (reserve0, reserve1) = if let Ok(reserves_result) = multicall_result.2 {
        (
            U256::from(reserves_result._reserve0),
            U256::from(reserves_result._reserve1),
        )
    } else if let Ok(reserves_result) = multicall_result.5 {
        (reserves_result._reserve0, reserves_result._reserve1)
    } else {
        return Err(anyhow!("Failed to get reserves"));
    };

    // Is Stable
    let is_stable = if let Ok(is_stable_result) = multicall_result.6 {
        is_stable_result
    } else if let Ok(is_stable_result) = multicall_result.8 {
        is_stable_result
    } else {
        false
    };

    // Fee
    let fee = if let Ok(mut fee_result) = multicall_result.4 {
        if fee_result.gt(&U256::from(REVERSE_FEE_MAX)) {
            fee_result = U256::from(GET_FEE_MAX) - fee_result;
        }
        U256::from(fee_result * U256::from(GET_FEE_MULTIPLIER))
    } else if let Ok(fee_result) = multicall_result.7 {
        U256::from(fee_result * U256::from(GET_FEE_MULTIPLIER))
    } else if let Ok(fee_result) = multicall_result.9 {
        U256::from(U256::from(fee_result) * U256::from(GET_FEE_MULTIPLIER))
    } else {
        factory = if !factory.is_zero() {
            factory
        } else {
            let factory_storage = provider
                .get_storage_at(pool_address, U256::from(FACTORY_STORAGE_SLOT))
                .await?;
            let factory = Address::from(U160::from(factory_storage));
            info!("Pool factory from storage: {}", factory);
            factory
        };
        // Try to get fee from factory to fee map
        match factory_to_fee.get(&factory.to_string()) {
            Some(fee) => U256::from(*fee),
            None => match get_v2_factory_fee(&factory) {
                // Keep this for backward compatibility with old config
                Ok(fee) => fee,
                Err(_) => {
                    let fee = if let Some(fee) = get_v2_fee_from_factory(
                        provider,
                        factory,
                        pool_address,
                        is_stable,
                        multicall_address,
                        block_number,
                    )
                    .await
                    {
                        fee
                    } else {
                        // If factory is not in factory to fee map try get pool from aero map
                        let mut multicall =
                            MulticallBuilder::new_dynamic(provider).address(multicall_address);
                        for factory in aero_factories {
                            let factory_instance = IVeloPoolFactory::new(*factory, &provider);
                            multicall = multicall.add_dynamic(factory_instance.getPair(
                                token0_address,
                                token1_address,
                                is_stable,
                            ));
                        }

                        let results = multicall.block(block_number).try_aggregate(false).await?;
                        let mut fee_found = None;
                        for (i, result) in results.into_iter().enumerate() {
                            if let Ok(pool_address_result) = result {
                                if pool_address_result.eq(&pool_address) {
                                    if let Some(fee) = get_v2_fee_from_factory(
                                        provider,
                                        *aero_factories.get(i).unwrap(),
                                        pool_address,
                                        is_stable,
                                        multicall_address,
                                        block_number,
                                    )
                                    .await
                                    {
                                        fee_found = Some(fee);
                                        factory = *aero_factories.get(i).unwrap();
                                        info!("Found Aero factory: {}", factory);
                                        break;
                                    }
                                };
                            }
                        }
                        if let Some(fee) = fee_found {
                            fee
                        } else {
                            info!("No Aero factory matched, using default factory fee");
                            default_factory_fee_by_chain_id(
                                provider.get_chain_id().await?,
                                &factory,
                            )?
                        }
                    };

                    fee
                }
            },
        }
    };

    // Pool type
    let pool_type = if is_stable {
        info!("Pool is stable");
        V2PoolType::Stable
    } else {
        V2PoolType::UniswapV2
    };

    // Create token objects (you'll need to fetch token details)
    let (token0, decimals0) =
        get_or_fetch_token(token_registry, provider, token0_address, multicall_address).await?;
    let (token1, decimals1) =
        get_or_fetch_token(token_registry, provider, token1_address, multicall_address).await?;
    // Create and return V2 pool
    info!(
        "{} Token0: {}, Token1: {}, Fee: {}, Factory: {}",
        if is_stable { "Stable Pool" } else { "V2 Pool" },
        token0,
        token1,
        fee,
        factory,
    );
    Ok(UniswapV2Pool::new(
        pool_type,
        pool_address,
        token0,
        token1,
        decimals0,
        decimals1,
        reserve0,
        reserve1,
        fee,
    ))
}

async fn get_v2_fee_from_factory<P: Provider + Send + Sync>(
    provider: &Arc<P>,
    factory: Address,
    pool_address: Address,
    is_stable: bool,
    multicall_address: Address,
    block_number: BlockId,
) -> Option<U256> {
    // If factory is not in factory to fee map, try to get fee from factory
    let factory_get_fee_pool_instance = UniswapV2FactoryGetFeePool::new(factory, &provider);
    let factory_pair_fee_instance = UniswapV2FactoryPairFee::new(factory, &provider);
    let factory_get_fee_only_pair_instance =
        UniswapV2FactoryGetFeeOnlyPair::new(factory, &provider);

    let multicall_result = provider
        .multicall()
        .address(multicall_address)
        .add(factory_get_fee_pool_instance.getFee(pool_address, is_stable)) // 0
        .add(factory_pair_fee_instance.getFee(is_stable)) // 1
        .add(factory_get_fee_only_pair_instance.getFee(pool_address)) // 2
        .block(block_number)
        .try_aggregate(false)
        .await
        .ok()?;

    let fee = if let Ok(fee_result) = multicall_result.0 {
        Some(U256::from(fee_result * U256::from(GET_FEE_MULTIPLIER)))
    } else if let Ok(fee_result) = multicall_result.1 {
        Some(U256::from(fee_result * U256::from(GET_FEE_MULTIPLIER)))
    } else if let Ok(fee_result) = multicall_result.2 {
        Some(U256::from(fee_result * U256::from(GET_FEE_MULTIPLIER)))
    } else {
        None
    };

    fee
}
