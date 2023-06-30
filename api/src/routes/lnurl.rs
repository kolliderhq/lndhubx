use actix_web::{get, web::Path, web::Query, HttpResponse};

use core_types::{Currency, Money};
use tokio::sync::mpsc;
use tokio::time::timeout;

use actix_web::web::Data;
use std::{sync::Arc, time::Duration};

use rust_decimal::prelude::Decimal;
use rust_decimal_macros::*;
use serde::Deserialize;
use serde_json::json;
use utils::xlogging::slog as log;
use uuid::Uuid;
use xerror::api::*;

use models::nostr_public_keys::*;
use models::users::*;

use msgs::api::*;
use msgs::*;
use utils::xlogging::slog::Logger;

use crate::comms::*;
use crate::jwt::*;
use crate::WebSender;
use crate::{ApiSettings, WebDbPool};

#[derive(Deserialize, Debug)]
pub struct CreateLnurlWithdrawalParams {
    pub amount: Decimal,
    pub currency: Currency,
}

#[get("/lnurl_withdrawal/create")]
pub async fn create_lnurl_withdrawal(
    auth_data: AuthData,
    query: Query<CreateLnurlWithdrawalParams>,
    web_sender: WebSender,
) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let uid = auth_data.uid as u64;

    if query.amount <= dec!(0) {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let money = Money::new(query.currency, query.amount);

    let request = CreateLnurlWithdrawalRequest {
        uid,
        req_id,
        amount: money,
        currency: query.currency,
        rate: None,
        fees: None,
    };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::CreateLnurlWithdrawalResponse(response)) if response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::CreateLnurlWithdrawalRequest(request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::CreateLnurlWithdrawalResponse(response))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[derive(Deserialize, Debug)]
pub struct GetLnurlWithdrawalParams {
    /// How we match withdrawal request to user
    q: Uuid,
}

#[get("/lnurl_withdrawal/request")]
pub async fn get_lnurl_withdrawal(
    query: Query<GetLnurlWithdrawalParams>,
    web_sender: WebSender,
) -> Result<HttpResponse, ApiError> {
    let req_id = query.q;

    let request = GetLnurlWithdrawalRequest { req_id };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::GetLnurlWithdrawalResponse(response)) if response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::GetLnurlWithdrawalRequest(request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::GetLnurlWithdrawalResponse(response))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        let response = json!({
            "callback": response.callback,
            "k1": response.req_id,
            "maxWithdrawable": response.max_withdrawable * 1000,
            "defaultDescription": "Kollider withdrawal".to_string(),
            "minWithdrawable": 1,
            "tag": "withdrawRequest".to_string(),
        });
        return Ok(HttpResponse::Ok().json(&response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[derive(Deserialize)]
pub struct PayLnurlWithdrawalParams {
    k1: Uuid,
    pr: String,
}

#[get("/lnurl_withdrawal/pay")]
pub async fn pay_lnurl_withdrawal(
    query: Query<PayLnurlWithdrawalParams>,
    web_sender: WebSender,
) -> Result<HttpResponse, ApiError> {
    let req_id = query.k1;

    let request = PayLnurlWithdrawalRequest {
        req_id,
        payment_request: query.pr.clone(),
    };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(move |message| {
        matches!(message, Message::Api(Api::PayLnurlWithdrawalResponse(response)) if response.req_id == req_id)
            || matches!(message, Message::Api(Api::PaymentResponse(response)) if response.req_id == req_id)
    });

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::PayLnurlWithdrawalRequest(request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    match timeout(Duration::from_secs(5), response_rx.recv()).await {
        Ok(Some(Ok(Message::Api(Api::PayLnurlWithdrawalResponse(response))))) => {
            return Ok(HttpResponse::Ok().json(&response))
        }
        Ok(Some(Ok(Message::Api(Api::PaymentResponse(_))))) => {
            return Ok(HttpResponse::Ok().json(json!({"status": "OK"})))
        }
        _ => {}
    };
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[get("/.well-known/lnurlp/{username}")]
pub async fn lnurl_pay_address(
    path: Path<String>,
    pool: WebDbPool,
    settings: Data<ApiSettings>,
) -> Result<HttpResponse, ApiError> {
    let username = path.into_inner();
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let _user = match User::get_by_username(&conn, username.clone()) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
    };

    let nostr_pubkey = match NostrPublicKey::get_by_username(&conn, String::from("kollider")) {
        Ok(pubk) => Some(pubk.pubkey),
        Err(_) => None,
    };

    let callback = format!("https://{}/api/pay/{username:}", settings.domain);
    let max_sendable = 1000000000;
    let min_sendable = 1000;
    let desc = format!("Paid to {username:}@{}", settings.domain);
    let metadata = json!([["text/plain", desc]]);

    let resp = json!({
        "callback": callback,
        "maxSendable": max_sendable,
        "minSendable": min_sendable,
        "metadata": metadata.to_string(),
        "allowsNostr": true,
        "nostrPubkey": nostr_pubkey,
        "tag": "payRequest",
    });

    Ok(HttpResponse::Ok().json(resp))
}

#[derive(Deserialize)]
pub struct PayAddressParams {
    amount: u64,
    #[serde(default)]
    nostr: Option<String>,
}

#[get("/pay/{username}")]
pub async fn pay_address(
    path: Path<String>,
    pool: WebDbPool,
    query: Query<PayAddressParams>,
    web_sender: WebSender,
    logger: Data<Logger>,
    settings: Data<ApiSettings>,
) -> Result<HttpResponse, ApiError> {
    let username = path.into_inner();
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let amount = Decimal::new(query.amount as i64, 0) / dec!(100_000_000_000);

    let money = Money::new(Currency::BTC, amount);

    let user = match User::get_by_username(&conn, username.clone()) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
    };

    let req_id = Uuid::new_v4();

    let uid = user.uid as u64;

    if amount <= dec!(0) {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let (memo, metadata) = match query.nostr {
        Some(ref zap_request_json) => {
            if let Err(err) = utils::nostr::validate_zap_request(zap_request_json, query.amount) {
                log::error!(logger, "Rejected a zap request: {}, error: {:?}", zap_request_json, err);
                return Err(ApiError::Request(RequestError::InvalidDataSupplied));
            } else {
                let memo = utils::nostr::ZAP_REQUEST_MEMO.to_string();
                (memo, Some(zap_request_json.clone()))
            }
        }
        None => {
            let memo = "Lnurl Pay".to_string();
            let desc = format!("Paid to {username:}@{}", settings.domain);
            (memo, Some(format!("[[\"text/plain\",\"{desc}\"]]")))
        }
    };

    if memo.len() > 128 {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let invoice_request = InvoiceRequest {
        req_id,
        meta: memo,
        metadata,
        uid,
        currency: Currency::BTC,
        account_id: None,
        amount: money,
        target_account_currency: None,
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
        let resp = json!({
            "pr": invoice.payment_request,
            "payment_hash": invoice.payment_hash,
            "routes": [],
        });
        return Ok(HttpResponse::Ok().json(resp));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}
