-- Your SQL goes here
CREATE TABLE pre_signups (
uid SERIAL NOT NULL PRIMARY KEY,
created_at TIMESTAMP default now(),
email TEXT NOT NULL UNIQUE
);
