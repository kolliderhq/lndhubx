use actix_web::{post, web::Json, HttpResponse};
use diesel::result::DatabaseErrorKind;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use xerror::api::*;

use models::users::*;

use crate::jwt::*;
use crate::WebDbPool;

// use serde_json::*;

#[derive(Deserialize)]
pub struct RegisterData {
    /// Username field on supplied json.
    pub username: Option<String>,
    /// Password field on supplied json.
    pub password: String,
}

#[post("/register")]
pub async fn register(pool: WebDbPool, register_data: Json<RegisterData>) -> Result<HttpResponse, ApiError> {
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let username = match &register_data.username {
        Some(un) => un.clone(),
        None => Uuid::new_v4().to_string(),
    };

    let hashed_password = hash(&username, &register_data.password);

    let user = InsertableUser {
        username,
        password: hashed_password,
    };

    if let Err(error) = user.insert(&conn) {
        match error {
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                return Err(ApiError::Db(DbError::UserAlreadyExists))
            }
            _ => return Err(ApiError::Db(DbError::Unknown)),
        }
    }

    Ok(HttpResponse::Ok().json(json!({"username": user.username})))
}

#[derive(Deserialize)]
pub struct LoginData {
    /// Username field on supplied json.
    pub username: String,
    /// Password field on supplied json.
    pub password: String,
}

#[post("/login")]
pub async fn login(pool: WebDbPool, login_data: Json<LoginData>) -> Result<HttpResponse, ApiError> {
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let user = match User::get_by_username(&conn, login_data.username.clone()) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
    };

    if !verify(&user.username, &user.password, &login_data.password) {
        return Err(ApiError::Auth(AuthError::IncorrectPassword));
    }

    let refresh_expiry = 1000000;

    let access_expiry = 10000000;

    let token = jwt_generate(user.uid, None, UserRoles::MasterToken, access_expiry);
    let refresh = jwt_generate_refresh_token(user.uid, UserRoles::MasterToken, refresh_expiry);

    // InsertableApiTokenFull::new(Uuid::new_v4().to_string(), Some(refresh.clone()), user.uid as i32).insert(&conn)?;

    Ok(HttpResponse::Ok().json(json!({"token": token, "refresh": refresh})))
}
