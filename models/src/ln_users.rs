use crate::schema::{ln_challenge_maps, ln_users};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Identifiable, Debug, Serialize)]
#[primary_key(uid)]
pub struct LnUser {
    // Time at which the user was created
    pub created_at: Option<std::time::SystemTime>,
    // Key identifying the user.
    pub pub_key: String,
    // User id.
    pub uid: i32,
    // Prevent deletion flag
    pub prevent_delete: bool,
    // Human readable alias
    pub alias: Option<String>,
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "ln_users"]
pub struct InsertableLnUser {
    pub pub_key: String,
    pub uid: i32,
    pub prevent_delete: bool,
}

impl InsertableLnUser {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(ln_users::table)
            .values(self)
            .returning(ln_users::pub_key)
            .get_result(conn)
    }
}

impl LnUser {
    pub fn get_by_public_key(conn: &diesel::PgConnection, public_key: String) -> Result<Self, DieselError> {
        ln_users::dsl::ln_users
            .filter(ln_users::pub_key.eq(public_key))
            .first::<Self>(conn)
    }
    pub fn get_by_uid(conn: &diesel::PgConnection, uid: i32) -> Result<Vec<Self>, DieselError> {
        ln_users::dsl::ln_users.filter(ln_users::uid.eq(uid)).load(conn)
    }

    pub fn delete_public_key(conn: &diesel::PgConnection, uid: i32, public_key: String) -> Result<usize, DieselError> {
        // Ideally we would form an expression using diesel's dsl like below
        // but it does not work as delete does not seem to work with sub-queries
        // let validated_email_count = users::dsl::users
        //     .filter(users::uid.eq(uid))
        //     .filter(users::validated_email.eq(true))
        //     .select(diesel::dsl::count_star())
        //     .single_value();
        //
        // let remaining_pub_keys_count = ln_users::dsl::ln_users
        //     .filter(ln_users::uid.eq(uid))
        //     .select(diesel::dsl::count_star())
        //     .single_value();
        //
        // let delete_expression = diesel::delete(
        //     ln_users::dsl::ln_users
        //         .filter(ln_users::pub_key.eq(public_key))
        //         .filter(ln_users::uid.eq(uid))
        //         .filter(ln_users::prevent_delete.eq(false))
        //         .filter(validated_email_count.lt(remaining_pub_keys_count)),
        // );

        // so instead we are opting in for raw query
        let delete_expression = diesel::sql_query(
            format!("\
            DELETE FROM ln_users \
            WHERE \
            uid = {} AND \
            pub_key = '{}' AND \
            prevent_delete = false AND \
            (SELECT COUNT(*) FROM users WHERE uid = {} AND validated_email = false) < (SELECT COUNT(*) FROM ln_users where uid = {})", uid, public_key, uid, uid));

        delete_expression.execute(conn)
    }

    pub fn update_prevent_delete_flag(
        conn: &diesel::PgConnection,
        uid: i32,
        public_key: String,
        prevent_delete: bool,
    ) -> QueryResult<usize> {
        diesel::update(
            ln_users::dsl::ln_users
                .filter(ln_users::uid.eq(uid))
                .filter(ln_users::pub_key.eq(public_key)),
        )
        .set(ln_users::prevent_delete.eq(prevent_delete))
        .execute(conn)
    }
}

#[derive(Queryable, Identifiable, Debug, Serialize, Clone)]
#[primary_key(challenge)]
pub struct LnChallengeMap {
    pub challenge: String,
    pub uid: i32,
}

impl LnChallengeMap {
    pub fn get_by_challenge(conn: &diesel::PgConnection, challenge: String) -> Result<Self, DieselError> {
        ln_challenge_maps::dsl::ln_challenge_maps
            .filter(ln_challenge_maps::challenge.eq(challenge))
            .first::<Self>(conn)
    }
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "ln_challenge_maps"]
pub struct InsertableLnChallengeMap {
    pub challenge: String,
    pub uid: i32,
}

impl InsertableLnChallengeMap {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(ln_challenge_maps::table)
            .values(self)
            .returning(ln_challenge_maps::challenge)
            .get_result(conn)
    }
}