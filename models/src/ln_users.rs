use crate::schema::ln_challenge_maps;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Identifiable, Debug, Serialize, Clone)]
#[primary_key(challenge)]
pub struct LnChallengeMap {
    pub challenge: String,
    pub uid: i32,
}

impl LnChallengeMap {
    pub fn get_by_challenge(conn: &diesel::PgConnection, challenge: String) -> Result<Self, DieselError> {
        ln_challenge_maps::dsl::ln_challenge_maps
            .filter(ln_challenge_maps::challenge.eq(challenge))
            .first::<Self>(conn)
    }
}

#[derive(Insertable, Debug, Deserialize)]
#[table_name = "ln_challenge_maps"]
pub struct InsertableLnChallengeMap {
    pub challenge: String,
    pub uid: i32,
}

impl InsertableLnChallengeMap {
    pub fn insert(&self, conn: &diesel::PgConnection) -> Result<String, DieselError> {
        diesel::insert_into(ln_challenge_maps::table)
            .values(self)
            .returning(ln_challenge_maps::challenge)
            .get_result(conn)
    }
}