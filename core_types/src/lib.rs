use diesel::{r2d2::ConnectionManager, PgConnection};
use rust_decimal::prelude::*;
use rust_decimal_macros::*;
use uuid::Uuid;

use std::fmt;

use crate::kollider_client::Side;
use serde::{Deserialize, Serialize};

pub mod kollider_client;

pub const SATS_IN_BITCOIN: Decimal = dec!(100000000.0);

#[derive(Debug, Clone, Copy)]
pub enum TxState {
    Pending,
    Confirmed,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub enum TxType {
    Internal,
    Inbound,
    Outbound,
}

impl fmt::Display for TxType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = match self {
            Self::Internal => "Internal",
            Self::Inbound => "Inbound",
            Self::Outbound => "Outbound",
        };

        write!(f, "{}", sign)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub enum AccountType {
    Internal,
    External,
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = match self {
            Self::Internal => "Internal",
            Self::External => "External",
        };

        write!(f, "{}", sign)
    }
}

impl FromStr for AccountType {
    type Err = String;

    fn from_str(currency: &str) -> Result<AccountType, Self::Err> {
        match currency {
            "Internal" => Ok(AccountType::Internal),
            "External" => Ok(AccountType::External),
            _ => Err("unknown account type".to_string()),
        }
    }
}

/// Available currencies.
#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize, Eq, Hash)]
pub enum Currency {
    USD,
    GBP,
    EUR,
    BTC,
}

#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub enum Denom {
    Sats(u64),
    MilliCents(u64),
    MilliPence(u64),
}

impl Denom {
    pub fn from_currency(currency: Currency) -> Self {
        match currency {
            Currency::BTC => Denom::Sats(100000000),
            Currency::USD => Denom::MilliCents(100000),
            Currency::GBP => Denom::MilliPence(100000),
            Currency::EUR => Denom::MilliCents(100000),
        }
    }

    pub fn into_inner(&self) -> u64 {
        match self {
            Self::Sats(v) => *v,
            Self::MilliCents(v) => *v,
            Self::MilliPence(v) => *v,
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = match self {
            Self::BTC => "BTC",
            Self::USD => "USD",
            Self::GBP => "GBP",
            Self::EUR => "EUR",
        };

        write!(f, "{}", sign)
    }
}

impl FromStr for Currency {
    type Err = String;

    fn from_str(currency: &str) -> Result<Currency, Self::Err> {
        let currency = currency.to_lowercase();
        match &currency[..] {
            "btc" => Ok(Currency::BTC),
            "eur" => Ok(Currency::EUR),
            "gbp" => Ok(Currency::GBP),
            "usd" => Ok(Currency::USD),
            _ => Err("unknown currency".to_string()),
        }
    }
}

impl From<Currency> for Symbol {
    fn from(currency: Currency) -> Self {
        match currency {
            Currency::USD => String::from("BTCUSD.PERP"),
            Currency::EUR => String::from("BTCEUR.PERP"),
            Currency::GBP => String::from("BTCGBP.PERP"),
            Currency::BTC => panic!("Incorrect usage"),
        }
    }
}

pub type AccountId = Uuid;
pub type RequestId = Uuid;
pub type Txid = Uuid;
pub type UserId = u64;
pub type Symbol = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub account_id: AccountId,
    pub balance: Decimal,
    pub currency: Currency,
    pub account_type: AccountType,
}

impl Account {
    pub fn new(currency: Currency, account_type: AccountType) -> Self {
        Self {
            currency,
            account_type,
            balance: dec!(0),
            account_id: Uuid::new_v4(),
        }
    }
}

pub type DbPool = r2d2::Pool<ConnectionManager<PgConnection>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceIdentity {
    Api,
    LndConnector,
    BankEngine,
    Dealer,
    Loopback,
    ElectrumConnector,
    Cli,
}

#[derive(Debug, Clone)]
pub struct ConversionInfo {
    pub from: Currency,
    pub to: Currency,
    pub base: Currency,
    pub quote: Currency,
    pub symbol: Symbol,
    pub side: Side,
}

impl ConversionInfo {
    pub fn new(from: Currency, to: Currency) -> Self {
        if from == to {
            panic!("Conversion between the same currency is not supported")
        }
        if !matches!(from, Currency::BTC) && !matches!(to, Currency::BTC) {
            panic!("Conversions must involve BTC")
        }
        let fiat = {
            if matches!(from, Currency::BTC) {
                to
            } else {
                from
            }
        };
        let symbol: Symbol = fiat.into();
        let side = {
            if to == fiat {
                Side::Ask
            } else {
                Side::Bid
            }
        };
        Self {
            from,
            to,
            base: Currency::BTC,
            quote: fiat,
            symbol,
            side,
        }
    }

    pub fn is_linear(&self) -> bool {
        self.base == self.from
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LndNodeInfo {
    pub identity_pubkey: String,
    pub uris: Vec<String>,
    pub num_active_channels: u64,
    pub num_pending_channels: u64,
    pub num_peers: u64,
    pub testnet: bool,
}

impl Default for LndNodeInfo {
    fn default() -> Self {
        Self {
            identity_pubkey: String::from(""),
            uris: vec![],
            num_active_channels: 0,
            num_pending_channels: 0,
            num_peers: 0,
            testnet: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub enum Network {
    Bitcoin,
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Bitcoin => "Bitcoin",
        };
        write!(f, "{}", text)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
