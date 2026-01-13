use crate::blockchain::{
    get_or_fetch_token, AlgebraPoolFeeInState, AlgebraTwoSideFee, AlgebraV3Pool, CLPPool,
    IAlgebraPoolSei, IPancakeV3Pool, IQuoter, IUniswapV3Pool,
};
use crate::models::pool::base::{
    EventApplicable, PoolInterface, PoolType, PoolTypeTrait, TopicList,
};
use crate::models::pool::v3::{get_ramses_quoter, is_ramses_factory, MAX_TICK_I32, MIN_TICK_I32};
use crate::services::Database;
use alloy::primitives::{aliases::U24, Address, Signed, U160, U256};
use alloy::primitives::{FixedBytes, Uint, U128};
use alloy::rpc::types::Log;
use alloy::sol_types::SolEvent;
use alloy::{
    eips::BlockId,
    providers::{MulticallBuilder, Provider},
};
use anyhow::{anyhow, Result};
use log::{debug, error, info, trace};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::sync::Arc;
use std::{collections::BTreeMap, fmt};

use tokio::sync::RwLock;

use super::{v3_swap, Tick, TickMap};
use crate::models::token::TokenRegistry;

/// The Q64.96 precision used by Uniswap V3
pub const Q96_U128: u128 = 1 << 96;
pub const FEE_DENOMINATOR: u32 = 1000000;
pub const RAMSES_FACTOR: u128 = 10000000000;
pub const CHUNK_SIZE: usize = 250;

/// Enum representing the type of V3 pool
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum V3PoolType {
    UniswapV3,
    PancakeV3,
    AlgebraV3,
    RamsesV2,
    AlgebraTwoSideFee,
    AlgebraPoolFeeInState,
}

/// Struct containing V3 pool information including tick data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV3Pool {
    /// Pool type
    pub pool_type: V3PoolType,
    /// Pool address
    pub address: Address,
    /// First token address in the pool
    pub token0: Address,
    /// Second token address in the pool
    pub token1: Address,
    /// Fee tier in the pool in basis points 1000000 = 100%
    pub fee: U24,
    /// Tick spacing for this pool
    pub tick_spacing: i32,
    /// Current sqrt price (sqrt(token1/token0)) * 2^96
    pub sqrt_price_x96: U160,
    /// Current tick
    pub tick: i32,
    /// Current liquidity
    pub liquidity: u128,
    /// Mapping of initialized ticks
    pub ticks: TickMap,
    /// Ratio conversion factor
    pub ratio_conversion_factor: U256,
    /// Factory address
    pub factory: Address,
    /// Last update timestamp
    pub last_updated: u64,
    /// Creation timestamp or block
    pub created_at: u64,
}

impl UniswapV3Pool {
    /// Create a new V3 pool
    pub fn new(
        address: Address,
        token0: Address,
        token1: Address,
        fee: U24,
        tick_spacing: i32,
        sqrt_price_x96: U160,
        tick: i32,
        liquidity: u128,
        factory: Address,
        pool_type: V3PoolType,
    ) -> Self {
        let current_time = chrono::Utc::now().timestamp() as u64;
        Self {
            pool_type,
            address,
            token0,
            token1,
            fee,
            tick_spacing,
            sqrt_price_x96,
            tick,
            liquidity,
            ticks: BTreeMap::new(),
            last_updated: current_time,
            created_at: current_time,
            ratio_conversion_factor: U256::from(RAMSES_FACTOR),
            factory,
        }
    }

    pub fn update_ratio_conversion_factor(&mut self, factor: U256) {
        self.ratio_conversion_factor = factor;
    }

    /// Update pool state based on swap event
    pub fn update_state(&mut self, sqrt_price_x96: U160, tick: i32, liquidity: u128) -> Result<()> {
        if sqrt_price_x96 == U160::ZERO {
            return Err(anyhow!("Invalid sqrt_price_x96: zero"));
        }
        // Validate tick range (Uniswap V3 tick bounds: -887272 to 887272)
        if tick < -887272 || tick > 887272 {
            return Err(anyhow!("Invalid tick: {} out of bounds", tick));
        }
        // let old_tick = self.tick;
        self.sqrt_price_x96 = sqrt_price_x96;
        self.tick = tick;
        self.liquidity = liquidity;
        self.last_updated = chrono::Utc::now().timestamp() as u64;
        // Update liquidity for any ticks crossed
        // self.update_liquidity_for_tick_range(old_tick, tick)?;
        Ok(())
    }

    /// Add or update a tick
    pub fn update_tick(
        &mut self,
        index: i32,
        liquidity_net: i128,
        liquidity_gross: u128,
    ) -> Result<()> {
        if liquidity_gross == 0 {
            // Remove tick if liquidity_gross is zero
            self.ticks.remove(&index);
        } else {
            let tick = Tick {
                index,
                liquidity_net,
                liquidity_gross,
            };
            self.ticks.insert(index, tick);
        }
        Ok(())
    }

