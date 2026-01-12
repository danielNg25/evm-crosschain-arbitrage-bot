/// Helper function to convert an Address to a string for MongoDB
pub fn address_to_string(address: &Address) -> String {
    format!("{:?}", address)
}

/// Helper function to convert a U256 to a string for MongoDB
pub fn u256_to_string(value: &U256) -> String {
    value.to_string()
}
use alloy::primitives::{Address, U256};
