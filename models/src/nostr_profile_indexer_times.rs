use crate::schema::nostr_profile_indexer_times;
use diesel::{ExpressionMethods, QueryResult, RunQueryDsl};

#[derive(Queryable, Identifiable, Debug)]
pub struct NostrProfileIndexerTime {
    pub id: i32,
    pub last_check: Option<i64>,
}

impl NostrProfileIndexerTime {
    pub fn get_last_check(conn: &diesel::PgConnection) -> QueryResult<Option<i64>> {
        nostr_profile_indexer_times::dsl::nostr_profile_indexer_times
            .first::<Self>(conn)
            .map(|indexer_time| indexer_time.last_check)
    }

    pub fn set_last_check(conn: &diesel::PgConnection, last_check: Option<i64>) -> QueryResult<usize> {
        diesel::update(nostr_profile_indexer_times::dsl::nostr_profile_indexer_times)
            .set(nostr_profile_indexer_times::dsl::last_check.eq(last_check))
            .execute(conn)
    }
}
