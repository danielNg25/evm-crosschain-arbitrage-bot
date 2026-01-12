use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Network model for MongoDB
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Network {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub chain_id: u64,
    pub name: String,
    pub rpcs: Vec<String>,
    pub websocket_urls: Option<Vec<String>>,
    pub block_explorer: Option<String>,
    pub wrap_native: String,
    pub min_profit_usd: f64,
    pub v2_factory_to_fee: Option<HashMap<String, u64>>,
    pub aero_factory_addresses: Option<Vec<String>>,
    pub multicall_address: Option<String>,
    pub max_blocks_per_batch: u64,
    pub wait_time_fetch: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Network {
    pub fn new(
        chain_id: u64,
        name: String,
        rpcs: Vec<String>,
        websocket_urls: Option<Vec<String>>,
        wrap_native: String,
        min_profit_usd: f64,
        block_explorer: Option<String>,
        v2_factory_to_fee: Option<HashMap<String, u64>>,
        aero_factory_addresses: Option<Vec<String>>,
        multicall_address: Option<String>,
        max_blocks_per_batch: u64,
        wait_time_fetch: u64,
    ) -> Self {
        Self {
            id: None,
            chain_id,
            name,
            rpcs,
            websocket_urls,
            block_explorer,
            wrap_native,
            min_profit_usd,
            v2_factory_to_fee,
            aero_factory_addresses,
            multicall_address,
            max_blocks_per_batch,
            wait_time_fetch,
            created_at: Utc::now().timestamp() as u64,
            updated_at: Utc::now().timestamp() as u64,
        }
    }
}
