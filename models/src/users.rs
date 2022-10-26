use crate::schema::users;

use diesel::prelude::*;
use diesel::result::Error as DieselError;
use ring::{digest, pbkdf2};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

static PBKDF2_ALG: pbkdf2::Algorithm = pbkdf2::PBKDF2_HMAC_SHA256;
static ITERATIONS: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(100_000) };
const CREDENTIAL_LEN: usize = digest::SHA256_OUTPUT_LEN;

type Credential = [u8; CREDENTIAL_LEN];

#[must_use]
pub fn hash(salt: &str, s: &str) -> String {
    let mut to_store: Credential = [0; CREDENTIAL_LEN];

    pbkdf2::derive(PBKDF2_ALG, ITERATIONS, salt.as_bytes(), s.as_bytes(), &mut to_store);

    base64::encode(&to_store)
}

#[must_use]
pub fn verify(salt: &str, password: &str, attempted_password: &str) -> bool {
    if let Ok(real_pwd) = base64::decode(&password) {
        pbkdf2::verify(
            PBKDF2_ALG,
            ITERATIONS,
            salt.as_bytes(),
            attempted_password.as_bytes(),
            real_pwd.as_slice(),
        )
        .is_ok()
    } else {
        false
    }
}

#[derive(Queryable, Identifiable, Debug, Serialize)]
#[primary_key(uid)]
pub struct User {
    /// User id as a 4 byte wide int
    pub uid: i32,
    /// Account creation
    pub created_at: Option<std::time::SystemTime>,
    /// Username for this row
    pub username: String,
    /// User password hash
    pub password: String,
    /// Internal user flag
    pub is_internal: bool,
    /// Suspension flag
    pub is_suspended: bool,
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "users"]
pub struct InsertableUser {
    pub username: String,
    pub password: String,
    pub is_internal: bool,
    pub is_suspended: bool,
}

impl User {
    pub fn get_by_id(conn: &diesel::PgConnection, uid: i32) -> Result<Self, DieselError> {
        users::dsl::users.filter(users::uid.eq(uid)).first::<Self>(conn)
    }

    pub fn get_by_username(conn: &diesel::PgConnection, username: String) -> Result<Self, DieselError> {
        users::dsl::users
            .filter(users::username.eq(username))
            .first::<Self>(conn)
    }

    pub fn get_all_suspended(conn: &diesel::PgConnection) -> Result<Vec<Self>, DieselError> {
        users::dsl::users
            .filter(users::is_suspended.eq(true))
            .load::<Self>(conn)
    }
}

impl InsertableUser {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<i32, DieselError> {
        diesel::insert_into(users::table)
            .values(self)
            .returning(users::uid)
            .get_result(conn)
    }
}
