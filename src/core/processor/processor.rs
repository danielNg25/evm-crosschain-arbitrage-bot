use crate::core::{MultichainNetworkRegistry, PoolChange};
use crate::models::path::PoolPath;
use crate::models::profit_token::ProfitTokenRegistry;
use crate::utils::metrics::Metrics;
use crate::PoolInterface;
use alloy::primitives::{Address, U256};
use anyhow::Result;
use dashmap::DashMap;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock; // For parallel iteration
const DEFAULT_MAX_AMOUNT: u128 = 100_000_000_000_000_000_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ContractPoolType {
    Other = 0,
    AerodromeV2Stable = 1,
    UniswapV2 = 2,
    UniswapV3 = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwoSidePath {
    pub anchor_token: Address,
    pub source_chain: PoolPath,
    pub target_chain: PoolPath,
    pub source_chain_id: u64,
    pub target_chain_id: u64,
}

impl fmt::Display for TwoSidePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TwoSidePath: anchor_token: {}\nsource_chain: {:?}\ntarget_chain: {:?}\nsource_chain_id: {}, target_chain_id: {}",
            self.anchor_token,
            self.source_chain,
            self.target_chain,
            self.source_chain_id,
            self.target_chain_id
        )
    }
}

impl TwoSidePath {
    pub fn hash(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.source_chain.hash(&mut hasher);
        self.target_chain.hash(&mut hasher);
        self.source_chain_id.hash(&mut hasher);
        self.target_chain_id.hash(&mut hasher);
        hasher.finish().to_string()
    }
}

#[derive(Clone)]
pub struct Opportunity {
    pub path: TwoSidePath,
    pub block_number: u64,
    pub profit: f64,
    pub amount_in: U256,
    pub amount_out: U256,
    pub anchor_token_amount: U256,
    pub steps: Vec<U256>,
    pub min_profit_usd: f64,
}

/// Configuration for the simulator
#[derive(Clone)]
pub struct ProcessorConfig {
    /// Maximum number of iterations for binary search
    pub max_iterations: u32,
    /// Maximum amount of usd to swap
    pub max_amount_usd: f64,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            max_amount_usd: 10000.0,
        }
    }
}

// This type definition and struct have been removed as they're no longer needed
// The functionality is now handled by:
// 1. token_max_outputs (FxHashMap) for early cut-off
// 2. memo_cache for caching calculations

pub struct CrossChainArbitrageProcessor {
    network_registries: Arc<MultichainNetworkRegistry>,
    executing_paths: Arc<DashMap<String, String>>,
    config: ProcessorConfig, // Added config field
    opportunity_tx: Arc<Sender<Opportunity>>,
    _metrics: Arc<RwLock<Metrics>>,
}

impl CrossChainArbitrageProcessor {
    pub fn new(
        network_registries: Arc<MultichainNetworkRegistry>,
        executing_paths: Arc<DashMap<String, String>>,
        config: ProcessorConfig, // Added config parameter
        opportunity_tx: Sender<Opportunity>,
        metrics: Arc<RwLock<Metrics>>,
    ) -> Self {
        Self {
            network_registries,
            executing_paths,
            config, // Store config
            opportunity_tx: Arc::new(opportunity_tx),
            _metrics: metrics,
        }
    }

    pub async fn get_min_profit_usd(&self, source_chain_id: u64, target_chain_id: u64) -> f64 {
        let source_min = self
            .network_registries
            .get_profit_token_registry(source_chain_id)
            .await
            .unwrap()
            .read()
            .await
            .min_profit_usd;
        let target_min = self
            .network_registries
            .get_profit_token_registry(target_chain_id)
            .await
            .unwrap()
            .read()
            .await
            .min_profit_usd;
        source_min.max(target_min)
    }

