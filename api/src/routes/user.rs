use actix_web::{
    get, post,
    web::{Json, Query},
    HttpResponse,
};

use core_types::{Currency, Money};
use tokio::sync::mpsc;
use tokio::time::timeout;

use actix_web::http::header;
use std::{sync::Arc, time::Duration};

use rust_decimal::prelude::Decimal;
use rust_decimal_macros::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use xerror::api::*;
use std::collections::HashMap;

use msgs::api::*;
use msgs::*;

use crate::comms::*;
use crate::jwt::*;
use crate::WebDbPool;
use crate::WebSender;

use models::invoices::*;
use models::transactions::Transaction;
use models::summary_transactions::SummaryTransaction;
use models::users::{ShareableUser, User};

const MINIMUM_PATTERN_LENGTH: usize = 1;

#[get("/balance")]
pub async fn balance(web_sender: WebSender, auth_data: AuthData) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let uid = auth_data.uid as u64;

    let get_balances = GetBalances { req_id, uid };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::Balances(_balances)) if get_balances.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::GetBalances(get_balances));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::Balances(balances))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&balances));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[derive(Deserialize)]
pub struct PayInvoiceData {
    pub payment_request: Option<String>,
    pub currency: Option<Currency>,
    pub recipient: Option<String>,
    pub amount: Option<Decimal>,
}

#[post("/payinvoice")]
pub async fn pay_invoice(
    auth_data: AuthData,
    web_sender: WebSender,
    pay_invoice_data: Json<PayInvoiceData>,
) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let uid = auth_data.uid as u64;

    if let Some(amount) = pay_invoice_data.amount {
        if amount <= dec!(0) {
            return Err(ApiError::Request(RequestError::InvalidDataSupplied));
        }
    }

    if let Some(recipient) = &pay_invoice_data.recipient {
        if recipient.len() > 128 {
            return Err(ApiError::Request(RequestError::InvalidDataSupplied));
        }
    }

    if let Some(payment_request) = &pay_invoice_data.payment_request {
        // TODO: We probably have to be a little bit smarter here.
        if payment_request.len() > 1024 {
            return Err(ApiError::Request(RequestError::InvalidDataSupplied));
        }
    }

    let currency = match pay_invoice_data.currency {
        Some(c) => c,
        None => Currency::BTC,
    };

    let money = if let Some(a) =  pay_invoice_data.amount {
        Some(Money::new(currency, Some(a)))
    } else {
        None
    };

    let payment_request = PaymentRequest {
        currency,
        req_id,
        uid,
        payment_request: pay_invoice_data.payment_request.clone(),
        rate: None,
        amount: money,
        receipient: pay_invoice_data.recipient.clone(),
        destination: None,
        fees: None,
    };

    if pay_invoice_data.payment_request.is_none() && pay_invoice_data.recipient.is_none() {
        return Ok(HttpResponse::Ok().json(json!({"error": "You have to specify either an invoice or a receipient"})));
    }

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::PaymentResponse(response)) if response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::PaymentRequest(payment_request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::PaymentResponse(response))))) =
        timeout(Duration::from_secs(10), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[derive(Deserialize, Debug)]
pub struct CreateInvoiceParams {
    pub amount: Decimal,
    pub meta: Option<String>,
    pub metadata: Option<String>,
    pub account_id: Option<Uuid>,
    pub currency: Option<Currency>,
    pub target_account_currency: Option<Currency>,
}

#[get("/addinvoice")]
pub async fn add_invoice(
    auth_data: AuthData,
    web_sender: WebSender,
    query: Query<CreateInvoiceParams>,
) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let uid = auth_data.uid as u64;

    if query.amount <= dec!(0) {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let meta = match &query.meta {
        Some(m) => m.clone(),
        None => "Lndhubx Invoice".to_string(),
    };

    if meta.len() > 128 {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let currency = match &query.currency {
        Some(c) => *c,
        None => Currency::BTC,
    };

    let amount = Money::new(currency, Some(query.amount));

    let invoice_request = InvoiceRequest {
        req_id,
        meta,
        amount,
        metadata: query.metadata.clone(),
        uid,
        currency,
        account_id: query.account_id,
        target_account_currency: query.target_account_currency,
    };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::InvoiceResponse(invoice)) if invoice.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::InvoiceRequest(invoice_request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::InvoiceResponse(invoice))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&invoice));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[derive(Deserialize)]
