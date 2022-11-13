-- Your SQL goes here
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS fees decimal NOT NULL DEFAULT 0;