    /// Get the price of token1 in terms of token0 from sqrt_price_x96
    pub fn get_price_from_sqrt_price(&self) -> Result<f64> {
        // Convert sqrtPriceX96 to a human-readable price
        // Price = (sqrtPriceX96 / 2^96)^2
        let sqrt_price: f64 = self.sqrt_price_x96.to::<u128>() as f64 / Q96_U128 as f64;
        Ok(sqrt_price * sqrt_price)
    }

    /// Calculate the amount of token1 for a given amount of token0
    fn calculate_zero_for_one(&self, amount: U256, is_exact_input: bool) -> Result<U256> {
        let amount_specified = if is_exact_input {
            Signed::from_raw(amount)
        } else {
            Signed::from_raw(amount).saturating_neg()
        };
        let swap_state = v3_swap(
            self.fee,
            self.sqrt_price_x96,
            self.tick,
            self.liquidity,
            &self.ticks,
            true,
            amount_specified,
            None,
        )?;
        Ok(swap_state.amount_calculated.abs().into_raw())
    }

    /// Calculate the amount of token0 for a given amount of token1 (exact input)
    fn calculate_one_for_zero(&self, amount: U256, is_exact_input: bool) -> Result<U256> {
        let amount_specified = if is_exact_input {
            Signed::from_raw(amount)
        } else {
            Signed::from_raw(amount).saturating_neg()
        };
        let swap_state = v3_swap(
            self.fee,
            self.sqrt_price_x96,
            self.tick,
            self.liquidity,
            &self.ticks,
            false,
            amount_specified,
            None,
        )?;
        Ok(swap_state.amount_calculated.abs().into_raw())
    }

    /// Get the adjacent initialized ticks for a given tick
    pub fn get_adjacent_ticks(&self, tick: i32) -> (Option<&Tick>, Option<&Tick>) {
        let below = self.ticks.range(..tick).next_back().map(|(_, tick)| tick);
        let above = self.ticks.range(tick..).next().map(|(_, tick)| tick);
        (below, above)
    }

    /// Check if the pool has sufficient liquidity
    pub fn has_sufficient_liquidity(&self) -> bool {
        self.liquidity != 0 && !self.ticks.is_empty()
    }

    /// Calculate the amount out for a swap with the exact formula
    pub fn calculate_exact_input(&self, token_in: &Address, amount_in: U256) -> Result<U256> {
        let result;
        if token_in == &self.token0 {
            result = self.calculate_zero_for_one(amount_in, true)?;
        } else if token_in == &self.token1 {
            result = self.calculate_one_for_zero(amount_in, true)?;
        } else {
            return Err(anyhow!("Token not in pool"));
        }
        if self.pool_type == V3PoolType::RamsesV2 {
            Ok(result * self.ratio_conversion_factor / U256::from(RAMSES_FACTOR))
        } else {
            Ok(result)
        }
    }

    /// Calculate the amount out for a swap with the exact formula
    pub fn calculate_exact_output(&self, token_out: &Address, amount_in: U256) -> Result<U256> {
        if token_out == &self.token0 {
            self.calculate_one_for_zero(amount_in, false)
        } else if token_out == &self.token1 {
            self.calculate_zero_for_one(amount_in, false)
        } else {
            Err(anyhow!("Token not in pool"))
        }
    }

    /// Apply a swap to the pool, updating the internal state
    fn apply_swap_internal(
        &mut self,
        token_in: &Address,
        _amount_in: U256,
        _amount_out: U256,
    ) -> Result<()> {
        // In a real implementation, we would update internal state based on the swap
        self.last_updated = chrono::Utc::now().timestamp() as u64;

        // Verify token
        if !self.contains_token(token_in) {
            return Err(anyhow!("Token not in pool"));
        }

        // TODO: Implement this
        Ok(())
    }

    /// Update liquidity based on ticks crossed between old and new tick
    #[allow(dead_code)]
    fn update_liquidity_for_tick_range(&mut self, old_tick: i32, new_tick: i32) -> Result<()> {
        if old_tick == new_tick {
            return Ok(());
        }

        // Determine direction of tick change
        if new_tick > old_tick {
            // Moving up in price (0->1)
            // Add liquidity_net when crossing ticks upward
            for tick_idx in self
                .ticks
                .range((old_tick + 1)..=new_tick)
                .map(|(k, _)| *k)
                .collect::<Vec<i32>>()
            {
                if let Some(tick) = self.ticks.get(&tick_idx) {
                    self.liquidity = self.liquidity.saturating_add(tick.liquidity_net as u128);
                }
            }
        } else {
            // Moving down in price (1->0)
            // Subtract liquidity_net when crossing ticks downward
            for tick_idx in self
                .ticks
                .range((new_tick + 1)..=old_tick)
                .map(|(k, _)| *k)
                .collect::<Vec<i32>>()
            {
                if let Some(tick) = self.ticks.get(&tick_idx) {
                    self.liquidity = self.liquidity.saturating_sub(tick.liquidity_net as u128);
                }
            }
        }

        Ok(())
    }

    /// Convert a tick to its corresponding word index in the tick bitmap
    pub fn tick_to_word(&self, tick: i32) -> i32 {
        let compressed = tick / self.tick_spacing;
        let compressed = if tick < 0 && tick % self.tick_spacing != 0 {
            compressed - 1
        } else {
            compressed
        };
        compressed >> 8
    }

