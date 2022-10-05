use core_types::{Network, ServiceIdentity, TxType};
use serde::{Deserialize, Serialize};

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
    pub requesting_identity: ServiceIdentity,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BtcReceiveAddress {
    pub uid: u64,
    // it is None when an address could not be provided by the connector
    // and it should be treated as an error response
    pub address: Option<String>,
    pub requesting_identity: ServiceIdentity,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Blockchain {
    BcTransactionState(BcTransactionState),
    BtcReceiveAddressRequest(BtcReceiveAddressRequest),
    BtcReceiveAddress(BtcReceiveAddress),
}
