use crate::blockchain::fetch_events;
use crate::blockchain::pool_fetcher::identify_and_fetch_pool;
use crate::models::pool::base::Topic;
use crate::models::pool::PoolRegistry;
use crate::models::token::TokenRegistry;
use crate::utils::metrics::Metrics;
use alloy::eips::{BlockId, BlockNumberOrTag};
use alloy::primitives::Address;
use alloy::providers::{DynProvider, Provider};
use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::RwLock;

use super::PoolChange;

pub struct PoolUpdaterLatestBlock {
    provider: Arc<DynProvider>,
    pool_registry: Arc<RwLock<PoolRegistry>>,
    token_registry: Arc<RwLock<TokenRegistry>>,
    pending_new_pools: Arc<RwLock<Vec<Address>>>,
    metrics: Arc<RwLock<Metrics>>,
    pub max_blocks_per_batch: u64,
    swap_event_tx: mpsc::Sender<PoolChange>,
    topics: Arc<Vec<Topic>>,
    profitable_topics: Arc<HashSet<Topic>>,
    pub multicall_address: Address,
    pub wait_time_fetch: u64,
    chain_id: u64,
}

impl PoolUpdaterLatestBlock {
    pub async fn new(
        provider: Arc<DynProvider>,
        pool_registry: Arc<RwLock<PoolRegistry>>,
        token_registry: Arc<RwLock<TokenRegistry>>,
        multicall_address: Address,
        metrics: Arc<RwLock<Metrics>>,
        swap_event_tx: mpsc::Sender<PoolChange>,
        start_block: u64,
        max_blocks_per_batch: u64,
        wait_time_fetch: u64,
        chain_id: u64,
    ) -> Self {
        // Initialize the last_processed_block in the registry if it's currently 0
        tokio::spawn({
            let pool_registry = Arc::clone(&pool_registry);
            async move {
                let current_block = pool_registry.read().await.get_last_processed_block().await;
                if current_block == 0 {
                    pool_registry
                        .write()
                        .await
                        .set_last_processed_block(start_block)
                        .await;
                    info!(
                        "CHAIN ID: {} | Initialized last processed block to {}",
                        chain_id, start_block
                    );
                } else if start_block > 0 && start_block > current_block {
                    // Override existing block if a higher start_block is provided
                    pool_registry
                        .write()
                        .await
                        .set_last_processed_block(start_block)
                        .await;
                    info!(
                        "CHAIN ID: {} | Updated last processed block from {} to {}",
                        chain_id, current_block, start_block
                    );
                }
            }
        });

        Self {
            provider,
            pool_registry: pool_registry.clone(),
            token_registry: token_registry.clone(),
            pending_new_pools: Arc::new(RwLock::new(Vec::new())),
            metrics: metrics.clone(),
            max_blocks_per_batch,
            swap_event_tx,
            topics: Arc::new(pool_registry.read().await.get_topics().await.clone()),
            profitable_topics: Arc::new(
                pool_registry
                    .read()
                    .await
                    .get_profitable_topics()
                    .await
                    .clone(),
            ),
            multicall_address,
            wait_time_fetch,
            chain_id,
        }
    }

