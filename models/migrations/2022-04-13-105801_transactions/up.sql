-- Your SQL goes here
CREATE TABLE transactions (
txid TEXT NOT NULL PRIMARY KEY,
created_at BIGINT NOT NULL,
outbound_amount decimal NOT NULL DEFAULT 0,
inbound_amount decimal NOT NULL DEFAULT 0,
outbound_account_id uuid NOT NULL,
inbound_account_id uuid NOT NULL,
outbound_uid integer NOT NULL,
inbound_uid integer NOT NULL,
outbound_currency TEXT NOT NULL,
inbound_currency TEXT NOT NULL,
exchange_rate decimal NOT NULL DEFAULT 0,
tx_type TEXT NOT NULL
);
