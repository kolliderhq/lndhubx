use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub txid: String,
	pub outbound_currency: String,
	pub inbound_currency: String,
	pub outbound_amount: f64,
	pub inbound_amount: f64,
	pub outbound_uid: u64,
	pub inbound_uid: u64,
	pub tx_type: String,
	pub fees: f64,
	pub exchange_rate: f64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Journal {
    Transaction(Transaction),
}
