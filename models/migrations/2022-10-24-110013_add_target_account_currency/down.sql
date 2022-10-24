-- This file should undo anything in `up.sql`
ALTER TABLE invoices DROP COLUMN IF EXISTS target_account_currency;