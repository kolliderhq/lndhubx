use core_types::*;
use std::collections::HashMap;
use rust_decimal::prelude::Decimal;


use serde::{Deserialize, Serialize};
use uuid::Uuid;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankStateRequest {
    pub req_id: RequestId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankStateResponse {
    pub req_id: RequestId,
    pub uid: UserId,
    pub amount: Option<u64>,
    pub meta: String,
    pub currency: Currency,
    pub account_id: Option<Uuid>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankState {
    pub total_exposures: HashMap<Currency, Decimal>,
    pub external_account: Account,
    pub insurance_fund_account: Account,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayInvoice {
    pub req_id: RequestId,
    pub payment_request: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInvoiceRequest {
    pub req_id: RequestId,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInvoiceResponse {
    pub req_id: RequestId,
    pub amount: u64,
    pub payment_request: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum HealthStatus {
    Running,
    Down,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DealerHealth {
    pub status: HealthStatus,
    pub available_currencies: Vec<Currency>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiatDepositRequest {
    pub req_id: RequestId,
    pub amount: Decimal,
    pub currency: Currency,
    pub uid: UserId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FiatDepositResponseError {
    CurrencyNotAvailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiatDepositResponse {
    pub req_id: RequestId,
    pub amount: Decimal,
    pub rate: Option<Decimal>,
    pub currency: Currency,
    pub uid: UserId,
    pub error: Option<FiatDepositResponseError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Dealer {
    BankStateRequest(BankStateRequest),
    BankStateResponse(BankStateResponse),
    BankState(BankState),
    Health(DealerHealth),
    PayInvoice(PayInvoice),
    CreateInvoiceRequest(CreateInvoiceRequest),
    CreateInvoiceResponse(CreateInvoiceResponse),
    FiatDepositRequest(FiatDepositRequest),
    FiatDepositResponse(FiatDepositResponse),
}
