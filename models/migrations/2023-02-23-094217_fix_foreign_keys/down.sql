-- This file should undo anything in `up.sql`
ALTER TABLE accounts RENAME CONSTRAINT fk_accounts_uid_users_uid TO fk_id;
ALTER TABLE nostr_public_keys RENAME CONSTRAINT fk_nostr_public_keys_uid_users_uid TO fk_id;
ALTER TABLE user_profiles RENAME CONSTRAINT fk_user_profiles_uid_users_uid TO fk_id;