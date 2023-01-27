use actix_web::{
    get, post,
    web::{Json, Query},
    HttpResponse,
};

use actix_web::http::header;

use diesel::result::DatabaseErrorKind;
use diesel::result::Error;
use serde::Deserialize;
use serde_json::json;
use xerror::api::*;

use crate::comms::*;
use crate::jwt::*;
use crate::WebDbPool;
use crate::WebSender;
use msgs::{api::*, *};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio::time::timeout;
use uuid::Uuid;

use models::nostr_public_keys::*;
use models::users::User;

#[derive(Deserialize)]
pub struct NostrPubkeyData {
    pub pubkey: String,
}

#[post("/nostr_pubkey")]
pub async fn set_nostr_pubkey(
    pool: WebDbPool,
    auth_data: AuthData,
    data: Json<NostrPubkeyData>,
) -> Result<HttpResponse, ApiError> {
    let uid = auth_data.uid as u64;

    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    if User::get_by_id(&conn, uid as i32).is_err() {
        return Err(ApiError::Db(DbError::UserDoesNotExist));
    }

    let nostr_pubkey = InsertableNostrPublicKey {
        pubkey: data.pubkey.clone(),
        uid: uid as i32,
    };

    if let Err(err) = nostr_pubkey.insert(&conn) {
        match err {
            Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                return Ok(HttpResponse::Ok()
                    .insert_header((header::CONTENT_TYPE, "application/json"))
                    .json(json!({"status": "User has Pubkey already set.".to_string()})))
            }
            _ => {
                return Ok(HttpResponse::Ok()
                    .insert_header((header::CONTENT_TYPE, "application/json"))
                    .json(json!({"status": "error inserting".to_string()})))
            }
        }
    }

    Ok(HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .json(json!({"status": "ok".to_string()})))
}

#[post("/update_nostr_pubkey")]
pub async fn update_nostr_pubkey(
    pool: WebDbPool,
    auth_data: AuthData,
    data: Json<NostrPubkeyData>,
) -> Result<HttpResponse, ApiError> {
    let uid = auth_data.uid as u64;

    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    if NostrPublicKey::get_by_uid(&conn, uid as i32).is_ok() {
        let update_pubkey = UpdateNostrPublicKey {
            pubkey: data.pubkey.clone(),
        };
        if update_pubkey.update(&conn, uid as i32).is_err() {
            return Ok(HttpResponse::Ok()
                .insert_header((header::CONTENT_TYPE, "application/json"))
                .json(json!({"status": "error updating pubkey.".to_string()})));
        }
    } else {
        return Ok(HttpResponse::Ok()
            .insert_header((header::CONTENT_TYPE, "application/json"))
            .json(json!({"status": "Error: No public key exists for user.".to_string()})));
    }

    Ok(HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .json(json!({"status": "ok".to_string()})))
}

#[derive(Deserialize)]
pub struct Nip05Params {
    name: String,
}

#[get("/.well-known/nostr.json")]
pub async fn nostr_nip05(pool: WebDbPool, params: Query<Nip05Params>) -> Result<HttpResponse, ApiError> {
    let username = params.name.clone();
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let nostr_pubkey = match NostrPublicKey::get_by_username(&conn, username.clone()) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
    };

    let resp = json!({
        "names": json!({
          format!("{}", username): nostr_pubkey.pubkey
        })
    });

    Ok(HttpResponse::Ok().json(resp))
}

#[derive(Deserialize)]
pub struct GetNostrProfileParams {
    lightning_address: Option<String>,
    pubkey: Option<String>,
}

#[get("/get_nostr_profile")]
pub async fn get_nostr_profile(
    web_sender: WebSender,
    params: Query<GetNostrProfileParams>,
) -> Result<HttpResponse, ApiError> {
    let req_id = Uuid::new_v4();

    if params.lightning_address.is_none() && params.pubkey.is_none() {
        return Err(ApiError::Request(RequestError::InvalidDataSupplied));
    }

    let request = NostrProfileRequest {
        req_id,
        pubkey: params.pubkey.clone(),
        lightning_address: params.lightning_address.clone(),
    };

    let response_filter: Box<dyn Send + Fn(&Message) -> bool> = Box::new(
        move |message| matches!(message, Message::Api(Api::NostrProfileResponse(response)) if response.req_id == req_id),
    );

    let (response_tx, mut response_rx) = mpsc::channel(1);

    let message = Message::Api(Api::NostrProfileRequest(request));

    Arc::make_mut(&mut web_sender.into_inner())
        .send(Envelope {
            message,
            response_tx: Some(response_tx),
            response_filter: Some(response_filter),
        })
        .await
        .map_err(|_| ApiError::Comms(CommsError::FailedToSendMessage))?;

    if let Ok(Some(Ok(Message::Api(Api::NostrProfileResponse(response))))) =
        timeout(Duration::from_secs(5), response_rx.recv()).await
    {
        return Ok(HttpResponse::Ok().json(&response));
    }
    Err(ApiError::Comms(CommsError::ServerResponseTimeout))
}
