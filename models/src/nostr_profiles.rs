use crate::schema::nostr_profile_records;
use diesel::{BoolExpressionMethods, ExpressionMethods, PgTextExpressionMethods, QueryDsl, QueryResult, RunQueryDsl};

fn nullable_string(value: &Option<String>) -> String {
    match value {
        Some(text) => format!("'{}'", text.replace('\'', "''")),
        None => String::from("NULL"),
    }
}

fn nullable_bool(value: &Option<bool>) -> String {
    match value {
        Some(v) => format!("{v}"),
        None => String::from("NULL"),
    }
}

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
        // diesel::insert_into(nostr_profile_records::dsl::nostr_profile_records)
        //     .values(self)
        //     .on_conflict(nostr_profile_records::dsl::pubkey)
        //     .do_update()
        //     .set(self)
        //     .filter(nostr_profile_records::dsl::created_at.lt(self.created_at))
        //     .execute(conn)

        // the above does not work because diesel does not allow specifying filtering
        // that is where clause on the update statement after conflict
        let pubkey = self.pubkey.clone();
        let created_at = self.created_at;
        let received_at = self.received_at;
        let name = nullable_string(&self.name);
        let display_name = nullable_string(&self.display_name);
        let nip05 = nullable_string(&self.nip05);
        let lud06 = nullable_string(&self.lud06);
        let lud16 = nullable_string(&self.lud16);
        let nip05_verified = nullable_bool(&self.nip05_verified);
        let upsert_query = format!(
            "\
                INSERT INTO nostr_profile_records AS n (\
                    pubkey, \
                    created_at, \
                    received_at, \
                    name, \
                    display_name, \
                    nip05, \
                    lud06, \
                    lud16, \
                    nip05_verified\
                ) \
                VALUES(\
                    '{pubkey}', \
                    {created_at}, \
                    {received_at}, \
                    {name}, \
                    {display_name}, \
                    {nip05}, \
                    {lud06}, \
                    {lud16}, \
                    {nip05_verified}\
                ) \
                ON CONFLICT (pubkey) \
                DO UPDATE \
                SET \
                    created_at = EXCLUDED.created_at, \
                    received_at = EXCLUDED.received_at, \
                    name = EXCLUDED.name, \
                    display_name = EXCLUDED.display_name, \
                    nip05 = EXCLUDED.nip05, \
                    lud06 = EXCLUDED.lud06, \
                    lud16 = EXCLUDED.lud16, \
                    nip05_verified = EXCLUDED.nip05_verified \
                WHERE \
                    n.created_at < EXCLUDED.created_at OR \
                    (n.created_at = EXCLUDED.created_at \
                        AND (COALESCE(n.nip05_verified, FALSE) != COALESCE(EXCLUDED.nip05_verified, FALSE)))
            "
        );
        diesel::sql_query(upsert_query).execute(conn)
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
