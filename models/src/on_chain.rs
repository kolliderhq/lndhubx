use crate::schema::{bitcoin_addresses, onchain_transactions};
use diesel::prelude::*;
use diesel::result::Error as DieselError;

#[derive(Queryable, Identifiable, Insertable, Debug)]
#[primary_key(txid)]
pub struct OnchainTransaction {
    pub txid: String,
    pub uid: i32,
    pub timestamp: i64,
    pub address: String,
    pub block_number: i64,
    pub confirmations: i64,
    pub fee: i64,
    pub tx_type: String,
    pub is_confirmed: bool,
    pub network: String,
    pub value: i64,
}

impl OnchainTransaction {
    pub fn get_by_txid(conn: &PgConnection, txid: &str) -> Result<Self, DieselError> {
        onchain_transactions::dsl::onchain_transactions
            .filter(onchain_transactions::txid.eq(txid))
            .first::<Self>(conn)
    }

    pub fn insert(&self, conn: &PgConnection) -> Result<Self, DieselError> {
        diesel::insert_into(onchain_transactions::table)
            .values(self)
            .get_result(conn)
    }
}

#[derive(AsChangeset, Debug)]
#[table_name = "onchain_transactions"]
pub struct OnchainTransactionUpdate {
    pub txid: String,
    pub uid: Option<i32>,
    pub timestamp: Option<i64>,
    pub address: Option<String>,
    pub block_number: Option<i64>,
    pub confirmations: Option<i64>,
    pub fee: Option<i64>,
    pub tx_type: Option<String>,
    pub is_confirmed: Option<bool>,
    pub network: Option<String>,
    pub value: Option<i64>,
}

impl OnchainTransactionUpdate {
    pub fn update(&self, conn: &PgConnection) -> QueryResult<usize> {
        diesel::update(
            onchain_transactions::dsl::onchain_transactions.filter(onchain_transactions::txid.eq(self.txid.clone())),
        )
        .set(self)
        .execute(conn)
    }
}

#[derive(Queryable, Identifiable, Insertable, Debug)]
#[table_name = "bitcoin_addresses"]
#[primary_key(address)]
pub struct BitcoinAddress {
    pub address: String,
    pub uid: i32,
}

impl BitcoinAddress {
    pub fn get_by_address(conn: &PgConnection, address: &str) -> Result<Self, DieselError> {
        bitcoin_addresses::dsl::bitcoin_addresses
            .filter(bitcoin_addresses::address.eq(address))
            .first::<Self>(conn)
    }

    pub fn insert(&self, conn: &PgConnection) -> Result<Self, DieselError> {
        diesel::insert_into(bitcoin_addresses::table)
            .values(self)
            .get_result(conn)
    }
}
