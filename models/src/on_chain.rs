use crate::schema::{bitcoin_addresses, onchain_transactions};
use diesel::prelude::*;
use diesel::result::Error as DieselError;

#[derive(Queryable, Identifiable, Insertable, Debug)]
#[primary_key(txid)]
pub struct OnchainTransaction {
    pub txid: String,
    pub is_settled: bool,
}

impl OnchainTransaction {
    pub fn get_by_txid(conn: &PgConnection, txid: &str) -> Result<Self, DieselError> {
        onchain_transactions::dsl::onchain_transactions
            .filter(onchain_transactions::txid.eq(txid))
            .first::<Self>(conn)
    }

    pub fn set_settled(conn: &PgConnection, txid: &str) -> QueryResult<usize> {
        diesel::update(onchain_transactions::dsl::onchain_transactions.find(txid))
            .set(onchain_transactions::dsl::is_settled.eq(true))
            .execute(conn)
    }

    pub fn insert(&self, conn: &PgConnection) -> Result<Self, DieselError> {
        diesel::insert_into(onchain_transactions::table)
            .values(self)
            .get_result(conn)
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
