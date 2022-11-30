CREATE TABLE ln_addresses (
id SERIAL NOT NULL PRIMARY KEY,
created_at TIMESTAMP default now(),
username TEXT NOT NULL UNIQUE,
domain TEXT NOT NULL
);