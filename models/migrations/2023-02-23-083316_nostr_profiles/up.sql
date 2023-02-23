-- Your SQL goes here
CREATE TABLE IF NOT EXISTS nostr_profile_records(
    pubkey TEXT PRIMARY KEY,
    created_at BIGINT NOT NULL,
    received_at BIGINT NOT NULL,
    name TEXT,
    display_name TEXT,
    nip05 TEXT,
    lud06 TEXT,
    lud16 TEXT,
    nip05_verified BOOLEAN
);