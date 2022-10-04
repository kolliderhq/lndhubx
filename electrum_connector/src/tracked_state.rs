use crate::bitcoin::{TrackedAddr, TrackedTransaction};
use crate::electrum_client::ElectrumClient;
use crate::error::Error;
use crate::{util, SATS_IN_BITCOIN};
use bitcoincore_rpc::Client as BitcoinRpcClient;
use crossbeam_channel::Sender;
use msgs::blockchain::{BcTransactionState, Network, TxType};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub struct State {
    pub tracked_addresses: HashMap<String, TrackedAddr>,
    pub tracked_transactions: HashMap<String, TrackedTransaction>,
}

impl State {
    pub async fn initialise(
        &mut self,
        electrum_client: &ElectrumClient,
        rpc_client: &BitcoinRpcClient,
        min_confirmations: i64,
        event_sender: Sender<BcTransactionState>,
    ) -> Result<(), Error> {
        println!("Initializing electrum wallet history");
        let wallet_txs = electrum_client.get_wallet_history().await?;

        println!("Analyzing {} wallet txs", wallet_txs.len());
        for tx in wallet_txs {
            if tx.incoming {
                for output in tx.outputs.iter() {
                    let txid = tx.txid.clone();
                    let address = output.address.clone();
                    let is_mine = electrum_client.is_mine(address.clone()).await?;
                    if is_mine {
                        let value = (SATS_IN_BITCOIN * tx.bc_value) as i64;
                        let (_tx_data, fee) = util::get_transaction(rpc_client, &txid)?;
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
                        let transaction_state = BcTransactionState {
                            uid: 0,
                            txid: txid.clone(),
                            timestamp,
                            address: address.clone(),
                            block_number: tx.height,
                            confirmations: tx.confirmations,
                            fee,
                            tx_type: TxType::Inbound,
                            is_confirmed: false,
                            network: Network::Bitcoin,
                            value,
                        };

                        if tx.confirmations < min_confirmations {
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
                                tx_type: TxType::Inbound,
                                value,
                            };
                            self.tracked_transactions.insert(txid, tracked_tx);
                            event_sender.send(transaction_state).unwrap_or_else(|err| {
                                panic!(
                                    "Failed to push transaction state into the channel during initialisation: {:?}",
                                    err
                                )
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub type SharedState = Arc<Mutex<State>>;
