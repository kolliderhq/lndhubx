// @generated automatically by Diesel CLI.

diesel::table! {
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

diesel::table! {
    internal_user_mappings (username) {
        username -> Text,
        uid -> Int4,
    }
}

diesel::table! {
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
        target_account_currency -> Nullable<Text>,
        reference -> Nullable<Text>,
    }
}

diesel::table! {
    pre_signups (uid) {
        uid -> Int4,
        created_at -> Nullable<Timestamp>,
        email -> Text,
    }
}

diesel::table! {
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
    }
}

diesel::table! {
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

diesel::table! {
    users (uid) {
        uid -> Int4,
        created_at -> Nullable<Timestamp>,
        username -> Text,
        password -> Text,
        is_internal -> Bool,
    }
}

diesel::joinable!(accounts -> users (uid));
diesel::joinable!(internal_user_mappings -> users (uid));

diesel::allow_tables_to_appear_in_same_query!(
    accounts,
    internal_user_mappings,
    invoices,
    pre_signups,
    summary_transactions,
    transactions,
    users,
);
