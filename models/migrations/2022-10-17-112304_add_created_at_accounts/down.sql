-- This file should undo anything in `up.sql`
ALTER TABLE accounts DROP COLUMN IF EXISTS created_at;