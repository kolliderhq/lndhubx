-- Your SQL goes here
CREATE TABLE bitcoin_addresses (
    address TEXT PRIMARY KEY,
    uid INT NOT NULL,
    CONSTRAINT btc_addr_fk_id FOREIGN KEY (uid) REFERENCES users(uid)
);

CREATE TABLE onchain_transactions (
    txid TEXT PRIMARY KEY,
    uid INT NOT NULL,
    timestamp BIGINT NOT NULL,
    address TEXT NOT NULL,
    block_number BIGINT NOT NULL,
    confirmations BIGINT NOT NULL,
    fee BIGINT NOT NULL,
    tx_type TEXT NOT NULL,
    is_confirmed BOOL NOT NULL,
    network TEXT NOT NULL,
    value BIGINT NOT NULL
);