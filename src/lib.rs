pub mod blockchain;
pub mod config;
pub mod core;
pub mod models;
pub mod services;
pub mod utils;

// Re-export commonly used types and functions
pub use crate::models::pool::{PoolInterface, PoolType, UniswapV2Pool};
pub use crate::models::token::Token;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
