use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum TxType {
    Inbound,
    Outbound,
    Internal,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum Network {
    Bitcoin,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BcTransactionState {
    pub uid: u64,
    pub txid: String,
    pub timestamp: u64,
    pub address: String,
    pub block_number: i64,
    pub confirmations: i64,
    pub fee: i64,
    pub tx_type: TxType,
    pub is_confirmed: bool,
    pub network: Network,
    pub value: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BtcReceiveAddressRequest {
    pub uid: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BtcReceiveAddress {
    pub uid: u64,
    pub address: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Blockchain {
    BcTransactionState(BcTransactionState),
    BtcReceiveAddressRequest(BtcReceiveAddressRequest),
    BtcReceiveAddress(BtcReceiveAddress),
}