pub struct SwapData {
    pub from_currency: Currency,
    pub to_currency: Currency,
    pub amount: Decimal,
    pub quote_id: Option<u128>,
}

#[post("/swap")]
pub async fn swap(auth_data: AuthData, web_sender: WebSender, data: Json<SwapData>) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let uid = auth_data.uid as u64;

    if data.amount <= dec!(0) {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let money = Money::new(data.from_currency, Some(data.amount));

    let swap_request = SwapRequest {
        req_id,
        uid,
        from: data.from_currency,
        to: data.to_currency,
        amount: money,
        quote_id: data.quote_id,
    };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::SwapResponse(swap_response)) if swap_response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::SwapRequest(swap_request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::SwapResponse(swap_response))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&swap_response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[get("/getuserinvoices")]
pub async fn get_user_invoices(pool: WebDbPool, auth_data: AuthData) -> Result<HttpResponse, ApiError> {
    let uid = auth_data.uid as u64;

    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let invoices = match Invoice::get_invoices_by_uid(&conn, uid as i32) {
        Ok(i) => i,
        Err(_) => return Err(ApiError::Db(DbError::CouldNotFetchData)),
    };

    Ok(HttpResponse::Ok().json(&invoices))
}

#[derive(Deserialize)]
pub struct QuoteParams {
    pub from_currency: Currency,
    pub to_currency: Currency,
    pub amount: Decimal,
}

#[get("/quote")]
pub async fn quote(
    auth_data: AuthData,
    web_sender: WebSender,
    query: Query<QuoteParams>,
) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let uid = auth_data.uid as u64;

    if query.amount <= dec!(0) {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let money = Money::new(query.from_currency, Some(query.amount));

    let quote_request = QuoteRequest {
        req_id,
        uid,
        from: query.from_currency,
        to: query.to_currency,
        amount: money,
    };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::QuoteResponse(quote_response)) if quote_response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::QuoteRequest(quote_request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::QuoteResponse(quote_response))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&quote_response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[derive(Deserialize)]
pub struct TransactionsParams {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub currency: Option<Currency>,
}

#[get("/gettxs")]
pub async fn get_txs(
    pool: WebDbPool,
    auth_data: AuthData,
    query: Query<TransactionsParams>,
) -> Result<HttpResponse, ApiError> {
    let uid = auth_data.uid as u64;
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;
    if let Some(currency) = query.currency {
        let transactions = match SummaryTransaction::get_historical_by_uid_and_currency(
            &conn,
            uid as i32,
            currency.to_string(),
            query.from,
            query.to,
        ) {
            Ok(i) => i,
            Err(_) => return Err(ApiError::Db(DbError::CouldNotFetchData)),
        };
        return Ok(HttpResponse::Ok().json(&transactions));
    }
    let transactions = match SummaryTransaction::get_historical_by_uid(&conn, uid as i32, query.from, query.to) {
        Ok(i) => i,
        Err(_) => return Err(ApiError::Db(DbError::CouldNotFetchData)),
    };
    Ok(HttpResponse::Ok().json(&transactions))
}

#[get("/getavailablecurrencies")]
pub async fn get_available_currencies(web_sender: WebSender) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let request = AvailableCurrenciesRequest { req_id };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::AvailableCurrenciesResponse(response)) if response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::AvailableCurrenciesRequest(request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::AvailableCurrenciesResponse(response))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[get("/nodeinfo")]
pub async fn get_node_info(web_sender: WebSender) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let request = GetNodeInfoRequest { req_id };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::GetNodeInfoResponse(response)) if response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::GetNodeInfoRequest(request));
    dbg!(&message);

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::GetNodeInfoResponse(response))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[derive(Deserialize)]
pub struct QueryRouteParams {
    pub payment_request: String,
    pub max_fee: Option<Decimal>,
}