    pub async fn handle_pool_change(&self, pool_change: PoolChange) -> Result<()> {
        info!(
            "CHAIN ID: {} | Handling pool change for pool {}",
            pool_change.chain_id, pool_change.pool_address
        );
        let chain_id = pool_change.chain_id;
        let pool_address = pool_change.pool_address;

        let block_number = pool_change.block_number;
        let (source_paths, target_paths) = match self
            .network_registries
            .get_path_registry(chain_id)
            .await
            .unwrap()
            .read()
            .await
            .get_paths_for_pool(pool_address)
            .await
        {
            Some((source_paths, target_paths)) => (source_paths, target_paths),
            None => return Ok(()),
        };

        let mut cache_max_amount: HashMap<(u64, Address), U256> = HashMap::new(); // (chain_id, token_address) -> amount
        let memo_cache: Arc<DashMap<(u64, Address, Address, U256), U256>> =
            Arc::new(DashMap::new());

        let source_chain_id = source_paths.chain_id;
        let source_anchor_token = source_paths.anchor_token;
        for source in &source_paths.paths {
            for target_chain in &target_paths {
                let target_chain_id = target_chain.chain_id;
                let target_anchor_token = target_chain.anchor_token;
                for target in &target_chain.paths {
                    let mut forward_source_chain = source.clone();
                    forward_source_chain.reverse();
                    // After reversing, swap token_in and token_out for each PoolDirection in the path
                    for pair in forward_source_chain.iter_mut() {
                        std::mem::swap(&mut pair.token_in, &mut pair.token_out);
                    }

                    let forward_path = TwoSidePath {
                        anchor_token: source_anchor_token,
                        source_chain: forward_source_chain,
                        target_chain: target.clone(),
                        source_chain_id,
                        target_chain_id,
                    };

                    let mut backward_source_chain = target.clone();
                    backward_source_chain.reverse();
                    // After reversing, swap token_in and token_out for each PoolDirection in the path
                    for pair in backward_source_chain.iter_mut() {
                        std::mem::swap(&mut pair.token_in, &mut pair.token_out);
                    }

                    let backward_path = TwoSidePath {
                        anchor_token: target_anchor_token,
                        source_chain: backward_source_chain,
                        target_chain: source.clone(),
                        source_chain_id: target_chain_id,
                        target_chain_id: source_chain_id,
                    };

                    // check if the path is already being executed
                    if self.executing_paths.contains_key(&forward_path.hash())
                        || self.executing_paths.contains_key(&backward_path.hash())
                    {
                        continue;
                    }
                    // get the pool snapshot
                    let mut forward_pool_snapshot: HashMap<
                        Address,
                        Box<dyn PoolInterface + Send + Sync>,
                    > = HashMap::new();
                    for pair in &forward_path.source_chain {
                        let pool = self
                            .network_registries
                            .get_pool_registry(source_chain_id)
                            .await
                            .unwrap()
                            .read()
                            .await
                            .get_pool(&pair.pool)
                            .await
                            .ok_or_else(|| {
                                anyhow::anyhow!("Pool not found for address: {}", pair.pool)
                            })?;
                        let pool_guard = pool.read().await;
                        forward_pool_snapshot.insert(pair.pool, pool_guard.clone_box());
                    }
                    let mut backward_pool_snapshot: HashMap<
                        Address,
                        Box<dyn PoolInterface + Send + Sync>,
                    > = HashMap::new();
                    for pair in &backward_path.source_chain {
                        let pool = self
                            .network_registries
                            .get_pool_registry(target_chain_id)
                            .await
                            .unwrap()
                            .read()
                            .await
                            .get_pool(&pair.pool)
                            .await
                            .ok_or_else(|| {
                                anyhow::anyhow!("Pool not found for address: {}", pair.pool)
                            })?;
                        let pool_guard = pool.read().await;
                        backward_pool_snapshot.insert(pair.pool, pool_guard.clone_box());
                    }

                    let forward_pool_snapshot = Arc::new(forward_pool_snapshot);
                    let backward_pool_snapshot = Arc::new(backward_pool_snapshot);

                    // check the forward path
                    let forward_max_amount = self
                        .get_max_amount(
                            &mut cache_max_amount,
                            source_chain_id,
                            forward_path.source_chain[0].token_in,
                        )
                        .await;

                    let backward_max_amount = self
                        .get_max_amount(
                            &mut cache_max_amount,
                            target_chain_id,
                            backward_path.source_chain[0].token_in,
                        )
                        .await;

                    let max_iterations = self.config.max_iterations;

                    let source_chain_profit_token_registry = self
                        .network_registries
                        .get_profit_token_registry(source_chain_id)
                        .await
                        .unwrap();

                    let target_chain_profit_token_registry = self
                        .network_registries
                        .get_profit_token_registry(target_chain_id)
                        .await
                        .unwrap();

                    let min_profit_usd = {
                        let source_min = source_chain_profit_token_registry
                            .read()
                            .await
                            .min_profit_usd;
                        let target_min = target_chain_profit_token_registry
                            .read()
                            .await
                            .min_profit_usd;
                        source_min.max(target_min)
                    };

                    let source_chain_profit_token_registry_clone =
                        source_chain_profit_token_registry.clone();
                    let target_chain_profit_token_registry_clone =
                        target_chain_profit_token_registry.clone();
                    let memo_cache_clone = memo_cache.clone();
                    let forward_pool_snapshot_clone = forward_pool_snapshot.clone();
                    let backward_pool_snapshot_clone = backward_pool_snapshot.clone();
                    let opportunity_tx = self.opportunity_tx.clone();
                    tokio::spawn(async move {
                        let result = Self::handle_path(
                            &forward_path,
                            &forward_pool_snapshot_clone,
                            &backward_pool_snapshot_clone,
                            forward_max_amount,
                            max_iterations,
                            &memo_cache_clone,
                            &source_chain_profit_token_registry_clone,
                            &target_chain_profit_token_registry_clone,
                        )
                        .await;

                        if let Ok(result) = result {
                            if let Some((
                                profit,
                                amount_in,
                                anchor_token_amount,
                                amount_out,
                                steps,
                            )) = result
                            {
                                if profit < min_profit_usd {
                                    return Ok(());
                                }
                                opportunity_tx
                                    .send(Opportunity {
                                        path: forward_path,
                                        block_number,
                                        profit,
                                        amount_in,
                                        amount_out,
                                        anchor_token_amount,
                                        steps,
                                        min_profit_usd,
                                    })
                                    .await
                                    .unwrap();
                                Ok(())
                            } else {
                                return Err(anyhow::anyhow!("No result from handle_path forward"));
                            }
                        } else {
                            return Err(anyhow::anyhow!("Error from handle_path forward"));
                        }
                    });

                    let memo_cache_clone = memo_cache.clone();
                    let forward_pool_snapshot_clone = forward_pool_snapshot.clone();
                    let backward_pool_snapshot_clone = backward_pool_snapshot.clone();
                    let opportunity_tx = self.opportunity_tx.clone();
                    tokio::spawn(async move {
                        let result = Self::handle_path(
                            &backward_path,
                            &backward_pool_snapshot_clone,
                            &forward_pool_snapshot_clone,
                            backward_max_amount,
                            max_iterations,
                            &memo_cache_clone,
                            &target_chain_profit_token_registry,
                            &source_chain_profit_token_registry,
                        )
                        .await;

                        if let Ok(result) = result {
                            if let Some((
                                profit,
                                amount_in,
                                anchor_token_amount,
                                amount_out,
                                steps,
                            )) = result
                            {
                                if profit < min_profit_usd {
                                    return Ok(());
                                }
                                opportunity_tx
                                    .send(Opportunity {
                                        path: backward_path,
                                        block_number,
                                        profit,
                                        amount_in,
                                        amount_out,
                                        steps,
                                        anchor_token_amount,
                                        min_profit_usd,
                                    })
                                    .await
                                    .unwrap();
                                Ok(())
                            } else {
                                return Err(anyhow::anyhow!("No result from handle_path backward"));
                            }
                        } else {
                            return Err(anyhow::anyhow!("Error from handle_path backward"));
                        }
                    });
                }
            }
        }

        Ok(())
    }

