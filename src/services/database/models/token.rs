use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Pool model for MongoDB
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Token {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub network_id: u64,
    pub address: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Token {
    pub fn new(
        network_id: u64,
        address: String,
        name: Option<String>,
        symbol: Option<String>,
        decimals: Option<u8>,
    ) -> Self {
        Self {
            id: None,
            network_id,
            address,
            name,
            symbol,
            decimals,
            created_at: Utc::now().timestamp() as u64,
            updated_at: Utc::now().timestamp() as u64,
        }
    }
}
