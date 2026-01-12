pub mod registry;
pub use registry::*;

// use crate::models::{pool::Pool, token::Token};
// use alloy_primitives::{Address, U256};
// use serde::{Deserialize, Serialize};
// use std::fmt;
// use std::hash::{Hash, Hasher};

// /// Represents a swap in a path
// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct Swap {
//     /// Pool to use for the swap
//     pub pool: Address,
//     /// Token to swap from
//     pub token_in: Address,
//     /// Token to swap to
//     pub token_out: Address,
//     /// Fee for this swap
//     pub fee: f64,
// }

// impl PartialEq for Swap {
//     fn eq(&self, other: &Self) -> bool {
//         self.pool == other.pool
//             && self.token_in == other.token_in
//             && self.token_out == other.token_out
//     }
// }

// impl Eq for Swap {}

// impl Hash for Swap {
//     fn hash<H: Hasher>(&self, state: &mut H) {
//         self.pool.hash(state);
//         self.token_in.hash(state);
//         self.token_out.hash(state);
//     }
// }

// /// Represents a complete arbitrage path
// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct ArbPath {
//     /// Unique identifier for this path
//     pub id: String,
//     /// Starting token (must be the same as ending token for cycles)
//     pub start_token: Address,
//     /// Series of swaps to execute
//     pub swaps: Vec<Swap>,
//     /// Cached path string for display
//     pub path_string: String,
// }

// /// Result of a path simulation
// #[derive(Debug, Clone)]
// pub struct SimulationResult {
//     /// Input amount
//     pub input_amount: U256,
//     /// Final output amount
//     pub output_amount: U256,
//     /// Profit (if any)
//     pub profit: f64,
//     /// Gas cost estimate
//     pub gas_cost: f64,
//     /// Net profit (profit - gas_cost)
//     pub net_profit: f64,
//     /// Whether this trade is profitable
//     pub profitable: bool,
//     /// Timestamp of simulation
//     pub timestamp: u64,
// }

// impl SimulationResult {
//     /// Check if the path is profitable after gas costs
//     pub fn is_profitable(&self) -> bool {
//         self.profitable
//     }
// }

// impl ArbPath {
//     /// Create a new arbitrage path
//     pub fn new(start_token: Address, swaps: Vec<Swap>) -> Self {
//         let id = format!("{:?}-{}", start_token, swaps.len());
//         let path_string = Self::generate_path_string(&start_token, &swaps);

//         Self {
//             id,
//             start_token,
//             swaps,
//             path_string,
//         }
//     }

//     /// Generate a human-readable path string
//     fn generate_path_string(start_token: &Address, swaps: &[Swap]) -> String {
//         if swaps.is_empty() {
//             return start_token.to_string();
//         }

//         let mut path = vec![start_token.to_string()];

//         for swap in swaps {
//             path.push(swap.token_out.to_string());
//         }

//         path.join(" → ")
//     }

//     /// Check if the path is valid (starts and ends with the same token)
//     pub fn is_valid_cycle(&self) -> bool {
//         if self.swaps.is_empty() {
//             return false;
//         }

//         let last_swap = &self.swaps[self.swaps.len() - 1];
//         last_swap.token_out == self.start_token
//     }

//     /// Get the length of the path (number of swaps)
//     pub fn len(&self) -> usize {
//         self.swaps.len()
//     }

//     /// Check if the path is empty
//     pub fn is_empty(&self) -> bool {
//         self.swaps.is_empty()
//     }
// }

// impl fmt::Display for ArbPath {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         write!(f, "{}", self.path_string)
//     }
// }

// impl PartialEq for ArbPath {
//     fn eq(&self, other: &Self) -> bool {
//         self.id == other.id
//     }
// }

// impl Eq for ArbPath {}

// impl Hash for ArbPath {
//     fn hash<H: Hasher>(&self, state: &mut H) {
//         self.id.hash(state);
//     }
// }
