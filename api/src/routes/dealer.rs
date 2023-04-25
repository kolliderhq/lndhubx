use actix_web::{
    get, post,
    web::{Json, Query, Data},
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
use std::collections::HashMap;
use uuid::Uuid;
use xerror::api::*;

use msgs::api::*;
use msgs::*;

use crate::comms::*;
use crate::jwt::*;
use crate::WebDbPool;
use crate::WebSender;

use models::deezy_stuff::*;
use models::invoices::*;
use models::ln_addresses::*;
use models::summary_transactions::SummaryTransaction;
use models::users::User;


#[post("/set_hedge_state")]
pub async fn set_hedge_state(
    auth_data: AuthData,
    web_sender: WebSender,
	pool: WebDbPool,
    // pay_invoice_data: Json<PayInvoiceData>,
) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let uid = auth_data.uid as u64;

    let user = match User::get_by_id(&conn, uid as i32) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
    };

    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}

#[get("/get_hedge_state")]
pub async fn get_hedge_state(
    // auth_data: AuthData,
    web_sender: WebSender,
	pool: WebDbPool,
	dealer_only_mode: Data<bool>,
) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    // let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    // let uid = auth_data.uid as u64;
	return Ok(HttpResponse::Ok().json(&json!({"dealer_only_mode": dealer_only_mode})));
}