-- Your SQL goes here
CREATE TABLE nostr_profile_indexer_times(id SERIAL PRIMARY KEY, last_check BIGINT);
CREATE UNIQUE INDEX nostr_profile_indexer_times_one_row ON nostr_profile_indexer_times ((TRUE));
INSERT INTO nostr_profile_indexer_times(last_check) VALUES (NULL);