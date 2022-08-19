use core_types::kollider_client::{Balances, PositionState, Side};
use core_types::{Currency, Symbol};
use msgs::kollider_client::{Channel, TradableSymbol};
use rust_decimal::Decimal;
use std::collections::HashMap;
use xerror::kollider_client::KolliderClientError;

pub type Result<T> = std::result::Result<T, KolliderClientError>;

pub trait WsClient {
    fn is_connected(&self) -> bool;
    fn is_authenticated(&self) -> bool;
    fn is_ready(&self) -> bool;
    fn get_balance(&self, currency: Currency) -> Result<Decimal>;
    fn get_all_balances(&self) -> Option<Balances>;
    fn get_position_state(&self, symbol: &Symbol) -> Result<Option<PositionState>>;
    fn get_tradable_symbols(&self) -> HashMap<Symbol, TradableSymbol>;
    fn make_withdrawal(&self, amount: u64, payment_request: String) -> Result<()>;
    fn make_order(&self, quantity: u64, symbol: Symbol, side: Side) -> Result<()>;
    fn subscribe(&self, chanels: Vec<Channel>, symbols: Option<Vec<Symbol>>);
    fn buy(&self, quantity: u64, currency: Currency) -> Result<()>;
    fn sell(&self, quantity: u64, currency: Currency) -> Result<()>;
}
