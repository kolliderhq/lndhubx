-- Your SQL goes here
CREATE TABLE accounts (
account_id uuid PRIMARY KEY,
balance decimal NOT NULL DEFAULT 0,
currency TEXT NOT NULL,
account_type TEXT NOT NULL,
uid integer NOT NULL,
CONSTRAINT fk_id
FOREIGN KEY (uid)
REFERENCES users(uid)
)
