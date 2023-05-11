use rust_decimal::prelude::*;

use serde::{Deserialize, Serialize};

pub mod api;
pub mod bank;
pub mod cli;
pub mod dealer;
pub mod kollider_client;
pub mod nostr;
pub mod journal;

use api::*;
use bank::*;
use cli::*;
use dealer::*;
use kollider_client::*;
use nostr::*;
use journal::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deposit {
    pub payment_request: String,
    pub payment_hash: String,
    pub description_hash: String,
    pub preimage: String,
    pub value: u64,
    pub value_msat: u64,
    pub settled: bool,
    pub creation_date: u64,
    pub settle_date: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Withdrawal {
    pub amount: Decimal,
    pub invoice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Api(Api),
    Deposit(Deposit),
    Dealer(Dealer),
    KolliderApiResponse(KolliderApiResponse),
    Bank(Bank),
    Cli(Cli),
    Nostr(Nostr),
    Journal(Journal)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
