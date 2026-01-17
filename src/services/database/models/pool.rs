use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Pool model for MongoDB
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Pool {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub network_id: u64,
    pub address: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted_at: Option<u64>,
}

impl Pool {
    pub fn new(network_id: u64, address: String) -> Self {
        Self {
            id: None,
            network_id,
            address,
            created_at: Utc::now().timestamp() as u64,
            updated_at: Utc::now().timestamp() as u64,
            deleted_at: None,
        }
    }
}