    async fn get_max_amount(
        &self,
        cache_max_amount: &mut HashMap<(u64, Address), U256>,
        chain_id: u64,
        token: Address,
    ) -> U256 {
        if let Some(max_amount) = cache_max_amount.get(&(chain_id, token)) {
            *max_amount
        } else {
            let max_amount = self
                .network_registries
                .get_profit_token_registry(chain_id)
                .await
                .unwrap()
                .read()
                .await
                .get_amount_for_value(&token, self.config.max_amount_usd)
                .await
                .unwrap_or(U256::from(DEFAULT_MAX_AMOUNT));
            cache_max_amount.insert((chain_id, token), max_amount);
            max_amount
        }
    }

    pub async fn load_pool_and_handle_path(
        &self,
        path: &TwoSidePath,
    ) -> Result<Option<(f64, U256, U256, U256, Vec<U256>)>> {
        // (profit_usd, amount_in, anchor_token_amount, amount_out, steps)
        let mut forward_pool_snapshot: HashMap<Address, Box<dyn PoolInterface + Send + Sync>> =
            HashMap::new();
        let mut backward_pool_snapshot: HashMap<Address, Box<dyn PoolInterface + Send + Sync>> =
            HashMap::new();
        for pair in &path.source_chain {
            let pool = self
                .network_registries
                .get_pool_registry(path.source_chain_id)
                .await
                .unwrap()
                .read()
                .await
                .get_pool(&pair.pool)
                .await
                .ok_or_else(|| anyhow::anyhow!("Pool not found for address: {}", pair.pool))?;
            let pool_guard = pool.read().await;
            forward_pool_snapshot.insert(pair.pool, pool_guard.clone_box());
        }
        for pair in &path.target_chain {
            let pool = self
                .network_registries
                .get_pool_registry(path.target_chain_id)
                .await
                .unwrap()
                .read()
                .await
                .get_pool(&pair.pool)
                .await
                .ok_or_else(|| anyhow::anyhow!("Pool not found for address: {}", pair.pool))?;
            let pool_guard = pool.read().await;
            backward_pool_snapshot.insert(pair.pool, pool_guard.clone_box());
        }

        let max_amount = self
            .get_max_amount(
                &mut HashMap::new(),
                path.source_chain_id,
                path.source_chain[0].token_in,
            )
            .await;

        let memo_cache = Arc::new(DashMap::new());
        let source_chain_profit_token_registry = self
            .network_registries
            .get_profit_token_registry(path.source_chain_id)
            .await
            .unwrap();
        let target_chain_profit_token_registry = self
            .network_registries
            .get_profit_token_registry(path.target_chain_id)
            .await
            .unwrap();

        Self::handle_path(
            path,
            &forward_pool_snapshot,
            &backward_pool_snapshot,
            max_amount,
            self.config.max_iterations,
            &memo_cache,
            &source_chain_profit_token_registry,
            &target_chain_profit_token_registry,
        )
        .await
    }

