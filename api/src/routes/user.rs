use actix_web::{
    get, post,
    web::{Json, Query},
    HttpResponse,
};

use core_types::Currency;
use tokio::sync::mpsc;
use tokio::time::timeout;

use std::{sync::Arc, time::Duration};

use rust_decimal::prelude::Decimal;
use rust_decimal_macros::*;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use xerror::api::*;

use msgs::api::*;
use msgs::*;

use crate::comms::*;
use crate::jwt::*;
use crate::WebDbPool;
use crate::WebSender;

use models::invoices::*;
use models::transactions::Transaction;

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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
}

#[derive(Deserialize)]
pub struct PayInvoiceData {
    pub payment_request: Option<String>,
    pub currency: Option<Currency>,
    pub receipient: Option<String>,
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
            return Err(ApiError::Request(RequestError::InvalidDataSupplied))
        }
    }

    if let Some(receipient) = &pay_invoice_data.receipient {
        if receipient.len() > 128 {
            return Err(ApiError::Request(RequestError::InvalidDataSupplied))
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

    let payment_request = PaymentRequest {
        currency,
        req_id,
        uid,
        payment_request: pay_invoice_data.payment_request.clone(),
        rate: None,
        amount: pay_invoice_data.amount,
        receipient: pay_invoice_data.receipient.clone(),
        fees: None,
    };

    if pay_invoice_data.payment_request.is_none() && pay_invoice_data.receipient.is_none() {
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
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&response));
    }
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
}

#[derive(Deserialize, Debug)]
pub struct CreateInvoiceParams {
    pub amount: Decimal,
    pub meta: Option<String>,
    pub account_id: Option<Uuid>,
    pub currency: Option<Currency>,
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

    let invoice_request = InvoiceRequest {
        req_id,
        meta,
        uid,
        account_id: query.account_id,
        amount: query.amount,
        currency,
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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
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

    let swap_request = SwapRequest {
        req_id,
        uid,
        from: data.from_currency,
        to: data.to_currency,
        amount: data.amount,
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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
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

    let quote_request = QuoteRequest {
        req_id,
        uid,
        from: query.from_currency,
        to: query.to_currency,
        amount: query.amount,
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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
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
        let transactions = match Transaction::get_historical_by_uid_and_currency(
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
    let transactions = match Transaction::get_historical_by_uid(&conn, uid as i32, query.from, query.to) {
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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
}
