use crate::schema::dca_settings;
use crate::schema::users;
use bigdecimal::BigDecimal;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};
use std::default::Default;
use uuid::Uuid;

#[derive(Queryable, Identifiable, Debug, Serialize, Deserialize)]
#[primary_key(id)]
pub struct DcaSetting {
    pub id: i32,
    pub uid: i32,
    pub interval: String,
    pub amount: BigDecimal,
    pub from_currency: String,
    pub to_currency: String,
}

impl DcaSetting {
    pub fn get_by_uid(conn: &diesel::PgConnection, uid: i32) -> Result<Self, DieselError> {
        dca_settings::dsl::dca_settings
            .filter(dca_settings::uid.eq(uid))
            .first::<Self>(conn)
    }
    pub fn get_by_interval(conn: &diesel::PgConnection, interval: String) -> Result<Vec<Self>, DieselError> {
        dca_settings::dsl::dca_settings
            .filter(dca_settings::interval.eq(interval))
            .load(conn)
    }
    pub fn delete(conn: &PgConnection, uid: i32) -> QueryResult<usize> {
        diesel::delete(
            dca_settings::dsl::dca_settings
                .filter(dca_settings::dsl::uid.eq(uid))
        )
        .execute(conn)
    }
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "dca_settings"]
pub struct InsertableDcaSetting {
    pub uid: i32,
    pub interval: String,
    pub amount: BigDecimal,
    pub from_currency: String,
    pub to_currency: String
}

impl InsertableDcaSetting {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<i32, DieselError> {
        diesel::insert_into(dca_settings::table)
            .values(self)
            .returning(dca_settings::id)
            .get_result(conn)
    }
}