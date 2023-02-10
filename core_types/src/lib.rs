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
}

impl Currency {
    pub fn dp(&self) -> u32 {
        match self {
            Currency::BTC => 12,
            Currency::USD => 6,
            Currency::EUR => 6,
            Currency::GBP => 6,
        }
    }
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

    pub fn normalize(&mut self) {
        self.balance = Money::normalized_value(self.balance, self.currency);
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
    value: Decimal,
    currency: Currency,
}

impl Money {
    pub fn new(currency: Currency, value: Decimal) -> Self {
        let value = value
            .round_dp_with_strategy(currency.dp(), RoundingStrategy::ToZero)
            .normalize();
        Self { currency, value }
    }

    pub fn zero(currency: Currency) -> Self {
        Self::new(currency, Decimal::ZERO)
    }

    pub fn value(&self) -> Decimal {
        self.value
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    pub fn set(&mut self, value: Decimal) {
        self.value = value;
        self.normalize();
    }

    pub fn mult(&mut self, value: Decimal) {
        self.value *= value;
        self.normalize();
    }

    pub fn div(&mut self, value: Decimal) {
        self.value /= value;
        self.normalize();
    }

    pub fn try_sats(&self) -> Result<Decimal, String> {
        if self.currency == Currency::BTC {
            Ok(self.value * SATS_IN_BITCOIN)
        } else {
            Err("Is not Bitcoin.".to_string())
        }
    }

    pub fn from_sats(value: Decimal) -> Self {
        let currency = Currency::BTC;
        let value = Self::normalized_value(value / SATS_IN_BITCOIN, currency);
        Self { currency, value }
    }

    pub fn exchange(&self, rate: &Rate) -> Result<Money, String> {
        let mut r = rate.value;
        let mut c = rate.quote;
        // We have to flip the rate if currencies not align.
        if self.currency != rate.base {
            r = dec!(1) / r;
            c = rate.base;
        }
        let mut exchanged_money = Money {
            currency: c,
            value: self.value * r,
        };
        exchanged_money.normalize();
        Ok(exchanged_money)
    }

    pub fn from_btc(value: Decimal) -> Self {
        let currency = Currency::BTC;
        let value = Self::normalized_value(value, currency);
        Self { currency, value }
    }

    fn normalize(&mut self) {
        self.value = Self::normalized_value(self.value, self.currency);
    }

    fn normalized_value(value: Decimal, currency: Currency) -> Decimal {
        value.round_dp_with_strategy(currency.dp(), RoundingStrategy::ToZero)
    }
}

impl FromStr for Money {
    type Err = String;

    fn from_str(currency: &str) -> Result<Money, Self::Err> {
        let currency = currency.to_lowercase();
        match &currency[..] {
            "btc" => Ok(Money::zero(Currency::BTC)),
            "eur" => Ok(Money::zero(Currency::EUR)),
            "gbp" => Ok(Money::zero(Currency::GBP)),
            "usd" => Ok(Money::zero(Currency::USD)),
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