    /// Save pool data to database
    pub fn save_to_db(&self, db: &Database) -> Result<()> {
        let key = self.address.to_string();
        db.insert("v3_pools", key, self)?;
        debug!("Saved V3 pool {} to database", self.address);
        Ok(())
    }

    /// Load pool data from database
    pub fn load_from_db(db: &Database, address: &Address) -> Result<Option<Self>> {
        let key = address.to_string();
        let pool = db.get::<_, Self>("v3_pools", key)?;
        if let Some(ref _loaded_pool) = pool {
            debug!("Loaded V3 pool {} from database", address);
        }
        Ok(pool)
    }

    /// Load all V3 pools from database
    pub fn load_all_from_db(db: &Database) -> Result<Vec<Self>> {
        let mut pools = Vec::new();
        let iter = db.iter::<Self>("v3_pools")?;

        for result in iter {
            match result {
                Ok((_, pool)) => pools.push(pool),
                Err(e) => error!("Error loading V3 pool: {}", e),
            }
        }

        info!("Loaded {} V3 pools from database", pools.len());
        Ok(pools)
    }
}

impl PoolInterface for UniswapV3Pool {
    fn calculate_output(&self, token_in: &Address, amount_in: U256) -> Result<U256> {
        self.calculate_exact_input(token_in, amount_in)
    }

    fn calculate_input(&self, token_out: &Address, amount_out: U256) -> Result<U256> {
        self.calculate_exact_output(token_out, amount_out)
    }

    fn apply_swap(&mut self, token_in: &Address, amount_in: U256, amount_out: U256) -> Result<()> {
        self.apply_swap_internal(token_in, amount_in, amount_out)
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
        format!(
            "v3-{}-{}-{}-{}",
            self.address,
            self.token0,
            self.token1,
            self.fee.to::<u128>()
        )
    }

