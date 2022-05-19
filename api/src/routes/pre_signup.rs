use actix_web::{
	post,
	web::{Json},
	HttpResponse,
};

use serde::Deserialize;
use xerror::api::*;

use crate::WebDbPool;

use models::pre_signups::*;

#[derive(Deserialize)]
pub struct PreSignupData {
	/// Password field on supplied json.
	pub email: String,
}

#[post("/presignup")]
pub async fn pre_signup(pool: WebDbPool, pre_signup_data: Json<PreSignupData>) -> Result<HttpResponse, ApiError> {
	let conn = pool.get().map_err(|_| ApiError::Db(DbError::DbConnectionError))?;

	let pre_signup = InsertablePreSignup {
		email: pre_signup_data.email.clone(),
	};
	match pre_signup.insert(&conn) {
		Ok(_) => Ok(HttpResponse::Ok().finish()),
		Err(_) => {Err(ApiError::Auth(AuthError::UserExists))}
	}
}
