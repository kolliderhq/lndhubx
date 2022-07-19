use crate::schema::{
    api_tokens, users,
};
use diesel::result::Error as DieselError;
use diesel::{prelude::*};
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
    let real_pwd = base64::decode(&password).unwrap();

    pbkdf2::verify(
        PBKDF2_ALG,
        ITERATIONS,
        salt.as_bytes(),
        attempted_password.as_bytes(),
        real_pwd.as_slice(),
    )
    .is_ok()
}

#[derive(Queryable, Identifiable, Debug, Serialize)]
#[primary_key(uid)]
pub struct User {
    /// User id as a 4 byte wide int
    pub uid: i32,
    /// Accountn creation
    pub created_at: Option<std::time::SystemTime>,
    /// Username for this row
    pub username: String,
    /// User email
    pub password: String,
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "users"]
pub struct InsertableUser {
    pub username: String,
    pub password: String,
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
}

impl InsertableUser {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<i32, DieselError> {
        diesel::insert_into(users::table)
            .values(self)
            .returning(users::uid)
            .get_result(conn)
    }
}

#[derive(Queryable, Identifiable, Debug, Associations)]
#[belongs_to(User, foreign_key = "uid")]
#[table_name = "api_tokens"]
pub struct ApiToken {
    pub id: i32,
    pub name: String,
    pub key: Option<String>,
    pub uid: i32,
    pub revoked: Option<bool>,
}

#[derive(Insertable, Debug)]
#[table_name = "api_tokens"]
pub struct InsertableApiToken {
    pub name: String,
    pub uid: i32,
}

impl ApiToken {
    /// Function `get_for_user` is used to fetch all of the API keys belonging to the user.
    ///
    /// # Arguments
    /// * `conn` - pg connection
    /// * `uid` - user id for which we want to fetch api keys
    ///
    /// # Example
    /// ```compile_fail
    /// let apis = ApiToken::get_for_user(&conn, 1);
    /// assert!(apis.is_ok());
    /// ```
    ///
    /// # Errors
    /// The `DieselError` is propagated from diesel. `Ok(..)` is returned even if the query doesnt
    /// find any matching fields. `Err(..)` is returned if the transaction fails or if no user was
    /// found by the `uid` supplied.
    pub fn get_for_user(conn: &diesel::PgConnection, uid: i32) -> Result<Vec<Self>, DieselError> {
        User::get_by_id(conn, uid).and_then(|ref u| Self::belonging_to(u).load::<Self>(conn))
    }

    /// Function `get_by_id` takes in a key id and returns the key object.
    ///
    /// # Arguments
    /// ```compile_fail
    /// let key = ApiToken::get_by_id(&conn, 123);
    /// assert!(key.is_ok());
    /// ```
    ///
    /// # Errors
    /// This function propagates the `DieselError` provided by `diesel` and does not alter or map
    /// them at all. Error will be returned if the `filter` predicate doesnt find any rows
    /// matching, or if the transaction fails.
    pub fn get_by_key(conn: &diesel::PgConnection, key: &str, uid: i32) -> Result<Self, DieselError> {
        api_tokens::dsl::api_tokens
            .filter(api_tokens::key.eq(key))
            .filter(api_tokens::uid.eq(uid))
            .first::<Self>(conn)
    }

    /// Function `delete` is used to remove a issued api token by its id.
    ///
    /// # Arguments
    /// * `conn` - pg connection
    /// * `kid` - api key id stored within a jwt token
    ///
    /// # Example
    /// ```compile_fail
    /// let token = ApiToken::get_by_id(&conn, 123);
    /// assert!(token.is_ok());
    ///
    /// let count = ApiToken::delete(&conn, 123);
    /// assert!(count, Ok(1));
    ///
    /// let token = ApiToken::get_by_id(&conn, 123);
    /// assert!(token.is_err());
    /// ```
    ///
    /// # Errors
    /// This function propagates the `DieselError` provided by `diesel` and does not alter or map
    /// them at all.
    pub fn delete(conn: &diesel::PgConnection, kid: i32) -> Result<usize, DieselError> {
        diesel::delete(api_tokens::table.filter(api_tokens::id.eq(kid))).execute(conn)
    }

    /// Function `delete_of_user` is used to remove a key filtered by `kid` of user filtered by
    /// `uid`.
    ///
    /// # Arguments
    /// * `conn` - postgres connection.
    /// * `uid` - user id
    /// * `kid` - key id
    ///
    /// # Errors
    /// Function simply propagates the `DieselError` from `diesel` without altering or mapping it.
    /// Function will return `Err(..)` will fail only if the transaction fails. If the `filter`
    /// predicate is invalid the function will still return `Ok(0)`.
    pub fn delete_of_user(conn: &diesel::PgConnection, uid: i32, kid: i32) -> Result<usize, DieselError> {
        diesel::delete(api_tokens::table.filter(api_tokens::id.eq(kid).and(api_tokens::uid.eq(uid)))).execute(conn)
    }

    pub fn revoke(conn: &diesel::PgConnection, key: &str) -> Result<usize, DieselError> {
        diesel::update(api_tokens::table.filter(api_tokens::key.eq(key)))
            .set(api_tokens::dsl::revoked.eq(true))
            .execute(conn)
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked.unwrap_or(false)
    }
}

impl InsertableApiToken {
    /// Function `insert_then_update` is used in the API key generation process.
    ///
    /// # Arguments
    /// * `conn`: connection instance for Pg
    /// * `f`: this is a closure that takes in the id of the new `api_key` in the db, then spits out
    /// a JWT.
    ///
    /// # Example
    /// ```compile_fail
    /// let new_token = InsertableApiToken {
    ///     name: "MyAPI".into(),
    ///     uid: 1,
    /// };
    ///
    /// new_token.insert_then_update(&conn, |id| format!("MY_KEY {}", id).into()).unwrap();
    /// ```
    ///
    /// # Errors
    /// This function propagates the `DieselError` provided by `diesel` and does not alter or map
    /// them at all.
    pub fn insert_then_update<F: FnOnce() -> String>(
        &self,
        conn: &diesel::PgConnection,
        f: F,
    ) -> Result<Option<String>, DieselError> {
        // We first query for a user by the id just to do some validation, we dont want to insert
        // keys for users that dont exsits -> we then insert a blank entry -> then we call `f(id)`
        // and update the field with our API Token.
        //
        // The reason we are doing it this way is because the tokens themselves will be
        // self-refferential to their id in the database. This seemed like the most efficient way
        // to this and avoid tons on-auth db calls.
        User::get_by_id(conn, self.uid)
            .and_then(|_| {
                diesel::insert_into(api_tokens::table)
                    .values(self)
                    .returning(api_tokens::uid)
                    .get_result(conn)
            })
            .and_then(|uid: i32| {
                diesel::update(api_tokens::dsl::api_tokens.filter(api_tokens::uid.eq(uid)))
                    .set(api_tokens::key.eq(f()))
                    .returning(api_tokens::key)
                    .get_result(conn)
            })
    }
}

/// This is the struct used when an ApiToken has to be inserted, and all the fields
/// are specified
#[derive(Insertable, Debug)]
#[table_name = "api_tokens"]
pub struct InsertableApiTokenFull {
    pub name: String,
    pub key: Option<String>,
    pub uid: i32,
}

impl InsertableApiTokenFull {
    pub const fn new(name: String, key: Option<String>, uid: i32) -> Self {
        Self { name, key, uid }
    }

    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<usize, DieselError> {
        diesel::insert_into(api_tokens::table).values(self).execute(conn)
    }
}
