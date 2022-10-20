use actix_web::{get, web::Query, HttpResponse};

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

use models::users::User;
use msgs::api::*;
use msgs::*;

use crate::comms::*;
use crate::jwt::*;
use crate::WebSender;
use crate::WebDbPool;

#[derive(Deserialize, Debug)]
pub struct CreateLnurlWithdrawalParams {
    pub amount: Decimal,
    pub currency: Currency,
}

#[get("/lnurl_withdrawal/create")]
pub async fn create_lnurl_withdrawal(
    pool: WebDbPool,
    auth_data: AuthData,
    query: Query<CreateLnurlWithdrawalParams>,
    web_sender: WebSender,
) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let uid = auth_data.uid as u64;
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;
    let user = match User::get_by_id(&conn, uid as i32) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
    };
    if user.is_suspended {
        return Err(ApiError::Request(RequestError::SuspendedUser));
    }

    if query.amount <= dec!(0) {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let request = CreateLnurlWithdrawalRequest {
        uid,
        req_id,
        amount: query.amount,
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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
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
    Ok(HttpResponse::InternalServerError().json(json!({"status": "timeout"})))
}
