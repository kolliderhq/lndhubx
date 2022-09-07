use core_types::{AccountId, Currency, UserId};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Cli {
    MakeTx(MakeTx),
    MakeTxResult(MakeTxResult),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MakeTx {
    pub outbound_uid: UserId,
    pub outbound_account_id: AccountId,
    pub inbound_uid: UserId,
    pub inbound_account_id: AccountId,
    pub amount: Decimal,
    pub currency: Currency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MakeTxResult {
    pub tx: MakeTx,
    pub result: String,
}