    fn log_summary(&self) -> String {
        format!(
            "V3 Pool {} - {} <> {} (fee: {:.2}%, tick: {}, liquidity: {}, sqrt_price_x96: {}, ticks: {})",
            self.address, self.token0, self.token1, self.fee, self.tick, self.liquidity, self.sqrt_price_x96, self.ticks.len()
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

impl EventApplicable for UniswapV3Pool {
    fn apply_log(&mut self, log: &Log) -> Result<()> {
        match log.topic0() {
            Some(&IUniswapV3Pool::Swap::SIGNATURE_HASH) => {
                let swap_data: IUniswapV3Pool::Swap = log.log_decode()?.inner.data;
                debug!(
                    "Applying V3Swap event to pool {}: sqrt_price_x96={}, tick={}, liquidity={}",
                    self.address, swap_data.sqrtPriceX96, swap_data.tick, swap_data.liquidity
                );
                self.update_state(
                    swap_data.sqrtPriceX96,
                    swap_data.tick.as_i32(),
                    swap_data.liquidity,
                )
            }
            Some(&IPancakeV3Pool::Swap::SIGNATURE_HASH) => {
                let swap_data: IPancakeV3Pool::Swap = log.log_decode()?.inner.data;
                debug!(
                    "Applying V3Swap event to pool {}: sqrt_price_x96={}, tick={}, liquidity={}",
                    self.address, swap_data.sqrtPriceX96, swap_data.tick, swap_data.liquidity
                );
                self.update_state(
                    swap_data.sqrtPriceX96,
                    swap_data.tick.as_i32(),
                    swap_data.liquidity,
                )
            }
            Some(&IAlgebraPoolSei::Swap::SIGNATURE_HASH) => {
                let swap_data: IAlgebraPoolSei::Swap = log.log_decode()?.inner.data;
                debug!(
                    "Applying AlgebraSwap event to pool {}: sqrt_price_x96={}, tick={}, liquidity={}",
                    self.address, swap_data.price, swap_data.tick, swap_data.liquidity
                );
                self.update_state(
                    swap_data.price,
                    swap_data.tick.as_i32(),
                    swap_data.liquidity,
                )
            }
            Some(&IUniswapV3Pool::Mint::SIGNATURE_HASH) => {
                let mint_data: IUniswapV3Pool::Mint = log.log_decode()?.inner.data;
                debug!(
                    "Applying V3Mint event to pool {}: tick_lower={}, tick_upper={}, amount={}",
                    self.address, mint_data.tickLower, mint_data.tickUpper, mint_data.amount
                );

                let amount_u128 = mint_data.amount;
                let tick_lower_i32 = mint_data.tickLower.as_i32();
                let tick_upper_i32 = mint_data.tickUpper.as_i32();

                // Validate tick range
                if tick_lower_i32 >= tick_upper_i32 {
                    return Err(anyhow!(
                        "Invalid tick range: tick_lower {} >= tick_upper {}",
                        tick_lower_i32,
                        tick_upper_i32
                    ));
                }

                // Update tick_lower
                if let Some(tick) = self.ticks.get_mut(&tick_lower_i32) {
                    tick.liquidity_net = tick.liquidity_net.saturating_add(amount_u128 as i128);
                    tick.liquidity_gross = tick.liquidity_gross.saturating_add(amount_u128);
                } else {
                    self.update_tick(tick_lower_i32, amount_u128 as i128, amount_u128)?;
                }

                // Update tick_upper
                if let Some(tick) = self.ticks.get_mut(&tick_upper_i32) {
                    tick.liquidity_net = tick.liquidity_net.saturating_sub(amount_u128 as i128);
                    tick.liquidity_gross = tick.liquidity_gross.saturating_add(amount_u128);
                } else {
                    self.update_tick(tick_upper_i32, -(amount_u128 as i128), amount_u128)?;
                }

                // Update pool liquidity if current tick is in range [tick_lower, tick_upper)
                if self.tick >= tick_lower_i32 && self.tick < tick_upper_i32 {
                    self.liquidity = self.liquidity.saturating_add(amount_u128);
                }

                Ok(())
            }
            Some(&IUniswapV3Pool::Burn::SIGNATURE_HASH) => {
                let burn_data: IUniswapV3Pool::Burn = log.log_decode()?.inner.data;
                debug!(
                    "Applying V3Burn event to pool {}: tick_lower={}, tick_upper={}, amount={}",
                    self.address, burn_data.tickLower, burn_data.tickUpper, burn_data.amount
                );

                let amount_u128 = burn_data.amount;
                let tick_lower_i32 = burn_data.tickLower.as_i32();
                let tick_upper_i32 = burn_data.tickUpper.as_i32();

                // Validate tick range
                if tick_lower_i32 >= tick_upper_i32 {
                    return Err(anyhow!(
                        "Invalid tick range: tick_lower {} >= tick_upper {}",
                        tick_lower_i32,
                        tick_upper_i32
                    ));
                }

                // Update tick_lower
                if let Some(tick) = self.ticks.get_mut(&tick_lower_i32) {
                    let liquidity_net = tick.liquidity_net;
                    tick.liquidity_net = tick.liquidity_net.saturating_sub(amount_u128 as i128);
                    tick.liquidity_gross = tick.liquidity_gross.saturating_sub(amount_u128);
                    if tick.liquidity_gross == 0 {
                        self.update_tick(tick_lower_i32, liquidity_net, 0)?;
                    }
                } else {
                    return Err(anyhow!(
                        "Burn attempted on uninitialized tick_lower: {}",
                        tick_lower_i32
                    ));
                }

                // Update tick_upper
                if let Some(tick) = self.ticks.get_mut(&tick_upper_i32) {
                    let liquidity_net = tick.liquidity_net;
                    tick.liquidity_net = tick.liquidity_net.saturating_add(amount_u128 as i128);
                    tick.liquidity_gross = tick.liquidity_gross.saturating_sub(amount_u128);
                    if tick.liquidity_gross == 0 {
                        self.update_tick(tick_upper_i32, liquidity_net, 0)?;
                    }
                } else {
                    return Err(anyhow!(
                        "Burn attempted on uninitialized tick_upper: {}",
                        tick_upper_i32
                    ));
                }

                // Update pool liquidity if current tick is in range [tick_lower, tick_upper)
                if self.tick >= tick_lower_i32 && self.tick < tick_upper_i32 {
                    self.liquidity = self.liquidity.saturating_sub(amount_u128);
                }

                Ok(())
            }
            Some(&IAlgebraPoolSei::Burn::SIGNATURE_HASH) => {
                let burn_data: IAlgebraPoolSei::Burn = log.log_decode()?.inner.data;
                debug!(
                    "Applying V3Burn event to pool {}: tick_lower={}, tick_upper={}, amount={}",
                    self.address,
                    burn_data.bottomTick,
                    burn_data.topTick,
                    burn_data.liquidityAmount
                );

                let amount_u128 = burn_data.liquidityAmount;
                let tick_lower_i32 = burn_data.bottomTick.as_i32();
                let tick_upper_i32 = burn_data.topTick.as_i32();

                // Validate tick range
                if tick_lower_i32 >= tick_upper_i32 {
                    return Err(anyhow!(
                        "Invalid tick range: tick_lower {} >= tick_upper {}",
                        tick_lower_i32,
                        tick_upper_i32
                    ));
                }

                // Update tick_lower
                if let Some(tick) = self.ticks.get_mut(&tick_lower_i32) {
                    let liquidity_net = tick.liquidity_net;
                    tick.liquidity_net = tick.liquidity_net.saturating_sub(amount_u128 as i128);
                    tick.liquidity_gross = tick.liquidity_gross.saturating_sub(amount_u128);
                    if tick.liquidity_gross == 0 {
                        self.update_tick(tick_lower_i32, liquidity_net, 0)?;
                    }
                } else {
                    return Err(anyhow!(
                        "Burn attempted on uninitialized tick_lower: {}",
                        tick_lower_i32
                    ));
                }

                // Update tick_upper
                if let Some(tick) = self.ticks.get_mut(&tick_upper_i32) {
                    let liquidity_net = tick.liquidity_net;
                    tick.liquidity_net = tick.liquidity_net.saturating_add(amount_u128 as i128);
                    tick.liquidity_gross = tick.liquidity_gross.saturating_sub(amount_u128);
                    if tick.liquidity_gross == 0 {
                        self.update_tick(tick_upper_i32, liquidity_net, 0)?;
                    }
                } else {
                    return Err(anyhow!(
                        "Burn attempted on uninitialized tick_upper: {}",
                        tick_upper_i32
                    ));
                }

                // Update pool liquidity if current tick is in range [tick_lower, tick_upper)
                if self.tick >= tick_lower_i32 && self.tick < tick_upper_i32 {
                    self.liquidity = self.liquidity.saturating_sub(amount_u128);
                }

                Ok(())
            }

            _ => {
                trace!("Ignoring non-V3 event for V3 pool");
                Ok(())
            }
        }
    }
}

impl TopicList for UniswapV3Pool {
    fn topics() -> Vec<FixedBytes<32>> {
        vec![
            IUniswapV3Pool::Swap::SIGNATURE_HASH,
            IUniswapV3Pool::Mint::SIGNATURE_HASH,
            IUniswapV3Pool::Burn::SIGNATURE_HASH,
            IPancakeV3Pool::Swap::SIGNATURE_HASH,
            IAlgebraPoolSei::Swap::SIGNATURE_HASH,
            IAlgebraPoolSei::Burn::SIGNATURE_HASH,
        ]
    }

    fn profitable_topics() -> Vec<FixedBytes<32>> {
        vec![
            IUniswapV3Pool::Swap::SIGNATURE_HASH,
            IPancakeV3Pool::Swap::SIGNATURE_HASH,
            IAlgebraPoolSei::Swap::SIGNATURE_HASH,
        ]
    }
}

impl fmt::Display for UniswapV3Pool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "V3 Pool {} - {} <> {} (fee: {:.2}%, tick: {}, liquidity: {})",
            self.address,
            self.token0,
            self.token1,
            (self.fee.to::<u128>() as f64 / FEE_DENOMINATOR as f64) * 100.0,
            self.tick,
            self.liquidity
        )
    }
}

