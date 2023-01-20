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

    if let Ok(up) = UserProfile::get_by_uid(&conn, uid as i32) {
        return Ok(HttpResponse::Ok().json(&up));
    } else {
        let insertable_user_profile = InsertableUserProfile {
            uid: uid as i32,
            email: None,
            img_url: None,
            is_email_verified: None,
            is_twitter_verified: None,
            twitter_handle: None,
            nostr_notifications: None,
            email_notifications: None, 
        };

        if insertable_user_profile.insert(&conn).is_err() {
            dbg!("Error inserting user profile");
        }
        return Ok(HttpResponse::Ok().json(&insertable_user_profile));
    }

}

#[derive(Deserialize)]
pub struct UpdateProfileData {
    pub email: Option<String>,
    pub nostr_notifications: Option<bool>,
    pub email_notifications: Option<bool>,
	pub img_url: Option<String>,
	pub twitter_handle: Option<String>,
}

#[post("/user_profile")]
pub async fn user_profile(pool: WebDbPool, auth_data: AuthData, data: Json<UpdateProfileData>) -> Result<HttpResponse, ApiError> {

    let uid = auth_data.uid as u64;

    let update_user_profile = UpdateUserProfile {
        email: data.email.clone(),
        nostr_notifications: data.nostr_notifications,
        email_notifications: data.email_notifications,
        img_url: data.img_url.clone(),
        twitter_handle: data.twitter_handle.clone(),
        is_twitter_verified: None,
        is_email_verified: None,
    };

    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    if let Ok(_) = update_user_profile.update(&conn, uid as i32) {
        return Ok(HttpResponse::Ok().json(json!({"status": "ok"})));
    } else {
        return Err(ApiError::Db(DbError::UserDoesNotExist))
    }

}