    pub async fn start(&self, initial_block: u64, initial_pools: Vec<Address>) -> Result<()> {
        // initialize the registry
        let factory_to_fee = self.pool_registry.read().await.get_factory_to_fee().await;
        let aero_factory_addresses = self
            .pool_registry
            .read()
            .await
            .get_aero_factory_addresses()
            .await;

        for pool in initial_pools {
            let pool = identify_and_fetch_pool(
                &self.provider,
                pool,
                BlockId::Number(BlockNumberOrTag::Number(initial_block)),
                &self.token_registry,
                self.multicall_address,
                &factory_to_fee,
                &aero_factory_addresses,
            )
            .await?;
            self.pool_registry.write().await.add_pool(pool).await;
        }
        self.pool_registry
            .write()
            .await
            .set_last_processed_block(initial_block)
            .await;

        loop {
            // Get the last processed block from registry
            let last_processed_block = self
                .pool_registry
                .read()
                .await
                .get_last_processed_block()
                .await;

            while self.pending_new_pools.read().await.len() > 0 {
                let pool_address = self.pending_new_pools.write().await.pop().unwrap();
                if let Ok(pool) = identify_and_fetch_pool(
                    &self.provider,
                    pool_address,
                    BlockId::Number(BlockNumberOrTag::Number(last_processed_block)),
                    &self.token_registry,
                    self.multicall_address,
                    &self.pool_registry.read().await.get_factory_to_fee().await,
                    &self
                        .pool_registry
                        .read()
                        .await
                        .get_aero_factory_addresses()
                        .await,
                )
                .await
                {
                    self.pool_registry.read().await.add_pool(pool).await;

                    info!(
                        "CHAIN ID: {} | Added pool {} to pool registry",
                        self.chain_id, pool_address
                    );
                } else {
                    error!(
                        "CHAIN ID: {} | Error fetching pool {} from identify_and_fetch_pool",
                        self.chain_id, pool_address
                    );
                    continue;
                }
            }
            // Get latest block number with retry logic
            let mut backoff = Duration::from_millis(50);
            let max_backoff = Duration::from_millis(500);
            let latest_block = loop {
                match self.provider.get_block_number().await {
                    Ok(block) => {
                        break block;
                    }
                    Err(e) => {
                        error!(
                            "CHAIN ID: {} | Error fetching block number, retrying in {}s: {}",
                            self.chain_id,
                            backoff.as_secs(),
                            e
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = std::cmp::min(backoff * 2, max_backoff);
                    }
                }
            };

            // Process blocks in batches up to the latest confirmed block
            let mut current_block = last_processed_block + 1;
            if current_block > latest_block {
                debug!(
                    "Waiting for new blocks. Current: {}, Latest: {}",
                    current_block, latest_block
                );
                tokio::time::sleep(Duration::from_millis(self.wait_time_fetch)).await;
                continue;
            }

            while current_block <= latest_block {
                let batch_end =
                    std::cmp::min(current_block + self.max_blocks_per_batch - 1, latest_block);
                info!(
                    "CHAIN ID: {} | Processing blocks: {} - {}",
                    self.chain_id, current_block, batch_end
                );

                // Process pools for confirmed blocks
                match proccess_pools(
                    &self.provider,
                    &self.pool_registry,
                    &self.metrics,
                    &self.swap_event_tx,
                    BlockNumberOrTag::Number(current_block),
                    BlockNumberOrTag::Number(batch_end),
                    batch_end == latest_block,
                    self.topics.clone(),
                    self.profitable_topics.clone(),
                    self.chain_id,
                )
                .await
                {
                    Ok(_) => {
                        // Update last processed block in registry
                        self.pool_registry
                            .write()
                            .await
                            .set_last_processed_block(batch_end)
                            .await;
                        info!(
                            "CHAIN ID: {} | Successfully processed blocks: {} - {}",
                            self.chain_id, current_block, batch_end
                        );
                    }
                    Err(e) => {
                        error!(
                            "CHAIN ID: {} | Error processing blocks {} - {}: {}",
                            self.chain_id, current_block, batch_end, e
                        );
                        // Don't update last_processed_block on error
                        break;
                    }
                }

                current_block = batch_end + 1;
            }

            // Add a small delay between iterations to prevent tight loops
            tokio::time::sleep(Duration::from_millis(self.wait_time_fetch)).await;
        }
    }

    pub async fn set_provider(&mut self, provider: DynProvider) {
        self.provider = Arc::new(provider);
    }

    pub async fn set_multicall_address(&mut self, multicall_address: Address) {
        self.multicall_address = multicall_address;
    }

    pub async fn set_wait_time_fetch(&mut self, wait_time_fetch: u64) {
        self.wait_time_fetch = wait_time_fetch;
    }

    pub async fn set_max_blocks_per_batch(&mut self, max_blocks_per_batch: u64) {
        self.max_blocks_per_batch = max_blocks_per_batch;
    }

    pub async fn add_pending_new_pools(&self, new_pools: Vec<Address>) -> Result<()> {
        for address in new_pools {
            info!(
                "CHAIN ID: {} | Checking if pool {} is already registered",
                self.chain_id, address
            );
            // Check if pool address is not already registered in pool_registry
            if !self.pool_registry.read().await.exists_pool(&address).await {
                // If not, add to self.pending_new_pools (if not already present)
                if !self.pending_new_pools.read().await.contains(&address) {
                    info!(
                        "CHAIN ID: {} | Adding pool {} to pending new pools",
                        self.chain_id, address
                    );
                    self.pending_new_pools.write().await.push(address);
                } else {
                    info!(
                        "CHAIN ID: {} | Pool {} is already in pending new pools",
                        self.chain_id, address
                    );
                }
            } else {
                info!(
                    "CHAIN ID: {} | Pool {} is already in pool registry",
                    self.chain_id, address
                );
            }
            info!(
                "CHAIN ID: {} | Pending new pools: {:?}",
                self.chain_id,
                self.pending_new_pools.read().await
            );
        }
        Ok(())
    }
}

async fn proccess_pools<P: Provider + Send + Sync + 'static>(
    provider: &Arc<P>,
    pool_registry: &Arc<RwLock<PoolRegistry>>,
    metrics: &Arc<RwLock<Metrics>>,
    swap_event_tx: &mpsc::Sender<PoolChange>,
    from_block: BlockNumberOrTag,
    to_block: BlockNumberOrTag,
    is_latest_block: bool,
    topics: Arc<Vec<Topic>>,
    profitable_topics: Arc<HashSet<Topic>>,
    chain_id: u64,
) -> Result<()> {
    let addresses: Vec<Address> = pool_registry.read().await.get_all_addresses().await.clone();
    let addresses_len = addresses.len();
    if addresses_len == 0 {
        return Ok(());
    }

    let mut backoff = Duration::from_millis(50);
    let max_backoff = Duration::from_millis(500);

    let topics = topics.clone().to_vec();
    loop {
        match fetch_events(
            provider,
            addresses.clone(),
            topics.clone(),
            from_block,
            to_block,
        )
        .await
        {
            Ok(events) => {
                let received_at = Utc::now().timestamp_millis() as u64;
                let mut swap_events = Vec::new();
                info!(
                    "CHAIN ID: {} | Processing {} events from {} to {}",
                    chain_id,
                    events.len(),
                    from_block.as_number().unwrap(),
                    to_block.as_number().unwrap()
                );
                for event in events {
                    if let Some(pool) = pool_registry.read().await.get_pool(&event.address()).await
                    {
                        if let Err(e) = pool.write().await.apply_log(&event) {
                            error!(
                                "CHAIN ID: {} | Error applying event {} for pool {}, event {}",
                                chain_id,
                                e,
                                event.address(),
                                event.transaction_hash.unwrap()
                            );
                        }

                        if is_latest_block && profitable_topics.contains(event.topic0().unwrap()) {
                            swap_events.push(event);
                        }
                    }
                }

                if is_latest_block && from_block == to_block {
                    for event in swap_events {
                        let tx_hash = event.transaction_hash.unwrap();
                        let log_index = event.log_index.unwrap();
                        let mut guard = metrics.write().await;
                        guard.add_opportunity(tx_hash, log_index, received_at);
                        guard.set_proccessed_at(
                            tx_hash,
                            log_index,
                            Utc::now().timestamp_millis() as u64,
                        );
                        drop(guard);
                        if let Err(e) = swap_event_tx
                            .send(PoolChange {
                                pool_address: event.address(),
                                block_number: event.block_number.unwrap_or(0),
                                chain_id,
                            })
                            .await
                        {
                            error!(
                                "CHAIN ID: {} | Error sending swap event to simulator: {}",
                                chain_id, e
                            );
                        }
                    }
                }
                break;
            }
            Err(e) => {
                error!(
                    "CHAIN ID: {} | Error fetching events, retrying in {}s: {}",
                    chain_id,
                    backoff.as_secs(),
                    e
                );
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, max_backoff);
            }
        }
    }

    Ok(())
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::blockchain::{fetch_and_display_pool_info, IQuoterV2};
//     use crate::models::path::PathRegistry;
//     use crate::models::pool::{PoolRegistry, UniswapV3Pool};
//     use crate::models::profit_token::price_updater::PriceUpdater;
//     use crate::models::profit_token::ProfitTokenRegistry;
//     use crate::models::token::TokenRegistry;
//     use crate::UniswapV2Pool;
//     use alloy::eips::BlockId;
//     use alloy::primitives::aliases::U24;
//     use alloy::primitives::utils::parse_ether;
//     use alloy::primitives::{address, Uint};
//     use alloy::providers::{ProviderBuilder, MULTICALL3_ADDRESS};
//     use env_logger::Env;
//     use url::Url;

