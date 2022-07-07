use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::Symbol;

pub type OrderId = u64;
pub type UserId = u64;
pub type ExtOrderId = Uuid;

#[derive(Copy, Clone, Deserialize, Serialize, Debug)]
pub enum Side {
    Bid,
    Ask,
}

impl Side {
    pub fn to_sign(&self) -> i64 {
        match self {
            Side::Bid => 1,
            Side::Ask => -1,
        }
    }

    pub fn from_sign(sign: i64) -> Self {
        if sign < 0 {
            Self::Ask
        } else {
            Self::Bid
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum OrderType {
    Market,
    Limit,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MarginType {
    Isolated,
    Cross,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SettlementType {
    Instant,
    Delayed,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TradeOrigin {
    Sdk,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PositionState {
    pub timestamp: u64,
    pub symbol: Symbol,
    pub upnl: Decimal,
    pub rpnl: Decimal,
    pub funding: Decimal,
    pub leverage: Decimal,
    pub real_leverage: Decimal,
    pub entry_price: Decimal,
    pub side: Option<Side>,
    pub quantity: Decimal,
    pub open_order_ids: HashSet<OrderId>,
    pub liq_price: Decimal,
    pub bankruptcy_price: Decimal,
    pub is_liquidating: bool,
    pub entry_value: Decimal,
    pub mark_value: Decimal,
    pub adl_score: Decimal,
    pub entry_time: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Balances {
    pub cash: HashMap<Symbol, Decimal>,
    pub isolated_margin: HashMap<Symbol, Decimal>,
    pub order_margin: HashMap<Symbol, Decimal>,
    pub cross_margin: Decimal,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MarkPrice {
    pub price: Decimal,
    pub symbol: Symbol,
}
