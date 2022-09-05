use core_types::kollider_client::Positions;
use core_types::{
    kollider_client::{
        Balances, ExtOrderId, MarginType, MarkPrice, OrderId, OrderType, PositionState, SettlementType, Side,
        TradeOrigin, UserId,
    },
    Symbol,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    PositionStates,
    MarkPrices,
    OrderbookLevel2,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Authenticate(AuthenticateRequest),
    FetchBalances,
    FetchTradableSymbols,
    FetchPositions,
    Subscribe(Subscribe),
    Order(Order),
    WithdrawalRequest(WithdrawalRequest),
    ChangeMargin(ChangeMargin),
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AuthenticateRequest {
    pub token: String,
    pub passphrase: String,
    pub signature: String,
    pub timestamp: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Subscribe {
    pub channels: Vec<Channel>,
    pub symbols: Vec<Symbol>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Order {
    ext_order_id: ExtOrderId,
    order_type: OrderType,
    side: Side,
    quantity: u64,
    symbol: Symbol,
    leverage: u64,
    margin_type: MarginType,
    settlement_type: SettlementType,
    origin: TradeOrigin,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct LnWithdrawalRequest {
    pub payment_request: String,
    pub amount: u64,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum WithdrawalRequestType {
    Ln(LnWithdrawalRequest),
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct WithdrawalRequest {
    pub withdrawal_request: WithdrawalRequestType,
}

impl Order {
    pub fn new(side: Side, quantity: u64, symbol: Symbol, ext_order_id: Option<Uuid>) -> Self {
        Self {
            ext_order_id: ext_order_id.unwrap_or_else(Uuid::new_v4),
            order_type: OrderType::Market,
            side,
            quantity,
            symbol,
            leverage: 100,
            margin_type: MarginType::Isolated,
            settlement_type: SettlementType::Instant,
            origin: TradeOrigin::Sdk,
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum KolliderApiResponse {
    Error(String),
    Success(String),
    Authenticate(Authenticate),
    PositionStates(Box<PositionState>),
    Positions(Positions),
    Balances(Balances),
    MarkPrices(MarkPrice),
    Received(Received),
    Fill(Fill),
    Trade(Trade),
    SettlementRequest(SettlementRequest),
    OrderInvoice(OrderInvoice),
    TradableSymbols(TradableSymbols),
    #[serde(rename = "level2state")]
    Level2State(Level2State),
    Disconnected(Disconnected),
    Reconnected(Reconnected),
    ChangeMarginSuccess(ChangeMarginSuccess),
    ChangeMarginRejection(ChangeMarginRejection),
    AddMarginRequest(AddMarginRequest),
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Disconnected {
    pub timestamp: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Reconnected {
    pub timestamp: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Authenticate {
    message: String,
}

impl Authenticate {
    pub fn success(&self) -> bool {
        self.message == "success"
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Trade {
    pub timestamp: u64,
    pub order_id: OrderId,
    pub quantity: Decimal,
    pub price: Decimal,
    pub side: Side,
    pub symbol: Symbol,
    pub leverage: Decimal,
    pub fees: Decimal,
    pub rpnl: Decimal,
    pub is_maker: bool,
    pub margin_type: MarginType,
    pub settlement_type: SettlementType,
    pub is_liquidation: bool,
    pub order_type: OrderType,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SettlementRequest {
    pub request_id: Uuid,
    pub amount: String,
    pub lnurl: Option<String>,
    pub side: Side,
    pub symbol: Symbol,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Received {
    pub timestamp: u64,
    pub order_id: OrderId,
    pub ext_order_id: ExtOrderId,
    pub uid: UserId,
    pub symbol: Symbol,
    pub quantity: u64,
    pub order_type: OrderType,
    pub price: u64,
    pub leverage: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Fill {
    pub order_id: OrderId,
    pub ext_order_id: ExtOrderId,
    pub price: u64,
    pub quantity: u64,
    pub partial: bool,
    pub symbol: Symbol,
    pub leverage: u64,
    pub side: Side,
    pub is_maker: bool,
    pub margin_type: MarginType,
    pub is_selftrade: bool,
    pub timestamp: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OrderInvoice {
    pub invoice: String,
    pub margin: String,
    pub symbol: Symbol,
    pub order_id: OrderId,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TradableSymbol {
    pub symbol: Symbol,
    pub contract_size: Decimal,
    pub max_leverage: Decimal,
    pub base_margin: Decimal,
    pub maintenance_margin: Decimal,
    pub is_inverse_priced: bool,
    pub price_dp: u32,
    pub underlying_symbol: Symbol,
    pub last_price: Decimal,
    pub tick_size: Decimal,
    pub risk_limit: Decimal,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TradableSymbols {
    pub symbols: HashMap<Symbol, TradableSymbol>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Level2State {
    pub update_type: String,
    pub seq_number: u64,
    pub symbol: Symbol,
    pub bids: BTreeMap<Decimal, u64>,
    pub asks: BTreeMap<Decimal, u64>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum ChangeMarginAction {
    Add,
    Delete,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ChangeMargin {
    pub uid: UserId,
    pub ext_id: Uuid,
    pub symbol: Symbol,
    pub amount: Decimal,
    pub action: ChangeMarginAction,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ChangeMarginSuccess {
    pub symbol: Symbol,
    pub amount: Decimal,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ChangeMarginRejection {
    pub symbol: Symbol,
    pub reason: RejectionReason,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum RejectionReason {
    ContractNotAvailable,
    PositionIsUnderLiquidation,
    PositionIsNotUnderLiquidation,
    DuplicatedOrderId,
    RiskLimitExceeded,
    MalformedOrder,
    NotEnoughMargin,
    NotEnoughAvailableBalance,
    MaxLeverageExceeded,
    BelowMinLeverage,
    NoMarket,
    InvalidOrder,
    OrderTooSmall,
    OrderDoesNotExist,
    InstantLiquidation,
    NoOpenPosition,
    PositionNotFound,
    InvalidMarginChange,
    SymbolInPostOnlyMode,
    SymbolInLimitOnlyMode,
    TradingForSymbolSuspended,
    SlippageExceeded,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AddMarginRequest {
    pub symbol: Symbol,
    pub amount: Decimal,
    pub invoice: String,
}
