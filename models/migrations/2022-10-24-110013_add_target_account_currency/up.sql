-- Your SQL goes here
ALTER TABLE invoices ADD COLUMN IF NOT EXISTS target_account_currency TEXT;