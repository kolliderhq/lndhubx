-- This file should undo anything in `up.sql`
ALTER TABLE nostr_profile_records DROP COLUMN content;
ALTER TABLE nostr_profile_records ADD COLUMN lud06 TEXT;
