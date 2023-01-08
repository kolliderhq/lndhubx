CREATE TABLE nostr_public_keys (
created_at TIMESTAMP default now(),
pubkey TEXT PRIMARY KEY,
uid integer NOT NULL UNIQUE,
CONSTRAINT fk_id
FOREIGN KEY (uid)
REFERENCES users(uid)
);