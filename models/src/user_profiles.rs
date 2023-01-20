use crate::schema::{user_profiles, users};

use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};


#[derive(Queryable, Identifiable, Debug, Serialize, Deserialize)]
#[primary_key(uid)]
pub struct UserProfile {
    pub uid: i32,
    pub email: Option<String>,
    pub nostr_notifications: Option<bool>,
    pub email_notifications: Option<bool>,
	pub img_url: Option<String>,
	pub twitter_handle: Option<String>,
    pub is_twitter_verified: Option<bool>,
    pub is_email_verified: Option<bool>,
}

#[derive(Insertable, Debug, Deserialize, Serialize)]
#[table_name = "user_profiles"]
pub struct InsertableUserProfile {
    pub uid: i32,
    pub email: Option<String>,
    pub nostr_notifications: Option<bool>,
    pub email_notifications: Option<bool>,
	pub img_url: Option<String>,
	pub twitter_handle: Option<String>,
    pub is_twitter_verified: Option<bool>,
    pub is_email_verified: Option<bool>,
}

#[derive(Default, AsChangeset, Debug, Deserialize)]
#[table_name = "user_profiles"]
pub struct UpdateUserProfile {
    pub email: Option<String>,
    pub nostr_notifications: Option<bool>,
    pub email_notifications: Option<bool>,
	pub img_url: Option<String>,
	pub twitter_handle: Option<String>,
    pub is_twitter_verified: Option<bool>,
    pub is_email_verified: Option<bool>,
}

impl UserProfile {
    pub fn get_by_uid(conn: &diesel::PgConnection, uid: i32) -> Result<Self, DieselError> {
        user_profiles::dsl::user_profiles.filter(user_profiles::uid.eq(uid)).first::<Self>(conn)
    }
}

impl InsertableUserProfile {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<i32, DieselError> {
        diesel::insert_into(user_profiles::table)
            .values(self)
            .returning(user_profiles::uid)
            .get_result(conn)
    }
}

impl UpdateUserProfile {
    pub fn update(&self, conn: &diesel::PgConnection, uid: i32) -> Result<usize, DieselError> {
        diesel::update(user_profiles::dsl::user_profiles.filter(user_profiles::uid.eq(uid)))
            .set(self)
            .execute(conn)
    }
}

/// Structure to use to share user data outside the application
#[derive(Debug, Serialize)]
pub struct ShareableUserProfile {
    pub uid: i32,
    pub img_url: Option<String>,
}