//     #[tokio::test]
//     async fn test_fetch_pools() {
//         const RPC_URL: &str = "https://sly-lively-water.story-mainnet.quiknode.pro/2cb2f586bc9ac68d8b0c29e46a6005abd5f0425e";
//         // Initialize logging
//         env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

//         let provider = ProviderBuilder::new().connect_http(Url::parse(&RPC_URL).unwrap());

//         let provider = Arc::new(provider);
//         let token_registry = Arc::new(RwLock::new(TokenRegistry::new(1)));
//         let price_updater = Arc::new(RwLock::new(
//             PriceUpdater::new("ethereum".to_string(), vec![]).await,
//         ));
//         let profit_token_registry = Arc::new(ProfitTokenRegistry::new(
//             Address::ZERO,
//             token_registry.clone(),
//             price_updater.clone(),
//             0f64,
//         ));
//         let pool_registry = Arc::new(PoolRegistry::new(provider.clone().erased(), 1));
//         let path_registry = Arc::new(PathRegistry::new(profit_token_registry.clone(), 4));

//         let pool_addresses: Vec<String> = vec![
//             "0x4a170A20Dd1ff838C99d49Ac18b517a339206E83".to_string(),
//             "0xc56c1bE28a22CED0270A4D2F45753d2b6300c1Ae".to_string(),
//             "0x84386055F90E898E8636e318a3ABf4ad672b51DF".to_string(),
//             "0x8a8139Bf88a645119701902c5a7Eb2669252b6ca".to_string(),
//             "0x3191De9B71687Cf1f964b8ABDa7BdC8f1d5c5580".to_string(),
//             "0xC7Cc67B2300d475F66E7fb21E04507E0E4839693".to_string(),
//             "0x0012dC8daEd76fd687b51aD6A4683aa151d3d59D".to_string(),
//             "0x4a170A20Dd1ff838C99d49Ac18b517a339206E83".to_string(),
//             "0xB1C4EA6C1c00567Da188eE0CFa8300185580B1a9".to_string(),
//             "0x9dFF955668f79a7fcB7396d244822257Fa409cD9".to_string(),
//             "0x15701E58038AFB06C4416A74C50eE8DF2A0c0B42".to_string(),
//             "0xbdD2fC284EDC7294Ba29E3aEccB05dfaB681Ae07".to_string(),
//             "0x402B4Dadd7eD67c08f98f30C7b8C975a03063a42".to_string(),
//             "0xd05F9692E27A308568fAa0E5a2291209DCa398Ca".to_string(),
//             "0x1bD56a4Eb84E3EA57EFA35526c15f8b50C17F41D".to_string(),
//             "0xd1275694C8e9Ef89b5523B753B3ACF2F829f7301".to_string(),
//             "0x2e65dEb500C8c5bf7eb92B4b23581BCf09F30117".to_string(),
//             "0x7e399B4A2414D90F17FBF1327E80DA57624B041b".to_string(),
//             "0x14181b8a908e7b0eb53adacb9949584783a28d51".to_string(),
//             "0x5827b9b2e155393347735ce93bccbea2b610304d".to_string(),
//         ];

