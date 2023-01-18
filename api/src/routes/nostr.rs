use actix_web::{
    get, post,
    web::{Json, Query, Path},
    HttpResponse,
};

use actix_web::http::header;

use serde::{Deserialize, Serialize};
use serde_json::json;
use xerror::api::*;
use diesel::result::DatabaseErrorKind;
use diesel::result::Error;

use crate::jwt::*;
use crate::WebDbPool;

use models::nostr_public_keys::*;
use models::users::User;

#[derive(Deserialize)]
pub struct NostrPubkeyData {
    pub pubkey: String,
}

#[post("/nostr_pubkey")]
pub async fn set_nostr_pubkey(pool: WebDbPool, auth_data: AuthData, data: Json<NostrPubkeyData>) -> Result<HttpResponse, ApiError> {
    let uid = auth_data.uid as u64;

    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

    let user = match User::get_by_id(&conn, uid as i32) {
        Ok(u) => u,
        Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
    };

    let nostr_pubkey = InsertableNostrPublicKey {
        pubkey: data.pubkey.clone(),
        uid: uid as i32
    };

    if let Err(err) = nostr_pubkey.insert(&conn) {
		match err {
			Error::DatabaseError(err, _) => {
				match err {
					DatabaseErrorKind::UniqueViolation => {
						return Ok(HttpResponse::Ok()
							.insert_header((header::CONTENT_TYPE, "application/json"))
							.json(json!({"status": "User has Pubkey already set.".to_string()})))
					},
					_ => {
						return Ok(HttpResponse::Ok()
							.insert_header((header::CONTENT_TYPE, "application/json"))
							.json(json!({"status": "error inserting".to_string()})))
					}
				}
			}
			_ => {
				return Ok(HttpResponse::Ok()
					.insert_header((header::CONTENT_TYPE, "application/json"))
					.json(json!({"status": "error inserting".to_string()})))
			}
		}
		dbg!(&err);
    }

    Ok(HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .json(json!({"status": "ok".to_string()})))
}

#[post("/update_nostr_pubkey")]
pub async fn update_nostr_pubkey(pool: WebDbPool, auth_data: AuthData, data: Json<NostrPubkeyData>) -> Result<HttpResponse, ApiError> {
    let uid = auth_data.uid as u64;

    let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

	if let Ok(_) = NostrPublicKey::get_by_uid(&conn, uid as i32) {
		let update_pubkey = UpdateNostrPublicKey {
			pubkey: data.pubkey.clone()
		};
		if let Err(_) = update_pubkey.update(&conn, uid as i32) {
			return Ok(HttpResponse::Ok()
				.insert_header((header::CONTENT_TYPE, "application/json"))
				.json(json!({"status": "error updating pubkey.".to_string()})))
		}
	} else {
		return Ok(HttpResponse::Ok()
			.insert_header((header::CONTENT_TYPE, "application/json"))
			.json(json!({"status": "Error: No public key exists for user.".to_string()})))
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

  return Ok(HttpResponse::Ok().json(resp));
}

// #[derive(Deserialize)]
// pub struct GetNostrProfileParams {
//     ln_address: Option<String>,
// 	pubkey: Option<String>
// }

// #[get("/get_nostr_profile")]
// pub async fn get_nostr_profile(pool: WebDbPool, params: Query<GetNostrProfileParams>) -> Result<HttpResponse, ApiError> {
//   let username = params.name.clone();
//   let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

//   let nostr_pubkey = match NostrPublicKey::get_by_username(&conn, username.clone()) {
//     Ok(u) => u,
//     Err(_) => return Err(ApiError::Db(DbError::UserDoesNotExist)),
//   };

//   let resp = json!({
//       "names": json!({
// 		format!("{}", username): nostr_pubkey.pubkey
// 	  })
//   });

//   return Ok(HttpResponse::Ok().json(resp));
// }