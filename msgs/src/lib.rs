use rust_decimal::prelude::*;

use serde::{Deserialize, Serialize};

pub mod api;
pub mod bank;
pub mod cli;
pub mod dealer;
pub mod kollider_client;
pub mod nostr;

use api::*;
use bank::*;
use cli::*;
use dealer::*;
use kollider_client::*;
use nostr::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deposit {
    pub payment_request: String,
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
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
