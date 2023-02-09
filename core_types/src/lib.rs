use diesel::{r2d2::ConnectionManager, PgConnection};
use rust_decimal::prelude::*;
use rust_decimal_macros::*;
use uuid::Uuid;

use std::fmt;

use crate::kollider_client::Side;
use serde::{Deserialize, Serialize};

pub mod kollider_client;
pub mod nostr;

pub const SATS_IN_BITCOIN: Decimal = dec!(100000000.0);
pub const RATE_DP: u32 = 16;

#[derive(Debug, Clone, Copy)]
pub enum TxState {
    Pending,
    Confirmed,
}

#[derive(Debug, Clone, Copy)]
pub enum TxType {
    Internal,
    External,
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

        write!(f, "{sign}")
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub enum AccountClass {
    Cash,
    Fees,
}

impl fmt::Display for AccountClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = match self {
            Self::Cash => "Cash",
            Self::Fees => "Fee",
        };

        write!(f, "{sign}")
    }
}

impl FromStr for AccountClass {
    type Err = String;

    fn from_str(account_type: &str) -> Result<AccountClass, Self::Err> {
        match account_type {
            "Cash" => Ok(AccountClass::Cash),
            "Fees" => Ok(AccountClass::Fees),
            _ => Err("unknown account class".to_string()),
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
    KKP,
}

#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub enum Denom {
    Sats(u64),
    MilliCents(u64),
    MilliPence(u64),
    Karma(u64),
}

impl Denom {
    pub fn from_currency(currency: Currency) -> Self {
        match currency {
            Currency::BTC => Denom::Sats(100000000),
            Currency::USD => Denom::MilliCents(100000),
            Currency::GBP => Denom::MilliPence(100000),
            Currency::EUR => Denom::MilliCents(100000),
            Currency::KKP => Denom::Karma(1),
        }
    }

    pub fn into_inner(&self) -> u64 {
        match self {
            Self::Sats(v) => *v,
            Self::MilliCents(v) => *v,
            Self::MilliPence(v) => *v,
            Self::Karma(v) => *v,
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
            Self::KKP => "KKP",
        };

        write!(f, "{sign}")
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
            "kkp" => Ok(Currency::KKP),
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
            Currency::BTC | Currency::KKP => panic!("Incorrect usage"),
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
    pub account_class: AccountClass,
}

impl Account {
    pub fn new(currency: Currency, account_type: AccountType, account_class: AccountClass) -> Self {
        Self {
            currency,
            account_type,
            account_class,
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
    Nostr,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LndNodeInfo {
    pub identity_pubkey: String,
    pub uris: Vec<String>,
    pub num_active_channels: u64,
    pub num_pending_channels: u64,
    pub num_peers: u64,
    pub testnet: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Money {
    pub value: Decimal,
    pub currency: Currency,
}

impl Money {
    pub fn new(currency: Currency, value: Option<Decimal>) -> Self {
        Self {
            currency,
            value: value.unwrap_or(dec!(0)),
        }
    }

    pub fn set(&mut self, value: Decimal) {
        self.value = value;
    }

    pub fn mult(&self, value: Decimal) -> Decimal {
        self.value * value
    }

    pub fn div(&self, value: Decimal) -> Decimal {
        self.value / value
    }

    pub fn try_sats(&self) -> Result<Decimal, String> {
        if self.currency == Currency::BTC {
            Ok(self.value * SATS_IN_BITCOIN)
        } else {
            Err("Is not Bitcoin.".to_string())
        }
    }
    pub fn from_sats(value: Decimal) -> Self {
        Self {
            currency: Currency::BTC,
            value: value / SATS_IN_BITCOIN,
        }
    }

    pub fn exchange(&self, rate: &Rate) -> Result<Money, String> {
        let mut r = rate.value;
        let mut c = rate.quote;
        // We have to flip the rate if currencies not align.
        if self.currency != rate.base {
            r = dec!(1) / r;
            r = r.round_dp_with_strategy(RATE_DP, RoundingStrategy::ToZero);
            c = rate.base;
        }
        let exchanged_money = Money {
            currency: c,
            value: self.value * r,
        };
        Ok(exchanged_money)
    }

    pub fn from_btc(value: Decimal) -> Self {
        Self {
            currency: Currency::BTC,
            value,
        }
    }
}

impl FromStr for Money {
    type Err = String;

    fn from_str(currency: &str) -> Result<Money, Self::Err> {
        let currency = currency.to_lowercase();
        match &currency[..] {
            "btc" => Ok(Money::new(Currency::BTC, None)),
            "eur" => Ok(Money::new(Currency::EUR, None)),
            "gbp" => Ok(Money::new(Currency::GBP, None)),
            "usd" => Ok(Money::new(Currency::USD, None)),
            _ => Err("unknown money".to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rate {
    pub value: Decimal,
    pub quote: Currency,
    pub base: Currency,
}

impl Rate {
    pub fn new(base: Currency, quote: Currency, value: Decimal) -> Self {
        Self { quote, base, value }
    }

    pub fn set(&mut self, value: Decimal) {
        self.value = value;
    }

    pub fn get_inv(&self) -> Rate {
        Rate {
            base: self.quote,
            quote: self.base,
            value: Decimal::ONE / self.value,
        }
    }
}

impl Default for Rate {
    fn default() -> Self {
        Self {
            value: Decimal::MIN,
            quote: Currency::BTC,
            base: Currency::BTC,
        }
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
