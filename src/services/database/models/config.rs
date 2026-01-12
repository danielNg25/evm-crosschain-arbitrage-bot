use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Config model for MongoDB
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub max_amount_usd: f64,
    pub recheck_interval: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Config {
    pub fn new(max_amount_usd: f64, recheck_interval: u64) -> Self {
        Self {
            id: None,
            max_amount_usd,
            recheck_interval,
            created_at: Utc::now().timestamp() as u64,
            updated_at: Utc::now().timestamp() as u64,
        }
    }
}
