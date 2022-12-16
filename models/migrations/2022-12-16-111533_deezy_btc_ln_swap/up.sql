CREATE TABLE deezy_btc_ln_swaps(
id SERIAL NOT NULL PRIMARY KEY,
created_at TIMESTAMP default now(),
uid integer NOT NULL,
ln_address TEXT NOT NULL,
secret_access_key TEXT NOT NULL,
btc_address TEXT NOT NULL,
sig TEXT NOT NULL,
webhook_url TEXT
);