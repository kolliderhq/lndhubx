use crate::schema::ln_addresses;

use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Identifiable, Debug, Serialize)]
#[primary_key(id)]
#[table_name = "ln_addresses"]
pub struct LnAddress {
    pub id: i32,
    pub created_at: Option<std::time::SystemTime>,
    pub username: String,
    pub domain: String,
}

impl LnAddress {
    pub fn get_by_id(conn: &diesel::PgConnection, id: i32) -> Result<Self, DieselError> {
        ln_addresses::dsl::ln_addresses.filter(ln_addresses::id.eq(id)).first::<Self>(conn)
    }

    pub fn get_by_username(conn: &diesel::PgConnection, username: String) -> Result<Self, DieselError> {
        ln_addresses::dsl::ln_addresses
            .filter(ln_addresses::username.eq(username))
            .first::<Self>(conn)
    }
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "ln_addresses"]
pub struct InsertableLnAddress {
    pub username: String,
    pub domain: String,
}

impl InsertableLnAddress {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<i32, DieselError> {
        diesel::insert_into(ln_addresses::table)
            .values(self)
            .returning(ln_addresses::id)
            .get_result(conn)
    }
}