table! {
    accounts (account_id) {
        account_id -> Uuid,
        balance -> Numeric,
        currency -> Text,
        account_type -> Text,
        uid -> Int4,
    }
}

table! {
    api_tokens (id) {
        id -> Int4,
        name -> Text,
        key -> Nullable<Text>,
        uid -> Int4,
        revoked -> Nullable<Bool>,
    }
}

table! {
    clients (id) {
        id -> Int4,
        name -> Text,
    }
}

table! {
    internal_user_mappings (username) {
        username -> Text,
        uid -> Int4,
    }
}

table! {
    invoices (payment_request) {
        payment_request -> Text,
        rhash -> Text,
        payment_hash -> Text,
        created_at -> Int8,
        value -> Int8,
        value_msat -> Int8,
        expiry -> Int8,
        settled -> Bool,
        add_index -> Int8,
        settled_date -> Int8,
        account_id -> Text,
        uid -> Int4,
        incoming -> Bool,
        owner -> Nullable<Int4>,
        fees -> Nullable<Int8>,
        currency -> Nullable<Text>,
    }
}

table! {
    ln_challenge_maps (challenge) {
        challenge -> Text,
        uid -> Int4,
    }
}

table! {
    ln_users (pub_key) {
        created_at -> Nullable<Timestamp>,
        pub_key -> Text,
        uid -> Int4,
        prevent_delete -> Bool,
        alias -> Nullable<Text>,
    }
}

table! {
    pre_signups (uid) {
        uid -> Int4,
        created_at -> Nullable<Timestamp>,
        email -> Text,
    }
}

table! {
    transactions (txid) {
        txid -> Text,
        created_at -> Int8,
        outbound_amount -> Numeric,
        inbound_amount -> Numeric,
        outbound_account_id -> Uuid,
        inbound_account_id -> Uuid,
        outbound_uid -> Int4,
        inbound_uid -> Int4,
        outbound_currency -> Text,
        inbound_currency -> Text,
        exchange_rate -> Numeric,
        tx_type -> Text,
    }
}

table! {
    user_types (id) {
        id -> Int4,
        name -> Text,
    }
}

table! {
    users (uid) {
        uid -> Int4,
        created_at -> Nullable<Timestamp>,
        username -> Text,
        password -> Text,
    }
}

joinable!(accounts -> users (uid));
joinable!(internal_user_mappings -> users (uid));

allow_tables_to_appear_in_same_query!(
    accounts,
    api_tokens,
    clients,
    internal_user_mappings,
    invoices,
    ln_challenge_maps,
    ln_users,
    pre_signups,
    transactions,
    user_types,
    users,
);
