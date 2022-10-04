use crate::connector_config::ConnectorConfig;
use crate::electrum_client::ElectrumClient;
use crate::error::Error;
use crate::tracked_state::State;
use crate::{util, zmq_handlers};
use bitcoincore_rpc::bitcoin::{Block, BlockHash};
use bitcoincore_rpc::json::GetRawTransactionResult;
use bitcoincore_rpc::{Auth, Client as BitcoinRpcClient, RpcApi};
use msgs::blockchain::BcTransactionState;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use utils::xzmq::SocketContext;

pub struct BlockExplorer {
    rpc_client: BitcoinRpcClient,
}

impl BlockExplorer {
    pub async fn new<F>(
        config: ConnectorConfig,
        zmq_context: &SocketContext,
        electrum_client: &ElectrumClient,
        mut listener: F,
    ) -> Result<Self, Error>
    where
        F: FnMut(BcTransactionState) + Send + 'static,
    {
        let shared_state = Arc::new(Mutex::new(State {
            tracked_addresses: HashMap::new(),
            tracked_transactions: HashMap::new(),
        }));

        let auth = Auth::UserPass(config.rpc_user, config.rpc_password);
        let rpc_client = BitcoinRpcClient::new(&config.bitcoind_url, auth.clone())
            .map_err(|err| Error { error: err.to_string() })?;

        let (events_tx, events_rx) = crossbeam_channel::bounded(2048);

        shared_state
            .lock()
            .unwrap()
            .initialise(electrum_client, &rpc_client, config.min_confs, events_tx.clone())
            .await?;

        let raw_tx_shared_state = shared_state.clone();
        let raw_tx_socket = zmq_context.create_subscriber_with_topic(&config.raw_tx_socket, "rawtx".as_bytes());
        let raw_tx_handler_rpc_client = BitcoinRpcClient::new(&config.bitcoind_url, auth.clone())
            .map_err(|err| Error { error: err.to_string() })?;
        let raw_tx_events = events_tx.clone();
        let raw_tx_listener = move |tx_state| {
            raw_tx_events
                .send(tx_state)
                .unwrap_or_else(|err| panic!("Failed to push raw tx event into the channel: {:?}", err));
        };
        std::thread::spawn(move || {
            zmq_handlers::raw_tx_handler(
                raw_tx_shared_state,
                raw_tx_socket,
                raw_tx_handler_rpc_client,
                raw_tx_listener,
            );
        });

        let raw_block_shared_state = shared_state.clone();
        let raw_block_socket =
            zmq_context.create_subscriber_with_topic(&config.raw_block_socket, "rawblock".as_bytes());
        let raw_block_handler_rpc_client =
            BitcoinRpcClient::new(&config.bitcoind_url, auth).map_err(|err| Error { error: err.to_string() })?;
        let raw_block_listener = move |tx_state| {
            events_tx
                .send(tx_state)
                .unwrap_or_else(|err| panic!("Failed to push raw block event into the channel: {:?}", err));
        };
        std::thread::spawn(move || {
            zmq_handlers::raw_block_handler(
                raw_block_shared_state,
                raw_block_socket,
                raw_block_handler_rpc_client,
                config.min_confs,
                raw_block_listener,
            );
        });

        std::thread::spawn(move || {
            while let Ok(event) = events_rx.recv() {
                listener(event);
            }
        });

        Ok(Self { rpc_client })
    }

    pub fn get_block(&self, block_hash: &str) -> Result<Block, Error> {
        let hash = BlockHash::from_str(block_hash).map_err(|err| Error { error: err.to_string() })?;
        self.rpc_client
            .get_block(&hash)
            .map_err(|err| Error { error: err.to_string() })
    }

    pub fn get_block_height(&self) -> Result<u64, Error> {
        self.rpc_client
            .get_block_count()
            .map_err(|err| Error { error: err.to_string() })
    }

    pub fn get_transaction(&self, tx_hash: &str) -> Result<(GetRawTransactionResult, i64), Error> {
        util::get_transaction(&self.rpc_client, tx_hash)
    }
}