impl PoolTypeTrait for UniswapV3Pool {
    fn pool_type(&self) -> PoolType {
        PoolType::UniswapV3
    }
}

/// Fetches pool data for a V3 pool
pub async fn fetch_v3_pool<P: Provider + Send + Sync>(
    provider: &Arc<P>,
    pool_address: Address,
    block_number: BlockId,
    token_registry: &Arc<RwLock<TokenRegistry>>,
    multicall_address: Address,
) -> Result<UniswapV3Pool> {
    let mut v3_pool_type = V3PoolType::UniswapV3;
    let uniswapv3_pool_instance = IUniswapV3Pool::new(pool_address, &provider);
    let clp_pool_instance = CLPPool::new(pool_address, &provider);
    let algebra_v3_pool_instance = AlgebraV3Pool::new(pool_address, &provider);
    let algebra_two_side_fee_pool_instance = AlgebraTwoSideFee::new(pool_address, &provider);
    let algebra_pool_fee_in_state_instance = AlgebraPoolFeeInState::new(pool_address, &provider);

    let multicall_result = provider
        .multicall()
        .address(multicall_address)
        .add(uniswapv3_pool_instance.token0()) // 0
        .add(uniswapv3_pool_instance.token1()) // 1
        .add(uniswapv3_pool_instance.fee()) // 2
        .add(uniswapv3_pool_instance.tickSpacing()) // 3
        .add(uniswapv3_pool_instance.slot0()) // 4
        .add(uniswapv3_pool_instance.liquidity()) // 5
        .add(uniswapv3_pool_instance.factory()) // 6
        .add(clp_pool_instance.slot0()) // this does not has feeProtocol param // 7
        .add(algebra_v3_pool_instance.fee()) // 8
        .add(algebra_v3_pool_instance.globalState()) // 9
        .add(algebra_two_side_fee_pool_instance.globalState()) // 10
        .add(algebra_two_side_fee_pool_instance.activeIncentive()) // 11
        .add(algebra_pool_fee_in_state_instance.globalState()) // 12
        .block(block_number)
        .try_aggregate(false)
        .await?;
    let (token0, token1, fee, tick_spacing, sqrt_price_x96, tick, liquidity, factory) =
        if let Ok(slot0_result) = multicall_result.7 {
            (
                multicall_result.0.unwrap(),
                multicall_result.1.unwrap(),
                multicall_result.2.unwrap(),
                multicall_result.3.unwrap(),
                slot0_result.sqrtPriceX96,
                slot0_result.tick,
                multicall_result.5.unwrap(),
                multicall_result.6.unwrap(),
            )
        } else if let Ok(slot0_result) = multicall_result.12 {
            let fee: U24 = U24::from(slot0_result.fee);
            v3_pool_type = V3PoolType::AlgebraPoolFeeInState; // AlgebraTwoSideFee
            (
                multicall_result.0.unwrap(),
                multicall_result.1.unwrap(),
                fee,
                multicall_result.3.unwrap(),
                slot0_result.price,
                slot0_result.tick,
                multicall_result.5.unwrap(),
                multicall_result.6.unwrap(),
            )
        } else if let Ok(_) = multicall_result.11 {
            let slot0_result = multicall_result.10.unwrap();
            let fee: U24 = if slot0_result.feeZto > slot0_result.feeOtz {
                U24::from(slot0_result.feeZto)
            } else {
                U24::from(slot0_result.feeOtz)
            };
            v3_pool_type = V3PoolType::AlgebraTwoSideFee;
            (
                multicall_result.0.unwrap(),
                multicall_result.1.unwrap(),
                fee,
                multicall_result.3.unwrap(),
                slot0_result.price,
                slot0_result.tick,
                multicall_result.5.unwrap(),
                multicall_result.6.unwrap(),
            )
        } else if let Ok(slot0_result) = multicall_result.9 {
            v3_pool_type = V3PoolType::AlgebraV3;
            (
                multicall_result.0.unwrap(),
                multicall_result.1.unwrap(),
                U24::from(multicall_result.8.unwrap()),
                multicall_result.3.unwrap(),
                slot0_result.price,
                slot0_result.tick,
                multicall_result.5.unwrap(),
                multicall_result.6.unwrap(),
            )
        } else {
            let slot0_result = multicall_result.4.unwrap();
            let factory = multicall_result.6.unwrap();
            if is_ramses_factory(factory) {
                v3_pool_type = V3PoolType::RamsesV2; // UniswapV3Pool
            }
            (
                multicall_result.0.unwrap(),
                multicall_result.1.unwrap(),
                multicall_result.2.unwrap(),
                multicall_result.3.unwrap(),
                slot0_result.sqrtPriceX96,
                slot0_result.tick,
                multicall_result.5.unwrap(),
                factory,
            )
        };

    // Create token objects (you'll need to fetch token details)
    let (token0, _) =
        get_or_fetch_token(token_registry, provider, token0, multicall_address).await?;
    let (token1, _) =
        get_or_fetch_token(token_registry, provider, token1, multicall_address).await?;

    info!(
        "V3 Pool {:?}: Token0: {}, Token1: {}, Fee: {}, Factory: {}, Tick: {}, Liquidity: {}",
        v3_pool_type, token0, token1, fee, factory, tick, liquidity
    );
    // Create and return V3 pool
    let mut pool = UniswapV3Pool::new(
        pool_address,
        token0,
        token1,
        fee,
        tick_spacing.as_i32(),
        sqrt_price_x96.to::<U160>(),
        tick.as_i32(),
        liquidity,
        factory,
        v3_pool_type,
    );

    if let Err(e) = fetch_v3_ticks(provider, &mut pool, block_number, multicall_address).await {
        error!("Error fetching ticks for pool {}: {}", pool_address, e);
        return Err(e);
    }

    if pool.pool_type == V3PoolType::RamsesV2 {
        let ratio_conversion_factor =
            calculate_ratio_conversion_factor(&pool, provider, block_number).await?;
        info!(
            "Ratio conversion factor: {}",
            ratio_conversion_factor.to::<U128>()
        );
        pool.update_ratio_conversion_factor(ratio_conversion_factor);
    }

    Ok(pool)
}

