table! {
    accounts (account_id) {
        account_id -> Uuid,
        balance -> Numeric,
        currency -> Text,
        account_type -> Text,
        uid -> Int4,
        created_at -> Int8,
        account_class -> Text,
    }
}

table! {
    dca_settings (id) {
        id -> Int4,
        uid -> Int4,
        interval -> Text,
        amount -> Numeric,
        from_currency -> Text,
        to_currency -> Text,
    }
}

table! {
    deezy_btc_ln_swaps (id) {
        id -> Int4,
        created_at -> Nullable<Timestamp>,
        uid -> Int4,
        ln_address -> Text,
        secret_access_key -> Text,
        btc_address -> Text,
        sig -> Text,
        webhook_url -> Nullable<Text>,
    }
}

table! {
    deezy_secret_keys (secret_key) {
        secret_key -> Text,
        created_at -> Nullable<Timestamp>,
        uid -> Int4,
    }
}

table! {
    internal_user_mappings (username) {
        username -> Text,
        uid -> Int4,
    }
}

table! {
    invoices (payment_hash) {
        payment_hash -> Text,
        payment_request -> Text,
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
        target_account_currency -> Nullable<Text>,
        reference -> Nullable<Text>,
        description -> Nullable<Text>,
    }
}

table! {
    ln_addresses (id) {
        id -> Int4,
        created_at -> Nullable<Timestamp>,
        username -> Text,
        domain -> Text,
    }
}

table! {
    nostr_profile_indexer_times (id) {
        id -> Int4,
        last_check -> Nullable<Int8>,
    }
}

table! {
    nostr_profile_records (pubkey) {
        pubkey -> Text,
        created_at -> Int8,
        received_at -> Int8,
        name -> Nullable<Text>,
        display_name -> Nullable<Text>,
        nip05 -> Nullable<Text>,
        lud16 -> Nullable<Text>,
        nip05_verified -> Nullable<Bool>,
        content -> Text,
        lnurl_pay_req -> Nullable<Text>,
    }
}

table! {
    nostr_public_keys (pubkey) {
        created_at -> Nullable<Timestamp>,
        pubkey -> Text,
        uid -> Int4,
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
    summary_transactions (txid) {
        txid -> Text,
        fee_txid -> Nullable<Text>,
        outbound_txid -> Nullable<Text>,
        inbound_txid -> Nullable<Text>,
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
        fees -> Numeric,
        reference -> Nullable<Text>,
        outbound_username -> Nullable<Text>,
        inbound_username -> Nullable<Text>,
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
        fees -> Numeric,
    }
}

table! {
    user_profiles (uid) {
        uid -> Int4,
        email -> Nullable<Text>,
        nostr_notifications -> Nullable<Bool>,
        email_notifications -> Nullable<Bool>,
        img_url -> Nullable<Text>,
        twitter_handle -> Nullable<Text>,
        is_twitter_verified -> Nullable<Bool>,
        is_email_verified -> Nullable<Bool>,
    }
}

table! {
    users (uid) {
        uid -> Int4,
        created_at -> Nullable<Timestamp>,
        username -> Text,
        password -> Text,
        is_internal -> Bool,
    }
}

joinable!(accounts -> users (uid));
joinable!(dca_settings -> users (uid));
joinable!(internal_user_mappings -> users (uid));
joinable!(nostr_public_keys -> users (uid));
joinable!(user_profiles -> users (uid));

allow_tables_to_appear_in_same_query!(
    accounts,
    dca_settings,
    deezy_btc_ln_swaps,
    deezy_secret_keys,
    internal_user_mappings,
    invoices,
    ln_addresses,
    nostr_profile_indexer_times,
    nostr_profile_records,
    nostr_public_keys,
    pre_signups,
    summary_transactions,
    transactions,
    user_profiles,
    users,
);
