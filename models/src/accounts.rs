use crate::schema::accounts;
use crate::schema::users;
use bigdecimal::BigDecimal;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use std::default::Default;
use std::str::FromStr;
use uuid::Uuid;

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

    pub fn get_non_internal_users_accounts(conn: &diesel::PgConnection) -> Result<Vec<Self>, DieselError> {
        users::dsl::users
            .inner_join(accounts::dsl::accounts)
            .select((
                accounts::account_id,
                accounts::balance,
                accounts::currency,
                accounts::account_type,
                accounts::uid,
            ))
            .filter(users::is_internal.eq(false))
            .load::<Self>(conn)
    }

    pub fn get_dealer_btc_accounts(conn: &diesel::PgConnection) -> Result<Vec<Self>, DieselError> {
        users::dsl::users
            .inner_join(accounts::dsl::accounts)
            .select((
                accounts::account_id,
                accounts::balance,
                accounts::currency,
                accounts::account_type,
                accounts::uid,
            ))
            .filter(users::uid.eq(52172712))
            .filter(users::is_internal.eq(true))
            .filter(users::username.eq("dealer"))
            .filter(accounts::account_type.eq("Internal"))
            .filter(accounts::currency.eq("BTC"))
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