/// Fetches tick data for a V3 pool
pub async fn fetch_v3_ticks<P: Provider + Send + Sync>(
    provider: &Arc<P>,
    pool: &mut UniswapV3Pool,
    block_number: BlockId,
    multicall_address: Address,
) -> Result<()> {
    let mut tick_indices = Vec::new();

    match pool.pool_type {
        V3PoolType::UniswapV3 | V3PoolType::RamsesV2 | V3PoolType::PancakeV3 => {
            // Fetch word bitmap
            let min_word = pool.tick_to_word(MIN_TICK_I32);
            let max_word = pool.tick_to_word(MAX_TICK_I32);

            // Fetching bitMaps from their position
            let mut word_pos_indices: Vec<i32> = vec![];

            // Split word bitmap fetching into chunks
            let mut all_bitmaps = Vec::new();
            let contract = IUniswapV3Pool::new(pool.address, provider);
            for chunk in (min_word..=max_word).collect::<Vec<_>>().chunks(CHUNK_SIZE) {
                let mut multicall =
                    MulticallBuilder::new_dynamic(provider).address(multicall_address);
                for &word_pos in chunk {
                    word_pos_indices.push(word_pos);
                    multicall = multicall.add_dynamic(contract.tickBitmap(word_pos as i16));
                }
                let results = multicall.block(block_number).aggregate().await?;
                all_bitmaps.extend(results.into_iter().map(|tick_bitmap| tick_bitmap));
            }

            for (j, word_pos) in word_pos_indices.iter().enumerate() {
                let bitmap = all_bitmaps[j];

                if bitmap != U256::ZERO {
                    for i in 0..256 {
                        let bit = U256::from(1u64);
                        let initialized = (bitmap & (bit << i)) != U256::ZERO;

                        if initialized {
                            let tick_index = (word_pos * 256 + i as i32) * pool.tick_spacing;
                            tick_indices.push(tick_index);
                        }
                    }
                }
            }
        }
        V3PoolType::AlgebraV3 => {
            // Algebra V3 approach: navigate through 3-level tree structure
            let contract = AlgebraV3Pool::new(pool.address, provider);
            // Step 1: Fetch the root of the tick tree
            let tick_tree_root: u32 = contract.tickTreeRoot().block(block_number).call().await?;
            if tick_tree_root == 0 {
                // No initialized ticks
                pool.ticks = BTreeMap::new();
                return Ok(());
            }

            // Step 2: Find active second layer indices from root
            let mut second_layer_indices = Vec::new();
            for root_bit in 0..32 {
                if (tick_tree_root & (1 << root_bit)) != 0 {
                    second_layer_indices.push(root_bit as i16);
                }
            }

            // Step 3: Fetch second layer bitmaps
            let mut second_layer_multicall =
                MulticallBuilder::new_dynamic(provider).address(multicall_address);
            for &second_layer_index in &second_layer_indices {
                second_layer_multicall = second_layer_multicall
                    .add_dynamic(contract.tickTreeSecondLayer(second_layer_index));
            }
            let second_layer_results = second_layer_multicall
                .block(block_number)
                .aggregate()
                .await?;

            // Step 4: Find active tick table indices from second layer
            let mut tick_table_indices = Vec::new();
            const SECOND_LAYER_OFFSET: i16 = 3466; // ceil(-MIN_TICK / 256)

            for (i, &second_layer_index) in second_layer_indices.iter().enumerate() {
                let second_layer_bitmap: U256 = second_layer_results[i];

                if second_layer_bitmap != U256::ZERO {
                    for second_bit in 0..256 {
                        if (second_layer_bitmap & (U256::from(1u64) << second_bit)) != U256::ZERO {
                            // Calculate the tick table index
                            // This is the leaf index in the tree structure
                            let leaf_index = second_layer_index as i32 * 256 + second_bit as i32;
                            let tick_table_index = leaf_index - SECOND_LAYER_OFFSET as i32;
                            tick_table_indices.push(tick_table_index as i16);
                        }
                    }
                }
            }

            // Step 5: Fetch tick table bitmaps (leaf layer)
            let mut tick_table_multicall =
                MulticallBuilder::new_dynamic(provider).address(multicall_address);
            for &tick_table_index in &tick_table_indices {
                tick_table_multicall =
                    tick_table_multicall.add_dynamic(contract.tickTable(tick_table_index));
            }
            let tick_table_results = tick_table_multicall.block(block_number).aggregate().await?;

            // Step 6: Find all initialized tick indices
            for (i, &tick_table_index) in tick_table_indices.iter().enumerate() {
                let tick_table_bitmap: U256 = tick_table_results[i];

                if tick_table_bitmap != U256::ZERO {
                    for tick_bit in 0..256 {
                        if (tick_table_bitmap & (U256::from(1u64) << tick_bit)) != U256::ZERO {
                            // Calculate the actual tick index using bit shift (like in TickLens)
                            // tick = (tickTableIndex << 8) + bitPosition
                            let tick_index = (tick_table_index as i32)
                                .wrapping_mul(256)
                                .wrapping_add(tick_bit as i32);

                            if tick_index >= MIN_TICK_I32 && tick_index <= MAX_TICK_I32 {
                                tick_indices.push(tick_index);
                            }
                        }
                    }
                }
            }
        }
        V3PoolType::AlgebraTwoSideFee | V3PoolType::AlgebraPoolFeeInState => {
            // Algebra Two Side Fee approach: navigate through 3-level tree structure
            // Fetch word bitmap
            let min_word = pool.tick_to_word(MIN_TICK_I32);
            let max_word = pool.tick_to_word(MAX_TICK_I32);

            // Fetching bitMaps from their position
            let mut word_pos_indices: Vec<i32> = vec![];

            // Split word bitmap fetching into chunks
            let mut all_bitmaps = Vec::new();
            let contract = AlgebraTwoSideFee::new(pool.address, provider);
            for chunk in (min_word..=max_word).collect::<Vec<_>>().chunks(CHUNK_SIZE) {
                let mut multicall =
                    MulticallBuilder::new_dynamic(provider).address(multicall_address);
                for &word_pos in chunk {
                    word_pos_indices.push(word_pos);
                    multicall = multicall.add_dynamic(contract.tickTable(word_pos as i16));
                }
                let results = multicall.block(block_number).aggregate().await?;
                all_bitmaps.extend(results.into_iter().map(|tick_bitmap| tick_bitmap));
            }

            for (j, word_pos) in word_pos_indices.iter().enumerate() {
                let bitmap = all_bitmaps[j];

                if bitmap != U256::ZERO {
                    for i in 0..256 {
                        let bit = U256::from(1u64);
                        let initialized = (bitmap & (bit << i)) != U256::ZERO;

                        if initialized {
                            let tick_index = (word_pos * 256 + i as i32) * pool.tick_spacing;
                            tick_indices.push(tick_index);
                        }
                    }
                }
            }
        }
    }

    // Split tick fetching into chunks
    let mut all_ticks: BTreeMap<i32, Tick> = BTreeMap::new();
    match pool.pool_type {
        V3PoolType::UniswapV3 | V3PoolType::RamsesV2 | V3PoolType::PancakeV3 => {
            let contract = IUniswapV3Pool::new(pool.address, provider);
            for chunk in tick_indices.chunks(CHUNK_SIZE) {
                let mut multicall =
                    MulticallBuilder::new_dynamic(provider).address(multicall_address);
                for &tick_index in chunk {
                    multicall = multicall.add_dynamic(
                        contract.ticks(Signed::<24, 1>::try_from(tick_index).unwrap()),
                    );
                }
                let results = multicall.block(block_number).aggregate().await?;
                for (i, tick_index) in chunk.iter().enumerate() {
                    let tick_response = &results[i];
                    let tick = Tick {
                        index: *tick_index,
                        liquidity_gross: tick_response.liquidityGross,
                        liquidity_net: tick_response.liquidityNet,
                    };
                    all_ticks.insert(*tick_index, tick);
                }
            }
        }
        V3PoolType::AlgebraV3
        | V3PoolType::AlgebraTwoSideFee
        | V3PoolType::AlgebraPoolFeeInState => {
            let contract = AlgebraV3Pool::new(pool.address, provider);
            for chunk in tick_indices.chunks(CHUNK_SIZE) {
                let mut multicall =
                    MulticallBuilder::new_dynamic(provider).address(multicall_address);
                for &tick_index in chunk {
                    multicall = multicall.add_dynamic(
                        contract.ticks(Signed::<24, 1>::try_from(tick_index).unwrap()),
                    );
                }
                let results = multicall.block(block_number).aggregate().await?;
                for (i, tick_index) in chunk.iter().enumerate() {
                    let tick_response = &results[i];
                    let tick = Tick {
                        index: *tick_index,
                        liquidity_gross: tick_response.liquidityTotal.to::<U128>().to::<u128>(),
                        liquidity_net: tick_response.liquidityDelta,
                    };
                    all_ticks.insert(*tick_index, tick);
                }
            }
        }
    }
    // println!("all_ticks: {:?}", all_ticks);

    pool.ticks = all_ticks;

    Ok(())
}

