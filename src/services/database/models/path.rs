use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::models::path::SingleChainPathsWithAnchorToken;

/// Pool model for MongoDB
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Path {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub paths: Vec<SingleChainPathsWithAnchorToken>,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted_at: Option<u64>,
}

impl Path {
    pub fn new(paths: Vec<SingleChainPathsWithAnchorToken>) -> Self {
        Self {
            id: None,
            paths,
            created_at: Utc::now().timestamp() as u64,
            updated_at: Utc::now().timestamp() as u64,
            deleted_at: None,
        }
    }
}
