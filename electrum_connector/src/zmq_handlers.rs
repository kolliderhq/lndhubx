use crate::bitcoin::{Input, Output, TrackedTransaction};
use crate::tracked_state::SharedState;
use crate::{util, MAX_BLOCK};
use bitcoin::{Block, Transaction};
use bitcoincore_rpc::{Client as BitcoinRpcClient, RpcApi};
use msgs::blockchain::{BcTransactionState, TxType};
use std::time::SystemTime;
use utils::xzmq::ZmqSocket;

struct RawTxMsg {
    txid: String,
    _inputs: Vec<Input>,
    outputs: Vec<Output>,
}

pub fn raw_tx_handler<F>(
    shared_state: SharedState,
    raw_tx_socket: ZmqSocket,
    rpc_client: BitcoinRpcClient,
    mut listener: F,
) where
    F: FnMut(BcTransactionState),
{
    while let Ok(frames) = raw_tx_socket.recv_multipart(0x00) {
        if let Ok(tx) = bitcoin::consensus::encode::deserialize::<Transaction>(&frames[1]) {
            let mut _inputs = Vec::new();
            let mut outputs = Vec::new();
            for vin in tx.input.iter() {
                let vin_txid = vin.previous_output.txid;
                let vin_vout_index = vin.previous_output.vout as usize;
                if !vin_txid.is_empty() {
                    match rpc_client.get_raw_transaction_info(&vin_txid, None) {
                        Ok(vin_lookup) => {
                            if let Some(vout) = vin_lookup.vout.get(vin_vout_index) {
                                if let Some(ref addresses) = vout.script_pub_key.addresses {
                                    if !addresses.is_empty() {
                                        let address = addresses[0].to_string();
                                        _inputs.push(Input { address });
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "Failed to lookup vin txid: {} in raw_tx_handler, error: {}, vin dump: {:?}",
                                vin_txid, err, vin
                            );
                            continue;
                        }
                    }
                }
            }

            for vout in tx.output.iter() {
                if vout.value > 0 {
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
            }

            let raw_tx_msg = RawTxMsg {
                txid: tx.txid().to_string(),
                _inputs,
                outputs,
            };

            for output in raw_tx_msg.outputs.iter() {
                if let Some(tracked_address) = shared_state.lock().unwrap().tracked_addresses.remove(&output.address) {
                    println!("Relevant address found on output side: {}", output.address);
                    let tracked_transactions = &mut shared_state.lock().unwrap().tracked_transactions;
                    if !tracked_transactions.contains_key(&raw_tx_msg.txid) {
                        match util::get_transaction(&rpc_client, &raw_tx_msg.txid) {
                            Ok((_tx, fee)) => {
                                let tracked_tx = TrackedTransaction {
                                    uid: tracked_address.uid,
                                    txid: raw_tx_msg.txid.clone(),
                                    timestamp: SystemTime::now(),
                                    address: output.address.clone(),
                                    block_number: MAX_BLOCK,
                                    fee,
                                    tx_type: TxType::Inbound,
                                    value: output.value,
                                };
                                tracked_transactions.insert(raw_tx_msg.txid.clone(), tracked_tx.clone());
                                let transaction_state = BcTransactionState::from(tracked_tx);
                                listener(transaction_state);
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

pub fn raw_block_handler<F>(
    mut shared_state: SharedState,
    raw_block_socket: ZmqSocket,
    rpc_client: BitcoinRpcClient,
    min_confirmations: i64,
    mut listener: F,
) where
    F: FnMut(BcTransactionState),
{
    while let Ok(frames) = raw_block_socket.recv_multipart(0x00) {
        let block = bitcoin::consensus::deserialize::<Block>(&frames[1]).unwrap();
        println!("New block mined, hash: {}", block.block_hash());
        let height = block.bip34_block_height().expect("Received a invalid block height") as i64;
        let block_hash = block.block_hash().to_string();
        for tx in block.txdata.iter() {
            let txid = tx.txid().to_string();
            if let Some(mut tracked_tx) = shared_state.lock().unwrap().tracked_transactions.get(&txid).cloned() {
                println!(
                    "Relevant tx: {} found at height {} in block {}",
                    txid, height, block_hash
                );
                match util::get_transaction(&rpc_client, &txid) {
                    Ok((_tx_info, fee)) => {
                        tracked_tx.fee = fee;
                        tracked_tx.block_number = height;
                        shared_state
                            .lock()
                            .unwrap()
                            .tracked_transactions
                            .insert(txid.clone(), tracked_tx.clone());
                        let mut transaction_state = BcTransactionState::from(tracked_tx);
                        transaction_state.confirmations = 1;
                        listener(transaction_state);
                    }
                    Err(err) => {
                        println!("Failed to get transaction: {}, error: {:?}", txid, err);
                        continue;
                    }
                }
            }
        }
        check_tx_confirmations(&mut shared_state, height, min_confirmations, &mut listener);
    }
}

struct TxToRemove {
    txid: String,
    tracked_tx: TrackedTransaction,
}

fn check_tx_confirmations<F>(shared_state: &mut SharedState, height: i64, min_confirmations: i64, listener: &mut F)
where
    F: FnMut(BcTransactionState),
{
    let mut transaction_to_remove = Vec::new();
    for (txid, tracked_tx) in shared_state.lock().unwrap().tracked_transactions.iter() {
        if tracked_tx.block_number == 0 {
            continue;
        }
        let confirmations = height + 1 - tracked_tx.block_number;
        if confirmations > 1 {
            let mut transaction_state = BcTransactionState::from(tracked_tx.clone());
            transaction_state.confirmations = confirmations;
            println!(
                "Tx state: {} updated by check_tx_confs, confirmations: {}",
                txid, confirmations
            );
            listener(transaction_state);
        }
        if confirmations > min_confirmations {
            println!(
                "Tx: {} confirmations: {} more than threshold: {}, Removing from tracker",
                txid, confirmations, min_confirmations
            );
            transaction_to_remove.push(TxToRemove {
                txid: txid.clone(),
                tracked_tx: tracked_tx.clone(),
            });
        }
    }
    for tx in transaction_to_remove {
        shared_state.lock().unwrap().tracked_transactions.remove(&tx.txid);
        shared_state
            .lock()
            .unwrap()
            .tracked_addresses
            .remove(&tx.tracked_tx.address);
    }
}
