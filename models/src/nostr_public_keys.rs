use crate::schema::nostr_public_keys;
use crate::schema::users;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use std::default::Default;

#[derive(Queryable, Identifiable, Debug)]
#[primary_key(pubkey)]
pub struct NostrPublicKey {
    pub created_at: Option<std::time::SystemTime>,
    pub pubkey: String,
	pub uid: i32,
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "nostr_public_keys"]
pub struct InsertableNostrPublicKey {
    pub pubkey: String,
    pub uid: i32,
}

#[derive(Default, AsChangeset, Debug, Deserialize)]
#[table_name = "nostr_public_keys"]
pub struct UpdateNostrPublicKey {
    pub pubkey: String,
}


impl NostrPublicKey {
    pub fn get_by_pubkey(conn: &diesel::PgConnection, pubkey: String) -> Result<Self, DieselError> {
        nostr_public_keys::dsl::nostr_public_keys
            .filter(nostr_public_keys::pubkey.eq(pubkey))
            .first::<Self>(conn)
    }
    pub fn get_by_uid(conn: &diesel::PgConnection, uid: i32) -> Result<Self, DieselError> {
        nostr_public_keys::dsl::nostr_public_keys
            .filter(nostr_public_keys::uid.eq(uid))
            .first::<Self>(conn)
    }
    pub fn get_by_username(conn: &diesel::PgConnection, username: String) -> Result<Self, DieselError> {
        users::dsl::users
			.inner_join(nostr_public_keys::dsl::nostr_public_keys)
            .select((
                nostr_public_keys::created_at,
                nostr_public_keys::pubkey,
                nostr_public_keys::uid,
            ))
            .filter(users::username.eq(username))
            .first::<Self>(conn)
    }
}

impl InsertableNostrPublicKey {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(nostr_public_keys::table)
            .values(self)
            .returning(nostr_public_keys::pubkey)
            .get_result(conn)
    }
}

impl UpdateNostrPublicKey {
    pub fn update(&self, conn: &diesel::PgConnection, uid: i32) -> Result<usize, DieselError> {
        diesel::update(nostr_public_keys::dsl::nostr_public_keys.filter(nostr_public_keys::uid.eq(uid)))
            .set(self)
            .execute(conn)
    }
}