//         let latest_block = 3500103;
//         // Fetch and display pool information
//         fetch_and_display_pool_info(
//             &provider,
//             &pool_addresses,
//             BlockNumberOrTag::Number(latest_block),
//             &token_registry,
//             &pool_registry,
//             &path_registry,
//             100,
//             MULTICALL3_ADDRESS,
//             &HashMap::new(),
//             &Vec::new(),
//         )
//         .await
//         .unwrap();

//         info!("Pool registry summary at: {}", latest_block);
//         info!("{}", pool_registry.log_summary().await);
//     }

//     #[tokio::test]
//     async fn test_pool_updater_both_latest_block() {
//         const RPC_URL: &str = "https://binance.llamarpc.com";
//         // Initialize logging
//         env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

//         let provider = ProviderBuilder::new().connect_http(Url::parse(&RPC_URL).unwrap());

//         let provider = Arc::new(provider);
//         let token_registry = Arc::new(RwLock::new(TokenRegistry::new(1)));
//         let price_updater = Arc::new(RwLock::new(
//             PriceUpdater::new("ethereum".to_string(), vec![]).await,
//         ));
//         let profit_token_registry = Arc::new(ProfitTokenRegistry::new(
//             Address::ZERO,
//             token_registry.clone(),
//             price_updater.clone(),
//             0f64,
//         ));
//         let pool_registry = PoolRegistry::new(provider.clone().erased(), 1);
//         let pool_registry_rw = Arc::new(RwLock::new(pool_registry));
//         let pool_registry = Arc::new(PoolRegistry::new(provider.clone().erased(), 1));
//         let path_registry = Arc::new(PathRegistry::new(profit_token_registry.clone(), 4));
//         let (swap_event_tx, _) = mpsc::channel(1000);
//         let metrics = Arc::new(RwLock::new(Metrics::new()));

