-- Your SQL goes here
CREATE TABLE bitcoin_addresses (
    address TEXT PRIMARY KEY,
    uid INT NOT NULL,
    CONSTRAINT btc_addr_fk_id FOREIGN KEY (uid) REFERENCES users(uid)
);

CREATE TABLE onchain_transactions (
    txid TEXT PRIMARY KEY,
    is_settled BOOL NOT NULL
);