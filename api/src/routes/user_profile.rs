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

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;
use xerror::api::*;

use msgs::api::*;
use msgs::*;

use crate::comms::*;
use crate::WebDbPool;
use crate::WebSender;

use models::users::{ShareableUser, User};
use models::user_profiles::*;
use crate::jwt::*;

#[get("/get_user_profile")]
pub async fn get_user_profile(pool: WebDbPool, auth_data: AuthData) -> Result<HttpResponse, ApiError> {

    let uid = auth_data.uid as u64;

    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    if let Ok(user_profile) = UserProfile::get_by_uid(&conn, uid as i32) {
        return Ok(HttpResponse::Ok().json(&user_profile));
    } else {
        return Err(ApiError::Db(DbError::UserDoesNotExist))
    }

}