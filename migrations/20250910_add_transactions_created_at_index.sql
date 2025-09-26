-- Add performant index to accelerate time-window filters on transactions.created_at
-- Use CONCURRENTLY to avoid long table locks in production

DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE c.relname = 'idx_transactions_created_at' AND n.nspname = 'public'
    ) THEN
        EXECUTE 'CREATE INDEX CONCURRENTLY idx_transactions_created_at ON transactions (created_at DESC)';
    END IF;
EXCEPTION WHEN undefined_table THEN
    -- Table may not exist yet on fresh environments; skip gracefully
    RAISE NOTICE 'transactions table not found; skipping created_at index creation.';
END $$;

