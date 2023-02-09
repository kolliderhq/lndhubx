use crate::schema::transactions;
use std::time::SystemTime;

use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_types::Numeric;
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
pub struct Transaction {
    pub txid: String,
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
}

impl Transaction {
    pub fn get_by_txid(conn: &diesel::PgConnection, txid: String) -> Result<Self, DieselError> {
        transactions::dsl::transactions
            .filter(transactions::txid.eq(txid))
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
        let owning_transactions = transactions::outbound_uid.eq(uid).or(transactions::inbound_uid.eq(uid));
        transactions::dsl::transactions
            .filter(
                owning_transactions
                    .and(transactions::created_at.ge(from))
                    .and(transactions::created_at.le(to)),
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
        let owning_transactions = transactions::outbound_uid
            .eq(uid)
            .or(transactions::inbound_uid.eq(uid))
            .and(
                transactions::outbound_currency
                    .eq(currency.clone())
                    .or(transactions::inbound_currency.eq(currency)),
            );
        transactions::dsl::transactions
            .filter(
                owning_transactions
                    .and(transactions::created_at.ge(from))
                    .and(transactions::created_at.le(to)),
            )
            .load(conn)
    }

    pub fn get_swap_totals(conn: &PgConnection) -> QueryResult<Vec<SwapTotals>> {
        let query_string = r#"select
                                        sum(inbound_amount) as total,
                                        inbound_currency,
                                        inbound_uid
                                    from
                                        transactions
                                    where
                                        tx_type = 'Internal' and
                                        outbound_uid = 23193913 and
                                        inbound_uid != 23193913 and
                                        inbound_uid != 52172712
                                    group by
                                        inbound_currency,
                                        inbound_uid"#;
        diesel::sql_query(query_string).load::<SwapTotals>(conn)
    }

    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(transactions::table)
            .values(self)
            .returning(transactions::txid)
            .get_result(conn)
    }
}

#[derive(QueryableByName, Debug)]
#[table_name = "transactions"]
pub struct SwapTotals {
    #[sql_type = "Numeric"]
    pub total: BigDecimal,
    pub inbound_currency: String,
    pub inbound_uid: i32,
}
