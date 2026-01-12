// use crate::core::PendingEvent;
// use crate::models::pool::base::Topic;
// use crate::models::pool::PoolRegistry;
// use crate::utils::metrics::Metrics;
// use alloy::eips::BlockNumberOrTag;
// use alloy::providers::Provider;
// use anyhow::Result;
// use chrono::Utc;
// use log::{error, info};
// use std::collections::{HashMap, HashSet};
// use std::sync::Arc;
// use tokio::sync::mpsc;
// use tokio::sync::RwLock;
// use tokio::time::Duration;

// use super::EventQueue;
// use crate::blockchain::fetch_events;

// pub struct PoolUpdaterLatestBlockWs<P: Provider + Send + Sync + 'static> {
//     provider: Arc<P>,
//     event_queue: EventQueue,
//     pool_registry: Arc<PoolRegistry>,
//     metrics: Arc<RwLock<Metrics>>,
//     swap_event_tx: mpsc::Sender<PendingEvent>,
//     max_blocks_per_batch: u64,
//     topics: Arc<Vec<Topic>>,
//     profitable_topics: Arc<HashSet<Topic>>,
// }

// impl<P: Provider + Send + Sync + 'static> PoolUpdaterLatestBlockWs<P> {
//     pub async fn new(
//         provider: Arc<P>,
//         event_queue: EventQueue,
//         pool_registry: Arc<PoolRegistry>,
//         metrics: Arc<RwLock<Metrics>>,
//         swap_event_tx: mpsc::Sender<PendingEvent>,
//         max_blocks_per_batch: u64,
//     ) -> Self {
//         let topics = pool_registry.get_topics().await.clone();
//         let profitable_topics = pool_registry.get_profitable_topics().await.clone();

//         Self {
//             provider,
//             event_queue,
//             pool_registry,
//             metrics,
//             swap_event_tx,
//             max_blocks_per_batch,
//             topics: Arc::new(topics),
//             profitable_topics: Arc::new(profitable_topics),
//         }
//     }

//     pub async fn start(&mut self) -> Result<()> {
//         // Bootstrap
//         let latest_block = self.provider.get_block_number().await?;
//         info!("Latest block: {}", latest_block);
//         let events = self.event_queue.get_all_available_events().await;
//         info!("Found {} events in EventQueue", events.len());
//         let mut first_event_block = latest_block;
//         let mut first_event_index = 0;
//         let mut first_event_log_index = 0;
//         if !events.is_empty() {
//             let first_event = events.first().unwrap();
//             first_event_block = first_event.block_number.unwrap();
//             first_event_index = first_event.transaction_index.unwrap();
//             first_event_log_index = first_event.log_index.unwrap();
//         }
//         info!(
//             "First event block: {}; index: {}",
//             first_event_block, first_event_index
//         );

//         // Calculate block ranges to fetch in batches
//         let last_processed_block = self.pool_registry.get_last_processed_block().await;

//         let mut start_block = last_processed_block;

//         let topics = self.topics.clone().to_vec();

//         info!("Catching up to first event block {}", first_event_block);
//         while start_block < first_event_block {
//             let end_block =
//                 std::cmp::min(start_block + self.max_blocks_per_batch, first_event_block);
//             let mut should_break = false;
//             match fetch_events(
//                 &self.provider,
//                 self.pool_registry.get_all_addresses().await,
//                 topics.clone(),
//                 BlockNumberOrTag::Number(start_block),
//                 BlockNumberOrTag::Number(end_block),
//             )
//             .await
//             {
//                 Ok(events) => {
//                     info!(
//                         "Fetched {} events in batch {} - {}",
//                         events.len(),
//                         start_block,
//                         end_block
//                     );
//                     for event in events {
//                         if let Some(pool) = self.pool_registry.get_pool(&event.address()).await {
//                             if event.block_number.unwrap() >= first_event_block
//                                 && event.transaction_index.unwrap() >= first_event_index
//                             {
//                                 if event.log_index.unwrap() >= first_event_log_index {
//                                     // We've reached the first event, break
//                                     info!(
//                                         "Reached first event {} block {} index {}, breaking",
//                                         event.transaction_hash.unwrap(),
//                                         event.block_number.unwrap(),
//                                         event.transaction_index.unwrap()
//                                     );
//                                     should_break = true;
//                                     break;
//                                 }
//                             }

//                             if let Err(e) = pool.write().await.apply_log(&event) {
//                                 error!(
//                                     "Error applying event {} for pool {}, event {}",
//                                     e,
//                                     event.address(),
//                                     event.transaction_hash.unwrap()
//                                 );
//                             }
//                         }
//                     }

//                     if end_block >= first_event_block {
//                         info!("Reached first event block {}, breaking", first_event_block);
//                         should_break = true;
//                     }

//                     if should_break {
//                         break;
//                     }
//                 }
//                 Err(e) => {
//                     error!(
//                         "Error fetching events in batch {}-{}: {}",
//                         start_block, end_block, e
//                     );
//                     continue;
//                 }
//             };

//             if should_break {
//                 break;
//             }

//             // Move to next batch
//             start_block = end_block;
//         }
//         for event in events {
//             if let Some(pool) = self.pool_registry.get_pool(&event.address()).await {
//                 if let Err(e) = pool.write().await.apply_log(&event) {
//                     error!(
//                         "Error applying event {} for pool {}, event {}",
//                         e,
//                         event.address(),
//                         event.transaction_hash.unwrap()
//                     );
//                 }
//             }
//         }

//         // Process events from EventQueue
//         loop {
//             // Get a batch of events from EventQueue
//             let events = self.event_queue.get_all_available_events().await;
//             if events.is_empty() {
//                 // Small delay to prevent tight loop
//                 tokio::time::sleep(Duration::from_millis(10)).await;
//                 continue;
//             }
//             let events_len = events.len();

//             info!("Processing {} events from EventQueue", events_len);
//             let received_at = Utc::now().timestamp_millis() as u64;
//             let mut swap_events = Vec::new();

//             for event in events {
//                 if let Some(pool) = self.pool_registry.get_pool(&event.address()).await {
//                     if let Err(e) = pool.write().await.apply_log(&event) {
//                         error!(
//                             "Error applying event {} for pool {}, event {}",
//                             e,
//                             event.address(),
//                             event.transaction_hash.unwrap()
//                         );
//                     }

//                     if self.profitable_topics.contains(event.topic0().unwrap()) {
//                         swap_events.push(event);
//                     }
//                 }
//             }

//             for event in swap_events {
//                 let tx_hash = event.transaction_hash.unwrap();
//                 let log_index = event.log_index.unwrap();
//                 let mut guard = self.metrics.write().await;
//                 guard.add_opportunity(tx_hash, log_index, received_at);
//                 guard.set_proccessed_at(tx_hash, log_index, Utc::now().timestamp_millis() as u64);
//                 drop(guard);
//                 if let Err(e) = self.swap_event_tx.send(PendingEvent { event }).await {
//                     error!("Error sending swap event to simulator: {}", e);
//                 }
//             }

//             info!("Processed {} events", events_len);
//             tokio::time::sleep(Duration::from_millis(10)).await;
//         }
//     }
// }
