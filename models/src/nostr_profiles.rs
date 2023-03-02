use crate::schema::nostr_profile_records;
use diesel::{
    BoolExpressionMethods, ExpressionMethods, PgSortExpressionMethods, PgTextExpressionMethods, QueryDsl, QueryResult,
    RunQueryDsl,
};

fn escaped_text(text: &str) -> String {
    text.replace('\'', "''")
}

fn nullable_string(value: &Option<String>) -> String {
    match value {
        Some(text) => format!("'{}'", escaped_text(text)),
        None => String::from("NULL"),
    }
}

fn not_nullable_string(value: &str) -> String {
    format!("'{}'", escaped_text(value))
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
    pub lud16: Option<String>,
    pub nip05_verified: Option<bool>,
    pub content: String,
}

impl NostrProfileRecord {
    pub fn fetch_all(conn: &diesel::PgConnection) -> QueryResult<Vec<Self>> {
        nostr_profile_records::dsl::nostr_profile_records.load(conn)
    }

    pub fn upsert(&self, conn: &diesel::PgConnection) -> QueryResult<usize> {
        // diesel does not allow specifying filtering using a WHERE clause
        // on the update statement after conflict, so using a hand crafted query
        let pubkey = self.pubkey.clone();
        let created_at = self.created_at;
        let received_at = self.received_at;
        let content = not_nullable_string(&self.content);
        let name = nullable_string(&self.name);
        let display_name = nullable_string(&self.display_name);
        let nip05 = nullable_string(&self.nip05);
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
                    lud16, \
                    nip05_verified, \
                    content \
                ) \
                VALUES(\
                    '{pubkey}', \
                    {created_at}, \
                    {received_at}, \
                    {name}, \
                    {display_name}, \
                    {nip05}, \
                    {lud16}, \
                    {nip05_verified}, \
                    {content} \
                ) \
                ON CONFLICT (pubkey) \
                DO UPDATE \
                SET \
                    created_at = EXCLUDED.created_at, \
                    received_at = EXCLUDED.received_at, \
                    name = EXCLUDED.name, \
                    display_name = EXCLUDED.display_name, \
                    nip05 = EXCLUDED.nip05, \
                    lud16 = EXCLUDED.lud16, \
                    nip05_verified = EXCLUDED.nip05_verified, \
                     content = EXCLUDED.content \
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

    pub fn search_by_text(
        conn: &diesel::PgConnection,
        pubkey: Option<String>,
        text: Option<String>,
        limit: Option<u64>,
    ) -> QueryResult<Vec<Self>> {
        if pubkey.is_none() && text.is_none() {
            return Ok(Vec::new());
        }

        let maybe_pubkey_expr = pubkey.map(|pkey| nostr_profile_records::dsl::pubkey.eq(pkey));
        let maybe_text_expr = text.map(|txt| {
            let escaped_lowered = txt.replace('%', "\\%").replace('_', "\\_").to_lowercase();
            let name_pattern = format!("%{escaped_lowered}%");
            let local_part_pattern = format!("{name_pattern}@%");
            nostr_profile_records::dsl::name
                .ilike(name_pattern.clone())
                .or(nostr_profile_records::dsl::display_name.ilike(name_pattern))
                .or(nostr_profile_records::dsl::nip05.ilike(local_part_pattern.clone()))
                .or(nostr_profile_records::dsl::lud16.ilike(local_part_pattern))
        });

        let query = nostr_profile_records::dsl::nostr_profile_records
            .order(nostr_profile_records::dsl::nip05_verified.desc().nulls_last());

        if let Some(pubkey_expr) = maybe_pubkey_expr {
            let query = query.filter(pubkey_expr);
            if let Some(text_expr) = maybe_text_expr {
                let query = query.filter(text_expr);
                let query_result = match limit {
                    Some(num_records) => query.limit(num_records as i64).load(conn),
                    None => query.load(conn),
                };
                return query_result;
            }
            match limit {
                Some(num_records) => query.limit(num_records as i64).load(conn),
                None => query.load(conn),
            }
        } else if let Some(text_expr) = maybe_text_expr {
            let query = query.filter(text_expr);
            match limit {
                Some(num_records) => query.limit(num_records as i64).load(conn),
                None => query.load(conn),
            }
        } else {
            Ok(Vec::new())
        }
    }
}
