use crate::schema::accounts;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use std::default::Default;
use std::str::FromStr;
use uuid::Uuid;

use bigdecimal::BigDecimal;

#[derive(Queryable, Identifiable, Debug)]
#[primary_key(account_id)]
pub struct Account {
    pub account_id: Uuid,
    pub balance: BigDecimal,
    pub currency: String,
    pub account_type: String,
    pub uid: i32,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            account_id: Uuid::new_v4(),
            balance: BigDecimal::from_str("0").unwrap(),
            currency: String::from("BTC"),
            account_type: String::from("Internal"),
            uid: 0,
        }
    }
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "accounts"]
pub struct InsertableAccount {
    pub account_id: Uuid,
    pub balance: Option<BigDecimal>,
    pub currency: String,
    pub account_type: String,
    pub uid: i32,
}

#[derive(Default, AsChangeset, Debug, Deserialize)]
#[table_name = "accounts"]
pub struct UpdateAccount {
    pub account_id: Uuid,
    pub balance: Option<BigDecimal>,
    pub currency: String,
    pub account_type: Option<String>,
    pub uid: Option<i32>,
}

impl Account {
    pub fn get_by_account_id(conn: &diesel::PgConnection, account_id: Uuid) -> Result<Self, DieselError> {
        accounts::dsl::accounts
            .filter(accounts::account_id.eq(account_id))
            .first::<Self>(conn)
    }

    /// TODO: TECH DEBT.
    pub fn get_all_not_in(conn: &diesel::PgConnection, account_ids: &[Uuid]) -> Result<Vec<Self>, DieselError> {
        accounts::dsl::accounts
            .filter(accounts::account_id.ne_all(account_ids))
            .load::<Self>(conn)
    }
}

impl InsertableAccount {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<Uuid, DieselError> {
        diesel::insert_into(accounts::table)
            .values(self)
            .returning(accounts::account_id)
            .get_result(conn)
    }
}

impl UpdateAccount {
    pub fn update(&self, conn: &diesel::PgConnection, account_id: Uuid) -> Result<usize, DieselError> {
        diesel::update(accounts::dsl::accounts.filter(accounts::account_id.eq(account_id)))
            .set(self)
            .execute(conn)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    const fn test_update_accounts() {}
}
