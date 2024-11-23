ALTER TABLE transactions
ALTER COLUMN status TYPE JSONB USING to_jsonb(status::text);