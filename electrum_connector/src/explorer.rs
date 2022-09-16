use crate::client::Client as ElectrumClient;
use crate::error::Error;
use bitcoincore_rpc::{Auth, Client as BitcoinRpcClient};
use utils::xzmq::{SocketContext, ZmqSocket};

const MAX_BLOCK: i64 = 10_000_000_000_000_000;

pub struct Config {
    electrum_username: String,
    electrum_password: String,
    electrum_path: String,
    electrum_url: String,
    raw_tx_socket: String,
    raw_block_socket: String,
    rpc_user: String,
    rpc_password: String,
    bitcoind_url: String,
    min_confs: i64,
}

pub struct BlockExplorer {
    electrum_client: ElectrumClient,
    socket_context: SocketContext,
    raw_tx_socket: ZmqSocket,
    raw_block_socket: ZmqSocket,
    rpc_client: BitcoinRpcClient,
    last_block_height: i64,
    last_block_hash: String,
    prev_block_hash: String,
    min_confs: i64,
}

impl BlockExplorer {
    pub fn new(config: Config, electrum_client: ElectrumClient) -> Result<Self, Error> {
        let socket_context = SocketContext::new();
        let raw_tx_socket = socket_context.create_subscriber(&config.raw_tx_socket);
        let raw_block_socket = socket_context.create_subscriber(&config.raw_block_socket);
        let auth = Auth::UserPass(config.rpc_user, config.rpc_password);
        let rpc_client =
            BitcoinRpcClient::new(&config.bitcoind_url, auth).map_err(|err| Error { error: err.to_string() })?;
        let explorer = Self {
            electrum_client,
            socket_context,
            raw_tx_socket,
            raw_block_socket,
            rpc_client,
            last_block_height: 0,
            last_block_hash: "".to_string(),
            prev_block_hash: "".to_string(),
            min_confs: config.min_confs,
        };
        Ok(explorer)
    }
}
