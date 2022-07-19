use actix_web::{post, get, web::Json, HttpResponse};
use actix_web::{
    web::{Query},
};
use diesel::result::DatabaseErrorKind;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use xerror::api::*;

use bech32::{self, ToBase32};
use secp256k1::{Message, PublicKey, Secp256k1, Signature};
use rand::Rng;
use models::users::*;
use models::{
    ln_users::{InsertableLnChallengeMap, InsertableLnUser, LnChallengeMap, LnUser},
    users::{InsertableApiTokenFull},
};
use std::str::FromStr;

use crate::jwt::*;
use crate::WebDbPool;

// use serde_json::*;

const PREFIX: &str = "lnurl";

static ACCESS_EXPIRY: i64 = 60 * 60 * 12;
static REFRESH_EXPIRY: i64 = 60 * 60 * 24 * 3;

#[derive(Deserialize)]
pub struct RegisterData {
    /// Username field on supplied json.
    pub username: Option<String>,
    /// Password field on supplied json.
    pub password: String,
}

#[post("/create")]
pub async fn create(pool: WebDbPool, register_data: Json<RegisterData>) -> Result<HttpResponse, ApiError> {
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

#[post("/auth")]
pub async fn auth(pool: WebDbPool, login_data: Json<LoginData>) -> Result<HttpResponse, ApiError> {
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

pub fn encode(url: String, q: Option<String>) -> String {
    let mut unencoded = url;
    if let Some(id) = q {
        unencoded = format!("{}?q={}", unencoded, id);
    }
    let encoded = bech32::encode(PREFIX, unencoded.as_bytes().to_vec().to_base32()).unwrap();
    encoded
}

pub fn generate_random_hex_string(len: usize) -> String {
    let mut rng = rand::thread_rng();
    let mut random_string = String::with_capacity(len);
    let mut remaining_len = len;
    while remaining_len > 0 {
        let random_bytes = rng.gen::<[u8; 32]>();
        let random_chunk = hex::encode(random_bytes);
        let chunk_len_required = std::cmp::min(remaining_len, random_chunk.len());
        random_string.push_str(&random_chunk[0..chunk_len_required]);
        remaining_len -= chunk_len_required;
    }
    random_string
}

pub fn create_auth_lnurl(
    domain: String,
    tag: String,
    action: Option<String>,
    challenge: Option<String>,
) -> (String, String) {
    let random_string = match challenge {
        Some(c) => c,
        None => generate_random_hex_string(64),
    };
    let mut url = format!("{}?tag={}&k1={}", domain, tag, random_string);
    if let Some(action) = action {
        url = format!("{}&action={}", url, action);
    }
    (url, random_string)
}

#[get("/auth/lnurl_auth")]
pub async fn get_auth_lnurl(pool: WebDbPool, auth_data: AuthData) -> Result<HttpResponse, ApiError> {
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let uid = auth_data.uid as i32;

    // let domain = &(**shared_settings).read().await.domain;
    let domain = "https://pay.alpha.kollider.xyz";
    let url = format!("{}auth/ln_login", domain);
    let tag = String::from("login");

    let (lnurl, challenge) = create_auth_lnurl(url, tag, None, None);
    let lnurl_encoded = encode(lnurl, None);

    let challenge_map = InsertableLnChallengeMap { challenge, uid };
    challenge_map.insert(&conn).unwrap();

    Ok(HttpResponse::Ok().json(json!({ "lnurl_auth": lnurl_encoded })))
}

pub fn verify_lnurl_auth(sig: String, public_key: String, expected: String) -> bool {
    let exp = hex::decode(expected).unwrap();
    let secp = Secp256k1::new();
    let public_key = PublicKey::from_str(&public_key).unwrap();
    let sig = Signature::from_str(&sig).unwrap();
    let message = Message::from_slice(&exp).unwrap();
    secp.verify(&message, &sig, &public_key).is_ok()
}

#[derive(Deserialize, Debug)]
pub struct LnAuthParams {
    sig: String,
    key: String,
    k1: String,
}

#[get("/auth/ln_login")]
pub async fn get_lnauth_login(
    pool: WebDbPool,
    args: Query<LnAuthParams>,
) -> Result<HttpResponse, ApiError> {
    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;
    let pass = verify_lnurl_auth(args.sig.clone(), args.key.clone(), args.k1.clone());

    if !pass {
        return Err(ApiError::Auth(AuthError::InvalidLNAuth));
    }

    let challenge_map = match LnChallengeMap::get_by_challenge(&conn, args.k1.clone()) {
        Ok(m) => m,
        Err(_) => return Err(ApiError::Auth(AuthError::InternalError)),
    };

    // there are two cases
    // a. one where the user already has a key registered as its lnauth key, and it matches with the one it's providing
    // b. another where the key does not match with the registered one
    // on case a. we just take the user details from the LnUser table, and do not create any mapping
    // on case b. we take the user details from the challenge, and will have to create a new mapping based on these details.

    let uid = if let Ok(u) = LnUser::get_by_public_key(&conn, args.key.clone()) {
        u.uid
    } else {
        challenge_map.uid
    };

    let mut user = User::get_by_id(&conn, uid).unwrap();
    // TODO: creating users
    // if the user is light, create a new one
    // if user.user_type_id == light_user_type_id {
    //     let random_string = uuid::Uuid::new_v4().to_string();

    //     let email = format!("{}-lnurl-auth@kollider.xyz", random_string);
    //     let hashed_password = hash(&email, &args.k1);

    //     let new_user = InsertableUser {
    //         username: Some(random_string),
    //         password: Some(hashed_password),
    //         email: Some(email),
    //         client_id: Some(1),
    //         user_type_id: Some(ln_user_type_id),
    //     };
    //     let uid = new_user.insert(&conn)?;

    //     // update the user with the created one
    //     user = User::get_by_id(&conn, uid).unwrap();
    // }

    let ln_user = InsertableLnUser {
        uid: user.uid,
        pub_key: args.key.clone(),
        prevent_delete: true,
    };

    ln_user.insert(&conn)?;

    let access_expiry = ACCESS_EXPIRY;
    let refresh_expiry = REFRESH_EXPIRY;

    let token = jwt_generate(user.uid, None, UserRoles::MasterToken, access_expiry);
    let refresh = jwt_generate_refresh_token(user.uid, UserRoles::MasterToken, refresh_expiry);
    InsertableApiTokenFull::new(Uuid::new_v4().to_string(), Some(refresh.clone()), user.uid as i32).insert(&conn)?;

    // let msg = Message::External(ExternalMessage::LnurlAuthCredentials(LnurlAuthCredentials {
    //     uid: challenge_map.uid as u64,
    //     access_token: token.clone(),
    //     refresh_token: refresh.clone(),
    // }));

    // Arc::make_mut(&mut node.into_inner())
    //     .send(Envelope {
    //         message: msg,
    //         response_tx: None,
    //         response_filter: None,
    //         destinations: vec![Identity::MarketDataGateway],
    //     })
    //     .await
    //     .map_err(|_| ApiError::Auth(AuthError::InternalError))?;

    Ok(HttpResponse::Ok().json(json!({ "status": "OK".to_string() })))
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