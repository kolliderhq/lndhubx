ALTER TABLE invoices DROP CONSTRAINT invoices_pkey;
CREATE UNIQUE INDEX invoices_pkey ON invoices USING btree (payment_hash);
ALTER TABLE invoices ADD CONSTRAINT invoices_pkey PRIMARY KEY USING INDEX invoices_pkey;
