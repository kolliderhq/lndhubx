-- Your SQL goes here
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_internal BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE accounts ALTER COLUMN account_id SET DEFAULT gen_random_uuid();

UPDATE users set is_internal = FALSE WHERE uid != 23193913;

INSERT INTO users(uid, username, password, is_internal) VALUES
    (52172712, 'dealer', 'arbitrary-string-no-one-can-find-out-about', TRUE);

INSERT INTO accounts(uid, currency, account_type) VALUES
    (52172712, 'BTC', 'Internal');