//         let pool_addresses: Vec<String> = vec![
//             "0x0eD7e52944161450477ee417DE9Cd3a859b14fD0".to_string(),
//             "0xafb2da14056725e3ba3a30dd846b6bbbd7886c56".to_string(),
//         ];

//         let latest_block = provider.get_block_number().await.unwrap();
//         let end_block = latest_block;
//         let latest_block = latest_block - 100000;
//         // Fetch and display pool information
//         fetch_and_display_pool_info(
//             &provider,
//             &pool_addresses,
//             BlockNumberOrTag::Number(latest_block),
//             &token_registry,
//             &pool_registry,
//             &path_registry,
//             100,
//             MULTICALL3_ADDRESS,
//             &HashMap::new(),
//             &Vec::new(),
//         )
//         .await
//         .unwrap();

//         let mut current_block = latest_block;
//         let mut last_block = if current_block + 10000 > end_block {
//             end_block
//         } else {
//             current_block + 10000
//         };

//         loop {
//             println!("Processing blocks from {} to {}", current_block, last_block);
//             proccess_pools(
//                 &provider,
//                 &pool_registry_rw,
//                 &metrics,
//                 &swap_event_tx,
//                 BlockNumberOrTag::Number(current_block),
//                 BlockNumberOrTag::Number(last_block),
//                 false,
//                 Arc::new(pool_registry.get_topics().await.clone()),
//                 Arc::new(pool_registry.get_profitable_topics().await.clone()),
//             )
//             .await
//             .unwrap();

//             if last_block == end_block {
//                 break;
//             }
//             current_block = last_block;
//             last_block = if current_block + 10000 > end_block {
//                 end_block
//             } else {
//                 current_block + 10000
//             };
//             // Sleep for 1 second
//             tokio::time::sleep(Duration::from_millis(50)).await;
//         }

//         let pool_registry2 = Arc::new(PoolRegistry::new(provider.clone().erased(), 1));
//         fetch_and_display_pool_info(
//             &provider,
//             &pool_addresses,
//             BlockNumberOrTag::Number(end_block),
//             &token_registry,
//             &pool_registry2,
//             &path_registry,
//             100,
//             MULTICALL3_ADDRESS,
//             &HashMap::new(),
//             &Vec::new(),
//         )
//         .await
//         .unwrap();

//         let pool = pool_registry
//             .get_pool(&pool_addresses[0].parse::<Address>().unwrap())
//             .await
//             .unwrap();
//         let pool_guard = pool.read().await;
//         let v2_pool = pool_guard.downcast_ref::<UniswapV2Pool>().unwrap();

//         let pool2 = pool_registry2
//             .get_pool(&pool_addresses[0].parse::<Address>().unwrap())
//             .await
//             .unwrap();
//         let pool2_guard = pool2.read().await;
//         let v2_pool2 = pool2_guard.downcast_ref::<UniswapV2Pool>().unwrap();

//         // Compare the reserves between both pools
//         assert_eq!(v2_pool.reserve0, v2_pool2.reserve0);
//         assert_eq!(v2_pool.reserve1, v2_pool2.reserve1);

//         let pool = pool_registry
//             .get_pool(&pool_addresses[1].parse::<Address>().unwrap())
//             .await
//             .unwrap();

//         // Get the pool from registry2 as a concrete UniswapV2 instance
//         let pool2 = pool_registry2
//             .get_pool(&pool_addresses[1].parse::<Address>().unwrap())
//             .await
//             .unwrap();
//         println!("Asserted pool");
//         // Get the pool as a concrete UniswapV2 instance
//         let pool_guard = pool.read().await;
//         let v3_pool = pool_guard.downcast_ref::<UniswapV3Pool>().unwrap();
//         // Downcast to UniswapV2Pool
//         let pool2_guard = pool2.read().await;
//         let v3_pool2 = pool2_guard.downcast_ref::<UniswapV3Pool>().unwrap();

