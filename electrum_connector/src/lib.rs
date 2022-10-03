pub mod bitcoin;
pub mod connector_config;
pub mod electrum_client;
pub mod error;
pub mod explorer;
pub mod messages;
pub mod tracked_state;
pub mod util;
pub mod zmq_handlers;

pub const SATS_IN_BITCOIN: f64 = 100_000_000.0;
pub const MAX_BLOCK: i64 = 10_000_000_000_000_000;
