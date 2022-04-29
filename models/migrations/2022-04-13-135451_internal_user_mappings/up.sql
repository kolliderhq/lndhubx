-- Your SQL goes here
CREATE TABLE internal_user_mappings (
username TEXT NOT NULL PRIMARY KEY,
uid integer NOT NULL references "users" (uid)
);
