-- Your SQL goes here
ALTER TABLE accounts RENAME CONSTRAINT fk_id TO fk_accounts_uid_users_uid;
ALTER TABLE nostr_public_keys RENAME CONSTRAINT fk_id TO fk_nostr_public_keys_uid_users_uid;
ALTER TABLE user_profiles RENAME CONSTRAINT fk_id TO fk_user_profiles_uid_users_uid;