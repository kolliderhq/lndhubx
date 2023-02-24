use crate::schema::nostr_profile_records;
use diesel::{BoolExpressionMethods, ExpressionMethods, PgTextExpressionMethods, QueryDsl, QueryResult, RunQueryDsl};

#[derive(Queryable, Identifiable, Insertable, AsChangeset, Debug)]
#[changeset_options(treat_none_as_null = "true")]
#[primary_key(pubkey)]
pub struct NostrProfileRecord {
    pub pubkey: String,
    pub created_at: i64,
    pub received_at: i64,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub nip05: Option<String>,
    pub lud06: Option<String>,
    pub lud16: Option<String>,
    pub nip05_verified: Option<bool>,
}

impl NostrProfileRecord {
    pub fn upsert(&self, conn: &diesel::PgConnection) -> QueryResult<usize> {
        diesel::insert_into(nostr_profile_records::dsl::nostr_profile_records)
            .values(self)
            .on_conflict(nostr_profile_records::dsl::pubkey)
            .do_update()
            .set(self)
            .execute(conn)
    }

    pub fn update_nip05_verified(
        conn: &diesel::PgConnection,
        pubkey: &str,
        nip05: &str,
        nip05_verified: Option<bool>,
    ) -> QueryResult<usize> {
        diesel::update(nostr_profile_records::dsl::nostr_profile_records)
            .filter(nostr_profile_records::dsl::pubkey.eq(pubkey))
            .filter(nostr_profile_records::dsl::nip05.eq(nip05))
            .set(nostr_profile_records::dsl::nip05_verified.eq(nip05_verified))
            .execute(conn)
    }

    pub fn search_by_text(conn: &diesel::PgConnection, text: &str) -> QueryResult<Vec<Self>> {
        let escaped_lowered = text.replace('%', "\\%").replace('_', "\\_").to_lowercase();
        let name_pattern = format!("%{escaped_lowered}%");
        let local_part_pattern = format!("{name_pattern}@%");
        let relevant_search = nostr_profile_records::dsl::name
            .ilike(&name_pattern)
            .or(nostr_profile_records::dsl::display_name.ilike(&name_pattern))
            .or(nostr_profile_records::dsl::nip05.ilike(&local_part_pattern))
            .or(nostr_profile_records::dsl::lud16.ilike(&local_part_pattern));
        nostr_profile_records::dsl::nostr_profile_records
            .filter(relevant_search)
            .load::<Self>(conn)
    }
}
