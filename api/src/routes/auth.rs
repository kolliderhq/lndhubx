use actix_web::web::Data;
use actix_web::{get, post, web::Json, HttpResponse};
use diesel::result::DatabaseErrorKind;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use utils::xlogging::slog as log;
use uuid::Uuid;
use xerror::api::*;

use models::ln_addresses::*;
use models::user_profiles::*;
use models::users::*;
use utils::xlogging::slog::Logger;

use crate::jwt::*;
use crate::{CreationLimiter, WebDbPool};

#[derive(Deserialize)]
pub struct RegisterData {
    /// Username field on supplied json.
    pub username: Option<String>,
    /// Password field on supplied json.
    pub password: String,
}

#[post("/create")]
pub async fn create(
    pool: WebDbPool,
    logger: Data<Logger>,
    creation_limiter: Data<Arc<Mutex<CreationLimiter>>>,
    register_data: Json<RegisterData>,
) -> Result<HttpResponse, ApiError> {
    let username = match &register_data.username {
        Some(un) => un.clone().to_lowercase(),
        None => Uuid::new_v4().to_string().to_lowercase(),
    };

    {
        let limiter = creation_limiter.into_inner();
        let mut creation_limiter_guard = limiter.lock().await;
        if !creation_limiter_guard.is_creation_enabled() {
            log::warn!(
                logger,
                "Unsuccessful wallet creation attempt for user: {} due to: new registrations disabled",
                username
            );
            return Err(ApiError::Auth(AuthError::RegistrationsDisabled));
        }

        if let Err(replenish_seconds_left) = creation_limiter_guard.increase() {
            log::warn!(logger, "Unsuccessful wallet creation attempt for user: {} due to: exceeded registration limit. Quota replenishes in {} seconds", username, replenish_seconds_left);
            return Err(ApiError::Auth(AuthError::RegistrationLimitExceeded));
        }
    }

    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let hashed_password = hash(&username, &register_data.password);

    let user = InsertableUser {
        username: username.clone(),
        password: hashed_password,
        is_internal: false,
    };

    let uid = match user.insert(&conn) {
        Ok(uid) => uid,
        Err(err) => match err {
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                return Err(ApiError::Db(DbError::UserAlreadyExists))
            }
            _ => return Err(ApiError::Db(DbError::Unknown)),
        },
    };

    // TODO: Make this configurable.
    let ln_address = InsertableLnAddress {
        username: format!("{username}@kollider.me"),
        domain: String::from("kollider.me"),
    };

    if ln_address.insert(&conn).is_err() {
        dbg!("Error inserting Ln Address");
    }

    let insertable_user_profile = InsertableUserProfile {
        uid,
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

    log::warn!(logger, "Successful wallet creation for user: {}", username);
    Ok(HttpResponse::Ok().json(json!({"username": user.username})))
}

#[derive(Deserialize)]
pub struct LoginData {
    /// Username field on supplied json.
    pub username: String,
    /// Password field on supplied json.
    pub password: String,
}

#[post("/auth")]
pub async fn auth(pool: WebDbPool, login_data: Json<LoginData>) -> Result<HttpResponse, ApiError> {
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let user = match User::get_by_username(&conn, login_data.username.clone()) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
    };

    if !verify(&user.username.to_lowercase(), &user.password, &login_data.password) {
        return Err(ApiError::Auth(AuthError::IncorrectPassword));
    }

    let refresh_expiry = 1000000;

    let access_expiry = 10000000;

    let token = jwt_generate(user.uid, None, UserRoles::MasterToken, access_expiry).map_err(ApiError::JWT)?;
    let refresh =
        jwt_generate_refresh_token(user.uid, UserRoles::MasterToken, refresh_expiry).map_err(ApiError::JWT)?;

    // InsertableApiTokenFull::new(Uuid::new_v4().to_string(), Some(refresh.clone()), user.uid as i32).insert(&conn)?;

    Ok(HttpResponse::Ok().json(json!({"token": token, "refresh": refresh})))
}

#[get("/whoami")]
pub async fn whoami(pool: WebDbPool, auth_data: AuthData) -> Result<HttpResponse, ApiError> {
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let uid = auth_data.uid as u64;

    let user = match User::get_by_id(&conn, uid as i32) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
    };

    Ok(HttpResponse::Ok().json(json!({"username": user.username, "uid": user.uid})))
}
