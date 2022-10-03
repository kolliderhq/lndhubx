use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnectorConfig {
    pub electrum_username: String,
    pub electrum_password: String,
    pub electrum_path: String,
    pub electrum_url: String,
    pub raw_tx_socket: String,
    pub raw_block_socket: String,
    pub rpc_user: String,
    pub rpc_password: String,
    pub bitcoind_url: String,
    pub min_confs: i64,
}
