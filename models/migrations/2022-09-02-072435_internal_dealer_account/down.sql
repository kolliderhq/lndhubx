-- This file should undo anything in `up.sql`
DELETE FROM accounts WHERE uid = 52172712;
DELETE FROM users WHERE uid = 52172712;
ALTER TABLE accounts ALTER COLUMN account_id DROP DEFAULT;
ALTER TABLE users DROP COLUMN IF EXISTS is_internal;