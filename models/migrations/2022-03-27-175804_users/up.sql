-- Your SQL goes here
CREATE TABLE users (
uid SERIAL NOT NULL PRIMARY KEY,
created_at TIMESTAMP default now(),
username TEXT NOT NULL UNIQUE,
password TEXT NOT NULL
);

INSERT INTO users(uid, username, password) VALUES
(23193913, 'bank', 'arbitrary-string-no-one-can-find-out-about');
