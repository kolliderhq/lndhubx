use crate::schema::deezy_btc_ln_swaps;
use crate::schema::deezy_secret_keys;

use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Identifiable, Debug, Serialize)]
#[primary_key(id)]
#[table_name = "deezy_btc_ln_swaps"]
pub struct DeezyBtcLnSwap {
    pub id: i32,
    pub created_at: Option<std::time::SystemTime>,
    pub uid: i32,
    pub ln_address: String,
	pub secret_access_key: String,
	pub btc_address: String,
	pub sig: String,
	pub webhook_url: Option<String>,
}

impl DeezyBtcLnSwap {

    pub fn get_btc_address(conn: &diesel::PgConnection, address: String) -> Result<Self, DieselError> {
        deezy_btc_ln_swaps::dsl::deezy_btc_ln_swaps.filter(deezy_btc_ln_swaps::btc_address.eq(address)).first::<Self>(conn)
    }

    pub fn get_by_sig(conn: &diesel::PgConnection, sig: String) -> Result<Self, DieselError> {
        deezy_btc_ln_swaps::dsl::deezy_btc_ln_swaps
            .filter(deezy_btc_ln_swaps::sig.eq(sig))
            .first::<Self>(conn)
    }
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "deezy_btc_ln_swaps"]
pub struct InsertableDeezyBtcLnSwap {
    pub uid: i32,
    pub ln_address: String,
	pub secret_access_key: String,
	pub btc_address: String,
	pub sig: String,
	pub webhook_url: Option<String>,
}

impl InsertableDeezyBtcLnSwap {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(deezy_btc_ln_swaps::table)
            .values(self)
            .returning(deezy_btc_ln_swaps::btc_address)
            .get_result(conn)
    }
}

#[derive(Queryable, Identifiable, Debug, Serialize)]
#[primary_key(secret_key)]
#[table_name = "deezy_secret_keys"]
pub struct DeezySecretKey {
    pub secret_key: String,
    pub created_at: Option<std::time::SystemTime>,
    pub uid: i32,
}

impl DeezySecretKey {

    pub fn get_by_uid(conn: &diesel::PgConnection, uid: i32) -> Result<Self, DieselError> {
        deezy_secret_keys::dsl::deezy_secret_keys.filter(deezy_secret_keys::uid.eq(uid)).first::<Self>(conn)
    }
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "deezy_secret_keys"]
pub struct InsertableDeezySecretKey {
    pub secret_key: String,
    pub uid: i32,
}

impl InsertableDeezySecretKey {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(deezy_secret_keys::table)
            .values(self)
            .returning(deezy_secret_keys::secret_key)
            .get_result(conn)
    }
}