//         // Compare the reserves between both pools
//         assert_eq!(v3_pool.liquidity, v3_pool2.liquidity);
//         println!("Asserted liquidity");
//         assert_eq!(v3_pool.sqrt_price_x96, v3_pool2.sqrt_price_x96);
//         println!("Asserted sqrt_price_x96");
//         assert_eq!(v3_pool.tick, v3_pool2.tick);
//         println!("Asserted tick");

//         assert_eq!(v3_pool.token0, v3_pool2.token0);
//         println!("Asserted token0");
//         assert_eq!(v3_pool.token1, v3_pool2.token1);
//         println!("Asserted token1");
//         assert_eq!(v3_pool.fee, v3_pool2.fee);
//         println!("Asserted fee");

//         // Compare ticks map
//         assert_eq!(
//             v3_pool.ticks.len(),
//             v3_pool2.ticks.len(),
//             "Ticks map length mismatch"
//         );
//         println!("Asserted ticks map length");

//         for (tick_idx, tick) in &v3_pool.ticks {
//             let tick2 = v3_pool2
//                 .ticks
//                 .get(tick_idx)
//                 .expect(&format!("Tick {} not found in second pool", tick_idx));
//             assert_eq!(
//                 tick.index, tick2.index,
//                 "Tick index mismatch at {}",
//                 tick_idx
//             );
//             assert_eq!(
//                 tick.liquidity_net, tick2.liquidity_net,
//                 "Tick liquidity_net mismatch at {}",
//                 tick_idx
//             );
//             assert_eq!(
//                 tick.liquidity_gross, tick2.liquidity_gross,
//                 "Tick liquidity_gross mismatch at {}",
//                 tick_idx
//             );
//         }
//         println!("Asserted all ticks match");

//         for (tick_idx, tick) in &v3_pool2.ticks {
//             let tick2 = v3_pool
//                 .ticks
//                 .get(tick_idx)
//                 .expect(&format!("Tick {} not found in second pool", tick_idx));
//             assert_eq!(
//                 tick.index, tick2.index,
//                 "Tick index mismatch at {}",
//                 tick_idx
//             );
//             assert_eq!(
//                 tick.liquidity_net, tick2.liquidity_net,
//                 "Tick liquidity_net mismatch at {}",
//                 tick_idx
//             );
//             assert_eq!(
//                 tick.liquidity_gross, tick2.liquidity_gross,
//                 "Tick liquidity_gross mismatch at {}",
//                 tick_idx
//             );
//         }
//         println!("Asserted all ticks match in reverse");

//         let quoter = IQuoterV2::new(
//             address!("0xb048bbc1ee6b733fffcfb9e9cef7375518e25997"),
//             &provider,
//         );

//         let amount_in = parse_ether("1").unwrap();
//         let amount_out = quoter
//             .quoteExactInputSingle(IQuoterV2::QuoteExactInputSingleParams {
//                 tokenIn: address!("0x0E09FaBB73Bd3Ade0a17ECC321fD13a19e81cE82"),
//                 tokenOut: address!("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"),
//                 fee: U24::from(500),
//                 amountIn: amount_in,
//                 sqrtPriceLimitX96: Uint::from(0),
//             })
//             .call()
//             .block(BlockId::Number(BlockNumberOrTag::Number(end_block)))
//             .await
//             .unwrap();

//         let amount_out_estimate = pool
//             .read()
//             .await
//             .calculate_output(
//                 &address!("0x0E09FaBB73Bd3Ade0a17ECC321fD13a19e81cE82"),
//                 amount_in,
//             )
//             .unwrap();
//         let amount_out_estimate2 = pool2
//             .read()
//             .await
//             .calculate_output(
//                 &address!("0x0E09FaBB73Bd3Ade0a17ECC321fD13a19e81cE82"),
//                 amount_in,
//             )
//             .unwrap();
//         println!("Amount out estimate: {}", amount_out_estimate);
//         println!("Amount out estimate2: {}", amount_out_estimate2);
//         println!("Amount out: {}", amount_out.amountOut);
//         assert!(amount_out.amountOut == amount_out_estimate);
//         assert!(amount_out.amountOut == amount_out_estimate2);
//     }
// }
