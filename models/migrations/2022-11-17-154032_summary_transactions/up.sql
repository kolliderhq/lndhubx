-- Your SQL goes here
CREATE TABLE summary_transactions (
txid TEXT NOT NULL PRIMARY KEY,
fee_txid TEXT,
outbound_txid TEXT,
inbound_txid TEXT,
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
tx_type TEXT NOT NULL,
fees decimal NOT NULL DEFAULT 0
);
