use crate::bitcoin::{TrackedAddr, TrackedTransaction, TransactionState};
use crate::client::Client as ElectrumClient;
use crate::error::Error;
use crate::SATS_IN_BITCOIN;
use bitcoincore_rpc::bitcoin::{Block, BlockHash, Txid};
use bitcoincore_rpc::json::GetRawTransactionResult;
use bitcoincore_rpc::{Auth, Client as BitcoinRpcClient, RpcApi};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, SystemTime};
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
    tracked_addresses: HashMap<String, TrackedAddr>,
    tracked_transactions: HashMap<String, TrackedTransaction>,
}

impl BlockExplorer {
    pub async fn new(config: Config, electrum_client: ElectrumClient) -> Result<Self, Error> {
        let socket_context = SocketContext::new();
        let raw_tx_socket = socket_context.create_subscriber_with_topic(&config.raw_tx_socket, "rawtx".as_bytes());
        let raw_block_socket =
            socket_context.create_subscriber_with_topic(&config.raw_block_socket, "rawblock".as_bytes());
        let auth = Auth::UserPass(config.rpc_user, config.rpc_password);
        let rpc_client =
            BitcoinRpcClient::new(&config.bitcoind_url, auth).map_err(|err| Error { error: err.to_string() })?;
        let mut explorer = Self {
            electrum_client,
            socket_context,
            raw_tx_socket,
            raw_block_socket,
            rpc_client,
            last_block_height: 0,
            last_block_hash: "".to_string(),
            prev_block_hash: "".to_string(),
            min_confs: config.min_confs,
            tracked_addresses: HashMap::new(),
            tracked_transactions: HashMap::new(),
        };
        explorer.initialise().await?;
        Ok(explorer)
    }

    async fn initialise(&mut self) -> Result<(), Error> {
        println!("Initializing electrum wallet history");
        let wallet_txs = self.electrum_client.get_wallet_history().await?;

        println!("Analyzing {} wallet txs", wallet_txs.len());
        for tx in wallet_txs {
            if tx.incoming {
                for output in tx.outputs.iter() {
                    let txid = tx.txid.clone();
                    let address = output.address.clone();
                    let is_mine = self.electrum_client.is_mine(address.clone()).await?;
                    if is_mine {
                        let value = (SATS_IN_BITCOIN * tx.bc_value) as i64;
                        let (_tx_data, fee) = self.get_transaction(&txid).await?;
                        let timestamp = {
                            let sys_time = if tx.timestamp == SystemTime::UNIX_EPOCH {
                                SystemTime::now()
                            } else {
                                tx.timestamp
                            };
                            sys_time
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_else(|err| {
                                    panic!(
                                        "System time should not be set to earlier than epoch start, error: {:?}",
                                        err
                                    )
                                })
                                .as_secs()
                        };
                        let tx_state = TransactionState {
                            uid: 0,
                            txid: txid.clone(),
                            timestamp,
                            address: address.clone(),
                            block_number: tx.height,
                            confirmations: tx.confirmations,
                            fee,
                            tx_type: "Inbound".to_string(),
                            is_confirmed: false,
                            network: "Bitcoin".to_string(),
                            value,
                        };

                        if tx.confirmations < self.min_confs {
                            let tracked_addr = TrackedAddr {
                                uid: 0,
                                timestamp: SystemTime::now(),
                            };
                            self.tracked_addresses.insert(address.clone(), tracked_addr.clone());

                            let tracked_tx = TrackedTransaction {
                                uid: 0,
                                txid: txid.clone(),
                                timestamp: tracked_addr.timestamp,
                                address,
                                block_number: tx.height,
                                fee,
                                tx_type: "Inbound".to_string(),
                                value,
                            };
                            self.tracked_transactions.insert(txid, tracked_tx);

                            // TODO send tx_state to any interested parties - if any
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn get_block(&self, block_hash: &str) -> Result<Block, Error> {
        let hash = BlockHash::from_str(block_hash).map_err(|err| Error { error: err.to_string() })?;
        self.rpc_client
            .get_block(&hash)
            .map_err(|err| Error { error: err.to_string() })
    }

    fn get_block_height(&self) -> Result<u64, Error> {
        self.rpc_client
            .get_block_count()
            .map_err(|err| Error { error: err.to_string() })
    }

    async fn get_transaction(&self, tx_hash: &str) -> Result<(GetRawTransactionResult, i64), Error> {
        let txid = Txid::from_str(tx_hash).map_err(|err| Error { error: err.to_string() })?;
        let tx = self
            .rpc_client
            .get_raw_transaction_info(&txid, None)
            .map_err(|err| Error { error: err.to_string() })?;
        let mut inputs_value: i64 = 0;
        let mut outputs_value: i64 = 0;
        for vin in tx.vin.iter() {
            if let Some(vin_txid) = vin.txid {
                match self.rpc_client.get_raw_transaction_info(&vin_txid, None) {
                    Ok(vin_lookup) => match vin.vout {
                        Some(vin_vout) => {
                            if let Some(vout) = vin_lookup.vout.get(vin_vout as usize) {
                                if let Some(ref addresses) = vout.script_pub_key.addresses {
                                    if !addresses.is_empty() {
                                        inputs_value += vout.value.to_sat() as i64;
                                    }
                                }
                            }
                        }
                        None => {
                            eprintln!(
                                "Failed to get vout from vin lookup txid: {} due to missing vout index, vin dump: {:?}",
                                vin_txid, vin
                            );
                            continue;
                        }
                    },
                    Err(err) => {
                        eprintln!(
                            "Failed to lookup vin txid: {}, error: {}, vin dump: {:?}",
                            vin_txid, err, vin
                        );
                        continue;
                    }
                }
            } else {
                eprintln!("Failed to get txid from vin: {:?}", vin);
                continue;
            }
        }
        for vout in tx.vout.iter() {
            if let Some(ref addresses) = vout.script_pub_key.addresses {
                if !addresses.is_empty() {
                    outputs_value += vout.value.to_sat() as i64;
                }
            }
        }
        let fees = inputs_value - outputs_value;
        Ok((tx, fees))
    }
}
