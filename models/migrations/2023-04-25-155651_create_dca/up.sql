CREATE TABLE dca_settings (
	id SERIAL NOT NULL PRIMARY KEY,
	uid integer references "users" (uid) UNIQUE NOT NULL,
	interval TEXT NOT NULL,
	amount decimal NOT NULL DEFAULT 0,
	from_currency TEXT NOT NULL,
	to_currency TEXT NOT NULL
);
