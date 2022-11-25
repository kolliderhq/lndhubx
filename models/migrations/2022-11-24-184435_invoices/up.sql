CREATE TABLE invoices (
payment_hash TEXT NOT NULL PRIMARY KEY,
payment_request TEXT NOT NULL,
created_at BIGINT NOT NULL DEFAULT 0,
"value" BIGINT NOT NULL DEFAULT 0,
"value_msat" BIGINT NOT NULL DEFAULT 0,
expiry BIGINT NOT NULL DEFAULT 0,
settled BOOLEAN NOT NULL,
add_index BIGINT NOT NULL DEFAULT 0,
settled_date BIGINT NOT NULL DEFAULT 0,
account_id TEXT NOT NULL,
uid integer NOT NULL DEFAULT 0 references "users" (uid),
incoming BOOLEAN NOT NULL,
owner integer references "users" (uid),
fees BIGINT,
currency TEXT,
target_account_currency TEXT,
reference TEXT
);