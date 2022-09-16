use serde::{Deserialize, Serialize};
use std::time::SystemTime;

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
