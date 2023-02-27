-- Your SQL goes here
ALTER TABLE nostr_profile_records DROP COLUMN lud06;
ALTER TABLE nostr_profile_records ADD COLUMN content TEXT NOT NULL DEFAULT '';