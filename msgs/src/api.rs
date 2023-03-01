use core_types::{nostr::NostrProfile, *};
use rust_decimal::prelude::*;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InvoiceResponseError {
    AccountDoesNotExist,
    RateNotAvailable,
    WithdrawalOnly,
    DepositLimitExceeded,
    RequestLimitExceeded,
    DatabaseConnectionFailed,
    InvoicingSuspended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CreateLnurlWithdrawalError {
    InsufficientFunds,
    FailedToCreateLnUrl,
    InvalidAmount,
    UserAccountNotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GetLnurlWithdrawalError {
    RequestNotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PayLnurlWithdrawalError {
    RequestNotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SwapResponseError {
    Invalid,
    CurrencyNotAvailable,
    InvalidQuoteId,
    NotEnoughAvailableBalance,
    BTCNotFromTo,
    UserAccountNotFound,
    DatabaseConnectionFailed,
    TransactionFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuoteResponseError {
    Invalid,
    ServiceNotAvailable,
    CurrencyNotAvailable,
    MarketNotAvailable,
    BTCNotFromTo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NostrResponseError {
    ProfileNotFound,
    ErrorSendingPrivateMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryRouteError {
    NoRouteFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AvailableCurrenciesResponseError {
    GeneralError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceRequest {
    pub req_id: RequestId,
    pub uid: UserId,
    pub amount: Money,
    pub meta: String,
    pub metadata: Option<String>,
    pub currency: Currency,
    pub account_id: Option<Uuid>,
    pub target_account_currency: Option<Currency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceResponse {
    pub req_id: RequestId,
    pub uid: UserId,
    pub payment_hash: Option<String>,
    pub payment_request: Option<String>,
    pub meta: String,
    pub metadata: Option<String>,
    pub amount: Money,
    pub rate: Option<Rate>,
    pub currency: Currency,
    pub target_account_currency: Option<Currency>,
    pub account_id: Option<Uuid>,
    pub error: Option<InvoiceResponseError>,
    pub fees: Option<Money>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequest {
    pub req_id: RequestId,
    pub uid: UserId,
    pub payment_request: Option<String>,
    pub currency: Currency,
    pub recipient: Option<String>,
    pub destination: Option<String>,
    pub amount: Option<Money>,
    pub invoice_amount: Option<Money>,
    pub rate: Option<Rate>,
    pub fees: Option<Money>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PaymentResponseError {
    InsufficientFunds,
    InsufficientFundsForFees,
    ZeroAmountInvoice,
    InvoiceAlreadyPaid,
    SelfPayment,
    RateNotAvailable,
    UserDoesNotExist,
    RequestLimitExceeded,
    InvalidAmount,
    UserAccountNotFound,
    TransactionFailed,
    DatabaseConnectionFailed,
    InvalidInvoice,
    CreatingInvoiceFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResponse {
    pub req_id: RequestId,
    pub payment_hash: String,
    pub uid: UserId,
    pub success: bool,
    pub currency: Currency,
    pub payment_request: Option<String>,
    pub amount: Option<Money>,
    pub fees: Option<Money>,
    pub rate: Option<Rate>,
    pub error: Option<PaymentResponseError>,
    pub payment_preimage: Option<String>,
    pub destination: Option<String>,
    pub description: Option<String>,
}

impl PaymentResponse {
    pub fn error(
        error: PaymentResponseError,
        req_id: RequestId,
        uid: UserId,
        payment_request: Option<String>,
        currency: Currency,
        payment_preimage: Option<String>,
    ) -> Self {
        Self {
            error: Some(error),
            amount: None,
            payment_hash: Uuid::new_v4().to_string(),
            req_id,
            uid,
            success: false,
            payment_request,
            currency,
            fees: None,
            rate: None,
            payment_preimage,
            description: None,
            destination: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRequest {
    pub req_id: RequestId,
    pub uid: UserId,
    pub amount: Money,
    pub from: Currency,
    pub to: Currency,
    pub quote_id: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapResponse {
    pub req_id: RequestId,
    pub uid: UserId,
    pub success: bool,
    pub amount: Money,
    pub from: Currency,
    pub to: Currency,
    pub rate: Option<Rate>,
    pub error: Option<SwapResponseError>,
    pub fees: Option<Money>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetBalances {
    pub req_id: RequestId,
    pub uid: UserId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balances {
    pub req_id: RequestId,
    pub uid: UserId,
    pub accounts: HashMap<AccountId, Account>,
    pub error: Option<BalancesResponseError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BalancesResponseError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteRequest {
    pub req_id: RequestId,
    pub uid: UserId,
    pub amount: Money,
    pub from: Currency,
    pub to: Currency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteResponse {
    pub req_id: RequestId,
    pub uid: UserId,
    pub amount: Money,
    pub from: Currency,
    pub to: Currency,
    // epoch in ms
    pub valid_until: u64,
    pub rate: Option<Rate>,
    pub quote_id: Option<u128>,
    pub error: Option<QuoteResponseError>,
    pub fees: Option<Money>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableCurrenciesRequest {
    pub req_id: RequestId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableCurrenciesResponse {
    pub req_id: RequestId,
    pub currencies: Vec<Currency>,
    pub error: Option<AvailableCurrenciesResponseError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetNodeInfoRequest {
    pub req_id: RequestId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetNodeInfoResponse {
    pub req_id: RequestId,
    pub lnd_node_info: LndNodeInfo,
    pub ln_network_max_fee: Decimal,
    pub ln_network_fee_margin: Decimal,
    pub reserve_ratio: Decimal,
    pub external_tx_fee: Decimal,
    pub internal_tx_fee: Decimal,
    pub error: Option<GetNodeInfoResponseError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GetNodeInfoResponseError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLnurlWithdrawalRequest {
    pub req_id: RequestId,
    pub uid: UserId,
    pub amount: Money,
    pub currency: Currency,
    pub rate: Option<Rate>,
    pub fees: Option<Money>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLnurlWithdrawalResponse {
    pub req_id: RequestId,
    pub lnurl: Option<String>,
    pub error: Option<CreateLnurlWithdrawalError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetLnurlWithdrawalRequest {
    pub req_id: RequestId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetLnurlWithdrawalResponse {
    pub req_id: RequestId,
    pub max_withdrawable: u64,
    pub min_withdrawable: u64,
    pub default_description: String,
    pub tag: String,
    pub callback: String,
    pub error: Option<GetLnurlWithdrawalError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayLnurlWithdrawalRequest {
    pub req_id: RequestId,
    pub payment_request: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayLnurlWithdrawalResponse {
    pub req_id: RequestId,
    pub error: Option<PayLnurlWithdrawalError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRouteRequest {
    pub req_id: RequestId,
    pub payment_request: String,
    pub max_fee: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRouteResponse {
    pub req_id: RequestId,
    pub total_fee: Decimal,
    pub error: Option<QueryRouteError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrProfileRequest {
    pub req_id: RequestId,
    pub pubkey: Option<String>,
    pub lightning_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrProfileResponse {
    pub req_id: RequestId,
    pub profile: Option<NostrProfile>,
    pub error: Option<NostrResponseError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareableNostrProfile {
    pub pubkey: String,
    pub created_at: i64,
    pub profile: NostrProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrProfileSearchRequest {
    pub req_id: RequestId,
    pub text: String,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrProfileSearchResponse {
    pub req_id: RequestId,
    pub data: Vec<ShareableNostrProfile>,
    pub error: Option<NostrResponseError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Api {
    InvoiceRequest(InvoiceRequest),
    InvoiceResponse(InvoiceResponse),
    PaymentRequest(PaymentRequest),
    PaymentResponse(PaymentResponse),
    SwapRequest(SwapRequest),
    SwapResponse(SwapResponse),
    GetBalances(GetBalances),
    Balances(Balances),
    QuoteRequest(QuoteRequest),
    QuoteResponse(QuoteResponse),
    AvailableCurrenciesRequest(AvailableCurrenciesRequest),
    AvailableCurrenciesResponse(AvailableCurrenciesResponse),
    GetNodeInfoRequest(GetNodeInfoRequest),
    GetNodeInfoResponse(GetNodeInfoResponse),
    CreateLnurlWithdrawalRequest(CreateLnurlWithdrawalRequest),
    CreateLnurlWithdrawalResponse(CreateLnurlWithdrawalResponse),
    GetLnurlWithdrawalRequest(GetLnurlWithdrawalRequest),
    GetLnurlWithdrawalResponse(GetLnurlWithdrawalResponse),
    PayLnurlWithdrawalRequest(PayLnurlWithdrawalRequest),
    PayLnurlWithdrawalResponse(PayLnurlWithdrawalResponse),
    QueryRouteRequest(QueryRouteRequest),
    QueryRouteResponse(QueryRouteResponse),
    NostrProfileRequest(NostrProfileRequest),
    NostrProfileResponse(NostrProfileResponse),
    NostrProfileSearchRequest(NostrProfileSearchRequest),
    NostrProfileSearchResponse(NostrProfileSearchResponse),
}