    async fn handle_path(
        path: &TwoSidePath,
        forward_pool_snapshot: &HashMap<Address, Box<dyn PoolInterface + Send + Sync>>,
        backward_pool_snapshot: &HashMap<Address, Box<dyn PoolInterface + Send + Sync>>,
        max_amount: U256,
        max_iterations: u32,
        memo_cache: &Arc<DashMap<(u64, Address, Address, U256), U256>>,
        source_chain_profit_token_registry: &Arc<RwLock<ProfitTokenRegistry>>,
        target_chain_profit_token_registry: &Arc<RwLock<ProfitTokenRegistry>>,
    ) -> Result<Option<(f64, U256, U256, U256, Vec<U256>)>> {
        // If max_amount is too small, return early
        if max_amount <= U256::ONE {
            return Ok(None);
        }

        let mut best_profit = 0.0;
        let mut best_amount_in = U256::ZERO;
        let mut best_amount_out = U256::ZERO;
        let mut best_anchor_token_amount = U256::ZERO;
        let mut best_steps = Vec::new();

        // Track search state
        let mut iterations = 0;

        // Configure search parameters based on strategy
        let (sample_capacity, min_samples) = (15, 8);

        // Generate better distributed sample points that work well for token amounts (where 1 = 10^18)
        let mut sample_points = Vec::with_capacity(sample_capacity);

        // Always include these key points
        sample_points.push(U256::from(100)); // Smallest possible amount
        sample_points.push(max_amount); // Maximum amount

        // Add the midpoint (this is where naive binary search would start)
        sample_points.push(max_amount / U256::from(2));

        sample_points.push(max_amount / U256::from(4));
        sample_points.push(max_amount * U256::from(3) / U256::from(4));
        sample_points.push(max_amount / U256::from(8));
        sample_points.push(max_amount * U256::from(3) / U256::from(8));
        sample_points.push(max_amount * U256::from(5) / U256::from(8));
        sample_points.push(max_amount * U256::from(7) / U256::from(8));

        // Add even finer divisions for very thorough search
        sample_points.push(max_amount / U256::from(16));
        sample_points.push(max_amount * U256::from(15) / U256::from(16));

        // Add logarithmic points between 1 and max_amount/16 to catch small optimal amounts
        // These are especially important for tokens with high value
        if max_amount > U256::from(16) {
            let log_max = max_amount / U256::from(16); // Only use logarithmic sampling for the lower 1/16 of the range
            let mut point = U256::from(10); // Start at 10 instead of 1

            while point < log_max {
                sample_points.push(point);
                // Use a larger multiplier for bigger jumps
                point = point.saturating_mul(U256::from(100));
            }
        }

        // Sort and deduplicate sample points
        sample_points.sort();
        sample_points.dedup();

        // Ensure we have at least min_samples points
        if sample_points.len() < min_samples && max_amount > U256::from(100) {
            // Calculate how many additional points we need
            let additional_needed = min_samples - sample_points.len();
            if additional_needed > 0 {
                // Create evenly spaced points across the range
                let step = max_amount / U256::from(additional_needed + 1);
                for i in 1..=additional_needed {
                    let new_point = step * U256::from(i);
                    if new_point > U256::ONE && new_point < max_amount {
                        sample_points.push(new_point);
                    }
                }
                // Sort again after adding new points
                sample_points.sort();
                sample_points.dedup();
            }
        }

        // Evaluate all sample points
        let mut sample_results = Vec::with_capacity(sample_points.len());
        for &amount in &sample_points {
            iterations += 1;
            if iterations > max_iterations {
                break;
            }

            // Run simulation
            let (profit, amount_in, anchor_token_amount, amount_out, steps) =
                Self::simulate_amount(
                    &path,
                    amount,
                    &forward_pool_snapshot,
                    &backward_pool_snapshot,
                    &memo_cache,
                    &source_chain_profit_token_registry,
                    &target_chain_profit_token_registry,
                )
                .await?;

            // Track result
            sample_results.push((amount, profit, anchor_token_amount));

            // Update best seen
            if profit > best_profit {
                best_profit = profit;
                best_amount_in = amount_in;
                best_amount_out = amount_out;
                best_steps = steps;
                best_anchor_token_amount = anchor_token_amount;
            }
        }

        // If we found no profit in the samples, return early
        if best_profit == 0.0 {
            return Ok(None);
        }

        // Find the most promising region based on sample results
        // Sort by profit to find the best point
        sample_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)); // Sort by profit (descending)

        // Second phase: Focused binary search around the most promising point
        if iterations < max_iterations && sample_results.len() > 0 {
            // Get the best point from sampling
            let (best_sample, _, _) = sample_results[0];

            // Define search bounds around this point with radius based on strategy
            let search_radius_factor = U256::from(2); // 50% radius

            // Use wider radius for more thorough search
            let search_radius = if best_sample > U256::from(4) {
                best_sample.saturating_mul(search_radius_factor)
                    / U256::from(search_radius_factor + U256::ONE)
            } else {
                U256::ONE
            };

            let low = if best_sample > search_radius {
                best_sample - search_radius
            } else {
                U256::ONE
            };
            let high = std::cmp::min(best_sample + search_radius, max_amount);

            // Only continue if we have a meaningful range to search
            if high > low.saturating_add(U256::from(10)) {
                let mut search_low = low;
                let mut search_high = high;

                // Traditional binary/ternary search in this focused region
                while search_low <= search_high && iterations < max_iterations {
                    iterations += 1;

                    // Avoid underflow when low == high
                    if search_low == search_high {
                        let (profit, amount_in, anchor_token_amount, amount_out, steps) =
                            Self::simulate_amount(
                                &path,
                                search_low,
                                &forward_pool_snapshot,
                                &backward_pool_snapshot,
                                &memo_cache,
                                &source_chain_profit_token_registry,
                                &target_chain_profit_token_registry,
                            )
                            .await?;

                        if profit > best_profit {
                            best_profit = profit;
                            best_amount_in = amount_in;
                            best_amount_out = amount_out;
                            best_steps = steps;
                            best_anchor_token_amount = anchor_token_amount;
                        }
                        break;
                    }

                    // Normal/Slow: Test two points (ternary search)
                    let mid1 = search_low + (search_high - search_low) / U256::from(3);
                    let mid2 =
                        search_low + (search_high - search_low) * U256::from(2) / U256::from(3);

                    // Get or create cache entry for mid1
                    // Evaluate mid1
                    let (profit1, amount_in1, anchor_token_amount1, amount_out1, steps1) =
                        Self::simulate_amount(
                            &path,
                            mid1,
                            &forward_pool_snapshot,
                            &backward_pool_snapshot,
                            &memo_cache,
                            &source_chain_profit_token_registry,
                            &target_chain_profit_token_registry,
                        )
                        .await?;

                    // Update best if applicable
                    if profit1 > best_profit {
                        best_profit = profit1;
                        best_amount_in = amount_in1;
                        best_amount_out = amount_out1;
                        best_steps = steps1;
                        best_anchor_token_amount = anchor_token_amount1;
                    }

                    // Normal/Slow: Evaluate mid2 for ternary search
                    let (profit2, amount_in2, anchor_token_amount2, amount_out2, steps2) =
                        Self::simulate_amount(
                            &path,
                            mid2,
                            &forward_pool_snapshot,
                            &backward_pool_snapshot,
                            &memo_cache,
                            &source_chain_profit_token_registry,
                            &target_chain_profit_token_registry,
                        )
                        .await?;

                    // Update best if applicable
                    if profit2 > best_profit {
                        best_profit = profit2;
                        best_amount_in = amount_in2;
                        best_amount_out = amount_out2;
                        best_steps = steps2;
                        best_anchor_token_amount = anchor_token_amount2;
                    }

                    // Ternary search decision
                    if profit1 >= profit2 {
                        // Peak is likely at mid1 or left of mid1
                        search_high = mid2 - U256::ONE;
                    } else {
                        // Peak is likely right of mid1
                        search_low = mid1 + U256::ONE;
                    }
                }
            }
        }

        // We no longer need to update a shared cache since we're using the global memo cache

        if best_profit > 0.0 {
            return Ok(Some((
                best_profit,
                best_amount_in,
                best_anchor_token_amount,
                best_amount_out,
                best_steps,
            )));
        }

        Ok(None)
    }

    async fn simulate_amount(
        path: &TwoSidePath,
        amount_in: U256,
        forward_pool_snapshot: &HashMap<Address, Box<dyn PoolInterface + Send + Sync>>,
        backward_pool_snapshot: &HashMap<Address, Box<dyn PoolInterface + Send + Sync>>,
        memo_cache: &Arc<DashMap<(u64, Address, Address, U256), U256>>,
        source_chain_profit_token_registry: &Arc<RwLock<ProfitTokenRegistry>>,
        target_chain_profit_token_registry: &Arc<RwLock<ProfitTokenRegistry>>,
    ) -> Result<(f64, U256, U256, U256, Vec<U256>)> {
        let mut current_amount = amount_in;
        let anchor_token_amount: U256;
        let path_length = forward_pool_snapshot.keys().len() + backward_pool_snapshot.keys().len();

        // Pre-allocate steps vector with capacity to avoid reallocations
        let mut steps: Vec<U256> = Vec::with_capacity(path_length + 1);
        steps.push(current_amount);

        for (_i, pair) in path.source_chain.iter().enumerate() {
            let output_amount = if let Some(cached_value) = memo_cache.get(&(
                path.source_chain_id,
                pair.pool,
                pair.token_in,
                current_amount,
            )) {
                *cached_value.value()
            } else {
                let pool = forward_pool_snapshot.get(&pair.pool).unwrap();
                let output_amount = pool
                    .calculate_output(&pair.token_in, current_amount)
                    .unwrap();
                memo_cache.insert(
                    (
                        path.source_chain_id,
                        pair.pool,
                        pair.token_in,
                        current_amount,
                    ),
                    output_amount,
                );
                output_amount
            };

            current_amount = output_amount;
            steps.push(current_amount);
        }

        anchor_token_amount = current_amount;

        for (_i, pair) in path.target_chain.iter().enumerate() {
            let output_amount = if let Some(cached_value) = memo_cache.get(&(
                path.target_chain_id,
                pair.pool,
                pair.token_in,
                current_amount,
            )) {
                *cached_value.value()
            } else {
                let pool = backward_pool_snapshot.get(&pair.pool).unwrap();
                let output_amount = pool
                    .calculate_output(&pair.token_in, current_amount)
                    .unwrap();
                memo_cache.insert(
                    (
                        path.target_chain_id,
                        pair.pool,
                        pair.token_in,
                        current_amount,
                    ),
                    output_amount,
                );
                output_amount
            };

            current_amount = output_amount;
            steps.push(current_amount);
        }

        let amount_out = current_amount;

        let amount_in_usd = source_chain_profit_token_registry
            .read()
            .await
            .get_value(&path.source_chain[0].token_in, amount_in)
            .await
            .unwrap_or(
                source_chain_profit_token_registry
                    .read()
                    .await
                    .to_human_amount_f64(&path.source_chain.first().unwrap().token_in, amount_in)
                    .await
                    .unwrap_or(0.0),
            );
        let amount_out_usd = target_chain_profit_token_registry
            .read()
            .await
            .get_value(&path.target_chain.last().unwrap().token_out, amount_out)
            .await
            .unwrap_or(
                target_chain_profit_token_registry
                    .read()
                    .await
                    .to_human_amount_f64(&path.target_chain[0].token_out, anchor_token_amount)
                    .await
                    .unwrap_or(0.0),
            );
        let profit_usd = amount_out_usd - amount_in_usd;

        Ok((
            profit_usd,
            amount_in,
            anchor_token_amount,
            amount_out,
            steps,
        ))
    }
}
