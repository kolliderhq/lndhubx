CREATE TABLE deezy_secret_keys(
secret_key TEXT NOT NULL PRIMARY KEY,
created_at TIMESTAMP default now(),
uid integer NOT NULL
);