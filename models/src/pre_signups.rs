use crate::schema::pre_signups;
use diesel::result::Error as DieselError;
use diesel::{prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Queryable, Identifiable, Debug, Serialize)]
#[primary_key(uid)]
pub struct PreSignup {
    /// User id as a 4 byte wide int
    pub uid: i32,
    /// Accountn creation
    pub created_at: Option<std::time::SystemTime>,
    /// Username for this row
    pub email: String,
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "pre_signups"]
pub struct InsertablePreSignup {
    pub email: String,
}

impl InsertablePreSignup {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<i32, DieselError> {
        diesel::insert_into(pre_signups::table)
            .values(self)
            .returning(pre_signups::uid)
            .get_result(conn)
    }
}