#[get("/query_route")]
pub async fn get_query_route(query: Query<QueryRouteParams>, web_sender: WebSender) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let params = query.into_inner();

    let request = QueryRouteRequest {
        req_id,
        payment_request: params.payment_request,
        max_fee: params.max_fee,
    };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::QueryRouteResponse(response)) if response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::QueryRouteRequest(request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::QueryRouteResponse(response))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[derive(Deserialize)]
pub struct SearchUserParams {
    text: String,
}

#[get("/search_user")]
pub async fn search_user(
    pool: WebDbPool,
    params: Query<SearchUserParams>,
) -> Result<HttpResponse, ApiError> {
    if params.text.len() < MINIMUM_PATTERN_LENGTH {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let escaped = params.text.replace('%', "\\%").replace('_', "\\_");
    let data = {
        let conn = pool.try_get().ok_or(ApiError::Db(DbError::DbConnectionError))?;
        User::search_by_username_fragment(&conn, &escaped).map_err(|_| ApiError::Db(DbError::UserDoesNotExist))
    }?;

    let response_data = data
        .into_iter()
        .map(|user| ShareableUser {
            uid: user.uid,
            username: user.username,
        })
        .collect::<Vec<ShareableUser>>();

    Ok(HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .json(json!({ "data": response_data, "error": null })))
}

#[derive(Deserialize)]
pub struct CheckUsernameData {
    pub username: String,
}

#[post("/check_username_available")]
pub async fn check_username_available(
    pool: WebDbPool,
    username_data: Json<CheckUsernameData>,
) -> Result<HttpResponse, ApiError> {
    let conn = pool.try_get().ok_or(ApiError::Db(DbError::DbConnectionError))?;
    match User::get_by_username(&conn, username_data.username.clone()) {
        Ok(_) => Ok(HttpResponse::Ok().json(json!({ "available": false}))),
        _ => Ok(HttpResponse::Ok().json(json!({ "available": true}))),
    }
}

#[derive(Deserialize)]
pub struct CheckPaymentParams {
    payment_hash: String,
}

#[derive(Serialize)]
pub struct CheckPaymentHashResponse {
    paid: bool
}

#[get("/checkpayment")]
pub async fn check_payment(
    pool: WebDbPool,
    params: Query<CheckPaymentParams>,
) -> Result<HttpResponse, ApiError> {

    let payment_hash = params.payment_hash.clone();

    let conn = pool.try_get().ok_or(ApiError::Db(DbError::DbConnectionError))?;

    let invoice = match Invoice::get_by_payment_request(&conn, payment_hash) {
        Ok(i) => i,
        Err(_) => return Err(ApiError::Db(DbError::CouldNotFetchData)),
    };

    let response = CheckPaymentHashResponse {
        paid: invoice.settled
    };

    Ok(HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .json(&response))
}

#[derive(Deserialize)]
pub struct KeySendData {
    pub amount: u64,
    pub destination: String,
    pub memo: String,
    pub custom_records: Option<HashMap<String, String>>,
}

#[post("/keysend")]
pub async fn keysend(auth_data: AuthData, web_sender: WebSender, data: Json<KeySendData>) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let uid = auth_data.uid as u64;

    if data.amount <= 0 {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let a = Decimal::new(data.amount as i64, 0);

    let currency = Currency::BTC;
    let money = Money::new(currency, Some(a));

    let payment_request = PaymentRequest {
        currency,
        req_id,
        uid,
        payment_request: None,
        rate: None,
        amount: Some(money),
        receipient: None,
        destination: Some(data.destination.clone()),
        fees: None,
    };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::PaymentResponse(payment_response)) if payment_response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::PaymentRequest(payment_request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::SwapResponse(payment_response))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&payment_response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}