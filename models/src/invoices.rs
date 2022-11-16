use crate::schema::invoices;

use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Insertable, Identifiable, Debug, Serialize, AsChangeset, Deserialize)]
#[primary_key(payment_request)]
pub struct Invoice {
    pub payment_request: String,
    pub rhash: String,
    pub payment_hash: String,
    pub created_at: i64,
    pub value: i64,
    pub value_msat: i64,
    pub expiry: i64,
    pub settled: bool,
    pub add_index: i64,
    pub settled_date: i64,
    pub account_id: String,
    pub uid: i32,
    pub incoming: bool,
    pub owner: Option<i32>,
    pub fees: Option<i64>,
    pub currency: Option<String>,
    pub target_account_currency: Option<String>,
}

impl Invoice {
    pub fn get_by_invoice_hash(conn: &diesel::PgConnection, payment_request: String) -> Result<Self, DieselError> {
        invoices::dsl::invoices
            .filter(invoices::payment_request.eq(payment_request))
            .first::<Self>(conn)
    }

    pub fn get_by_payment_request(conn: &diesel::PgConnection, payment_hash: String) -> Result<Self, DieselError> {
        invoices::dsl::invoices
            .filter(invoices::payment_hash.eq(payment_hash))
            .first::<Self>(conn)
    }

    pub fn get_invoices_by_uid(conn: &diesel::PgConnection, uid: i32) -> Result<Vec<Self>, DieselError> {
        invoices::dsl::invoices.filter(invoices::uid.eq(uid)).load::<Self>(conn)
    }

    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(invoices::table)
            .values(self)
            .returning(invoices::payment_request)
            .get_result(conn)
    }

    pub fn update(&self, conn: &diesel::PgConnection) -> Result<usize, DieselError> {
        diesel::update(invoices::dsl::invoices.filter(invoices::account_id.eq(self.account_id.clone())))
            .set(self)
            .execute(conn)
    }
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "invoices"]
pub struct InsertableInvoice {
    pub payment_request: String,
    pub rhash: String,
    pub payment_hash: String,
    pub created_at: i64,
    pub value: i64,
    pub value_msat: i64,
    pub expiry: i64,
    pub settled: bool,
    pub add_index: i64,
    pub settled_date: i64,
    pub account_id: String,
    pub uid: i32,
    pub incoming: bool,
    pub owner: Option<i32>,
    pub fees: Option<i64>,
    pub currency: Option<String>,
    pub target_account_currency: Option<String>,
}

impl InsertableInvoice {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(invoices::table)
            .values(self)
            .returning(invoices::payment_request)
            .get_result(conn)
    }
}
