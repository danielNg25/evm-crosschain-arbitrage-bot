pub mod event_queue;
pub mod network_registry;
mod pool_updater_latest_block;
pub mod pool_updater_websocket;
pub mod websocket_listener;
pub use event_queue::{create_event_queue, EventQueue};
pub use pool_updater_latest_block::*;
pub mod network_collector;
pub use network_collector::*;
pub use network_registry::*;
pub use websocket_listener::WebsocketListener;

use alloy::primitives::Address;

pub struct PoolChange {
    pub chain_id: u64,
    pub pool_address: Address,
    pub block_number: u64,
}
