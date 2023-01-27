use crate::schema::summary_transactions;
use std::time::SystemTime;

use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};

use bigdecimal::BigDecimal;
use uuid::Uuid;

fn time_now_as_i64() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("System time should not be earlier than epoch start")
        .as_millis() as i64
}

#[derive(Queryable, Identifiable, Insertable, Debug, Serialize, Deserialize)]
#[primary_key(txid)]
pub struct SummaryTransaction {
    pub txid: String,
    pub fee_txid: Option<String>,
    pub outbound_txid: Option<String>,
    pub inbound_txid: Option<String>,
    pub created_at: i64,
    pub outbound_amount: BigDecimal,
    pub inbound_amount: BigDecimal,
    pub outbound_account_id: Uuid,
    pub inbound_account_id: Uuid,
    pub outbound_uid: i32,
    pub inbound_uid: i32,
    pub outbound_currency: String,
    pub inbound_currency: String,
    pub exchange_rate: BigDecimal,
    pub tx_type: String,
    pub fees: BigDecimal,
    pub reference: Option<String>,
    pub outbound_username: Option<String>,
    pub inbound_username: Option<String>,
}

impl SummaryTransaction {
    pub fn get_by_txid(conn: &diesel::PgConnection, txid: String) -> Result<Self, DieselError> {
        summary_transactions::dsl::summary_transactions
            .filter(summary_transactions::txid.eq(txid))
            .first::<Self>(conn)
    }

    pub fn get_historical_by_uid(
        conn: &diesel::PgConnection,
        uid: i32,
        from: Option<i64>,
        to: Option<i64>,
    ) -> Result<Vec<Self>, DieselError> {
        let from = from.unwrap_or(0);
        let to = to.unwrap_or_else(time_now_as_i64);
        let owning_transactions = summary_transactions::outbound_uid
            .eq(uid)
            .or(summary_transactions::inbound_uid.eq(uid));
        summary_transactions::dsl::summary_transactions
            .filter(
                owning_transactions
                    .and(summary_transactions::created_at.ge(from))
                    .and(summary_transactions::created_at.le(to)),
            )
            .load(conn)
    }

    pub fn get_historical_by_uid_and_currency(
        conn: &diesel::PgConnection,
        uid: i32,
        currency: String,
        from: Option<i64>,
        to: Option<i64>,
    ) -> Result<Vec<Self>, DieselError> {
        let from = from.unwrap_or(0);
        let to = to.unwrap_or_else(time_now_as_i64);
        let owning_transactions = summary_transactions::outbound_uid
            .eq(uid)
            .or(summary_transactions::inbound_uid.eq(uid))
            .and(
                summary_transactions::outbound_currency
                    .eq(currency.clone())
                    .or(summary_transactions::inbound_currency.eq(currency)),
            );
        summary_transactions::dsl::summary_transactions
            .filter(
                owning_transactions
                    .and(summary_transactions::created_at.ge(from))
                    .and(summary_transactions::created_at.le(to)),
            )
            .load(conn)
    }

    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(summary_transactions::table)
            .values(self)
            .returning(summary_transactions::txid)
            .get_result(conn)
    }
}
