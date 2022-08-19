use crate::schema::internal_user_mappings;
use diesel::prelude::*;
use diesel::result::Error as DieselError;

#[derive(Queryable, Identifiable, Debug)]
#[primary_key(username)]
pub struct InternalUserMapping {
    pub username: String,
    pub uid: i32,
}

impl InternalUserMapping {
    pub fn get_all_not_in(conn: &diesel::PgConnection, username: &[String]) -> Result<Vec<Self>, DieselError> {
        internal_user_mappings::dsl::internal_user_mappings
            .filter(internal_user_mappings::username.ne_all(username))
            .load::<Self>(conn)
    }
}
