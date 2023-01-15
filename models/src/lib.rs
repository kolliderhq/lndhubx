#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

pub mod accounts;
pub mod conversions;
mod error;
pub mod internal_user_mappings;
pub mod invoices;
pub mod pre_signups;
mod schema;
pub mod transactions;
pub mod summary_transactions;
pub mod users;
pub mod ln_addresses;
pub mod deezy_stuff;
pub mod nostr_public_keys;
pub mod user_profiles;

cfg_if::cfg_if! {
    if #[cfg(debug_assertions)] {
        embed_migrations!("./migrations");

        /// Our init function must be called once at the startup of any program using this crate
        /// This function runs different migrations and health checks.
        pub fn init(conn: &diesel::PgConnection) -> Result<(), error::GeneralError> {
            Ok(embedded_migrations::run(conn)?)
        }
    } else {
        embed_migrations!("./migrations");

        /// Placeholder init for production envs.
        pub fn init(conn: &diesel::PgConnection) -> Result<(), error::GeneralError> {
            Ok(embedded_migrations::run(conn)?)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
