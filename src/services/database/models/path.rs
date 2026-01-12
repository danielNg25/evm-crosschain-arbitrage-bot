use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::models::path::PoolDirection;

/// Pool model for MongoDB
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Path {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub source_network_id: u64,
    pub target_network_id: u64,
    pub source_chain: Vec<PoolDirection>,
    pub target_chain: Vec<PoolDirection>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Path {
    pub fn new(
        source_network_id: u64,
        target_network_id: u64,
        source_chain: Vec<PoolDirection>,
        target_chain: Vec<PoolDirection>,
    ) -> Self {
        Self {
            id: None,
            source_network_id,
            target_network_id,
            source_chain,
            target_chain,
            created_at: Utc::now().timestamp() as u64,
            updated_at: Utc::now().timestamp() as u64,
        }
    }
}