pub async fn calculate_ratio_conversion_factor<P: Provider + Send + Sync>(
    pool_v3: &UniswapV3Pool,
    provider: &Arc<P>,
    block_number: BlockId,
) -> Result<U256> {
    let quoter = get_ramses_quoter(pool_v3.factory);
    if let Some(quoter) = quoter {
        let quoter_instance = IQuoter::new(quoter, &provider);
        let amount_in = U256::from(100000000000u64);

        let ratio_conversion_factor_0 = match quoter_instance
            .quoteExactInputSingle(
                pool_v3.token0,
                pool_v3.token1,
                U24::from(pool_v3.fee),
                amount_in,
                Uint::from(0),
            )
            .call()
            .block(block_number)
            .await
        {
            Ok(amount_out_0) => {
                let amount_out_estimate_0 = pool_v3
                    .calculate_output(&pool_v3.token0, amount_in)
                    .unwrap();

                let ratio_conversion_factor_0 = if amount_out_estimate_0 == U256::ZERO {
                    U256::MAX
                } else if amount_out_0 == amount_out_estimate_0 {
                    U256::from(RAMSES_FACTOR)
                } else {
                    amount_out_0 * U256::from(RAMSES_FACTOR) / amount_out_estimate_0 - U256::ONE
                };
                info!("Ratio conversion factor 0: {}", ratio_conversion_factor_0);
                ratio_conversion_factor_0
            }
            Err(_) => {
                info!("Failed to fetch ratio conversion factor 0");
                U256::from(RAMSES_FACTOR)
            }
        };

        let ratio_conversion_factor_1 = match quoter_instance
            .quoteExactInputSingle(
                pool_v3.token1,
                pool_v3.token0,
                U24::from(pool_v3.fee),
                amount_in,
                Uint::from(0),
            )
            .call()
            .block(block_number)
            .await
        {
            Ok(amount_out_1) => {
                let amount_out_estimate_1 = pool_v3
                    .calculate_output(&pool_v3.token1, amount_in)
                    .unwrap();

                let ratio_conversion_factor_1 = if amount_out_estimate_1 == U256::ZERO {
                    U256::MAX
                } else if amount_out_1 == amount_out_estimate_1 {
                    U256::from(RAMSES_FACTOR)
                } else {
                    amount_out_1 * U256::from(RAMSES_FACTOR) / amount_out_estimate_1 - U256::ONE
                };
                info!("Ratio conversion factor 1: {}", ratio_conversion_factor_1);
                ratio_conversion_factor_1
            }
            Err(_) => {
                info!("Failed to fetch ratio conversion factor 1");
                U256::from(RAMSES_FACTOR)
            }
        };

        if ratio_conversion_factor_0 == U256::MAX && ratio_conversion_factor_1 == U256::MAX {
            Ok(U256::from(RAMSES_FACTOR))
        } else {
            Ok(ratio_conversion_factor_0.min(ratio_conversion_factor_1))
        }
    } else {
        Ok(U256::from(RAMSES_FACTOR))
    }
}
