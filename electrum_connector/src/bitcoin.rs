use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Input {
    pub address: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Output {
    pub address: String,
    pub value: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Transaction {
    pub txid: String,
    pub incoming: bool,
    pub outputs: Vec<Output>,
    pub bc_value: f64,
    pub timestamp: SystemTime,
    pub height: i64,
    pub confirmations: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransactionState {
    pub uid: i64,
    pub txid: String,
    pub timestamp: u64,
    pub address: String,
    pub block_number: i64,
    pub confirmations: i64,
    pub fee: i64,
    pub tx_type: String,
    pub is_confirmed: bool,
    pub network: String,
    pub value: i64,
}

impl From<TrackedTransaction> for TransactionState {
    fn from(tracked_tx: TrackedTransaction) -> Self {
        let timestamp = tracked_tx
            .timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time may not be set to earlier than epoch start")
            .as_secs();
        Self {
            uid: tracked_tx.uid,
            txid: tracked_tx.txid,
            timestamp,
            address: tracked_tx.address,
            block_number: tracked_tx.block_number,
            confirmations: 0,
            fee: tracked_tx.fee,
            tx_type: tracked_tx.tx_type,
            is_confirmed: false,
            network: "Bitcoin".to_string(),
            value: tracked_tx.value,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrackedTransaction {
    pub uid: i64,
    pub txid: String,
    pub timestamp: SystemTime,
    pub address: String,
    pub block_number: i64,
    pub fee: i64,
    pub tx_type: String,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct TrackedAddr {
    pub uid: i64,
    pub timestamp: SystemTime,
}
