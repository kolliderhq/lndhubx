use crate::bitcoin::{Input, Output, TrackedAddr, TrackedTransaction, TransactionState};
use crate::connector_config::ConnectorConfig;
use crate::electrum_client::ElectrumClient;
use crate::error::Error;
use crate::SATS_IN_BITCOIN;
use bitcoincore_rpc::bitcoin::{Block, BlockHash, Txid};
use bitcoincore_rpc::json::GetRawTransactionResult;
use bitcoincore_rpc::{Auth, Client as BitcoinRpcClient, RpcApi};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::SystemTime;
use utils::xzmq::{SocketContext, ZmqSocket};

const MAX_BLOCK: i64 = 10_000_000_000_000_000;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RawTxMsg {
    txid: String,
    inputs: Vec<Input>,
    outputs: Vec<Output>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RawBlockMsg {
    block_hash: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct TxToRemove {
    txid: String,
    tracked_tx: TrackedTransaction,
}

pub struct BlockExplorer {
    electrum_client: ElectrumClient,
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
    pub async fn new(
        config: ConnectorConfig,
        zmq_context: SocketContext,
        electrum_client: ElectrumClient,
    ) -> Result<Self, Error> {
        let raw_tx_socket = zmq_context.create_subscriber_with_topic(&config.raw_tx_socket, "rawtx".as_bytes());
        let raw_block_socket =
            zmq_context.create_subscriber_with_topic(&config.raw_block_socket, "rawblock".as_bytes());
        let auth = Auth::UserPass(config.rpc_user, config.rpc_password);
        let rpc_client =
            BitcoinRpcClient::new(&config.bitcoind_url, auth).map_err(|err| Error { error: err.to_string() })?;
        let mut explorer = Self {
            electrum_client,
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
                        let transaction_state = TransactionState {
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
                            self.send_tx_state(transaction_state);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn send_tx_state(&self, tx_state: TransactionState) {
        // TODO send tx_state to any interested parties - if any
        println!("TODO Send transaction state to any interested parties: {:?}", tx_state);
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

    pub async fn raw_tx_handler(&mut self) {
        while let Ok(frames) = self.raw_tx_socket.recv_multipart(0x00) {
            if let Ok(tx) =
                <bitcoin::blockdata::transaction::Transaction as bitcoin::psbt::serialize::Deserialize>::deserialize(
                    &frames[1],
                )
            {
                let mut inputs = Vec::new();
                let mut outputs = Vec::new();
                for vin in tx.input.iter() {
                    let vin_txid = vin.previous_output.txid;
                    let vin_vout_index = vin.previous_output.vout as usize;
                    if !vin_txid.is_empty() {
                        match self.rpc_client.get_raw_transaction_info(&vin_txid, None) {
                            Ok(vin_lookup) => {
                                if let Some(vout) = vin_lookup.vout.get(vin_vout_index) {
                                    if let Some(ref addresses) = vout.script_pub_key.addresses {
                                        if !addresses.is_empty() {
                                            let address = addresses[0].to_string();
                                            inputs.push(Input { address });
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                eprintln!(
                                    "Failed to lookup vin txid: {}, error: {}, vin dump: {:?}",
                                    vin_txid, err, vin
                                );
                                continue;
                            }
                        }
                    }
                }

                for vout in tx.output.iter() {
                    match bitcoin::util::address::Address::from_script(&vout.script_pubkey, bitcoin::Network::Bitcoin) {
                        Ok(address) => {
                            let value = vout.value as i64;
                            outputs.push(Output {
                                address: address.to_string(),
                                value,
                            });
                        }
                        Err(err) => {
                            eprintln!("Failed to get address for vout's script {:?}, error: {:?}", vout, err);
                            continue;
                        }
                    }
                }

                let raw_tx_msg = RawTxMsg {
                    txid: tx.txid().to_string(),
                    inputs,
                    outputs,
                };

                for output in raw_tx_msg.outputs.iter() {
                    if let Some(tracked_address) = self.tracked_addresses.remove(&output.address) {
                        println!("Relevant address found on output side: {}", output.address);
                        if !self.tracked_transactions.contains_key(&raw_tx_msg.txid) {
                            match self.get_transaction(&raw_tx_msg.txid).await {
                                Ok((_tx, fee)) => {
                                    let tracked_tx = TrackedTransaction {
                                        uid: tracked_address.uid,
                                        txid: raw_tx_msg.txid.clone(),
                                        timestamp: SystemTime::now(),
                                        address: output.address.clone(),
                                        block_number: MAX_BLOCK,
                                        fee,
                                        tx_type: "Inbound".to_string(),
                                        value: output.value,
                                    };
                                    self.tracked_transactions
                                        .insert(raw_tx_msg.txid.clone(), tracked_tx.clone());
                                    let transaction_state = TransactionState::from(tracked_tx);
                                    self.send_tx_state(transaction_state);
                                }
                                Err(err) => {
                                    println!("Failed to get transaction on {}: {:?}", raw_tx_msg.txid, err);
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub async fn raw_block_handler(&mut self) {
        while let Ok(frames) = self.raw_block_socket.recv_multipart(0x00) {
            let block = bitcoin::consensus::deserialize::<Block>(&frames[1]).unwrap();
            println!("New block mined, hash: {}", block.block_hash());
            let height = block.bip34_block_height().expect("Received a invalid block height") as i64;
            let block_hash = block.block_hash().to_string();
            self.last_block_hash = block_hash.clone();
            self.last_block_height = height;
            self.prev_block_hash = block.header.prev_blockhash.to_string();
            for tx in block.txdata.iter() {
                let txid = tx.txid().to_string();
                if let Some(mut tracked_tx) = self.tracked_transactions.get(&txid).cloned() {
                    println!(
                        "Relevant tx: {} found at height {} in block {}",
                        txid, height, block_hash
                    );
                    match self.get_transaction(&txid).await {
                        Ok((_tx_info, fee)) => {
                            tracked_tx.fee = fee;
                            tracked_tx.block_number = height;
                            self.tracked_transactions.insert(txid.clone(), tracked_tx.clone());
                            let mut transaction_state = TransactionState::from(tracked_tx);
                            transaction_state.confirmations = 1;
                            self.send_tx_state(transaction_state);
                        }
                        Err(err) => {
                            println!("Failed to get transaction: {}, error: {:?}", txid, err);
                            continue;
                        }
                    }
                }
            }
            self.check_tx_confirmations().await;
        }
    }

    async fn check_tx_confirmations(&mut self) {
        let mut transaction_to_remove = Vec::new();
        let height = self.last_block_height;
        for (txid, tracked_tx) in self.tracked_transactions.iter() {
            if tracked_tx.block_number == 0 {
                continue;
            }
            let confirmations = height + 1 - tracked_tx.block_number;
            if confirmations > 1 {
                let mut transaction_state = TransactionState::from(tracked_tx.clone());
                transaction_state.confirmations = confirmations;
                println!(
                    "Tx state: {} updated by check_tx_confs, confirmations: {}",
                    txid, confirmations
                );
                self.send_tx_state(transaction_state);
            }
            if confirmations > self.min_confs {
                println!(
                    "Tx: {} confirmations: {} more than threshold: {}, Removing from tracker",
                    txid, confirmations, self.min_confs
                );
                transaction_to_remove.push(TxToRemove {
                    txid: txid.clone(),
                    tracked_tx: tracked_tx.clone(),
                });
            }
        }
        for tx in transaction_to_remove {
            self.tracked_transactions.remove(&tx.txid);
            self.tracked_addresses.remove(&tx.tracked_tx.address);
        }
    }
}
