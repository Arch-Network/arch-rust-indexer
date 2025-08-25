-- Remove program trigger/function; rely on Rust indexer as source of truth
-- Safe to run multiple times

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'transaction_programs_trigger'
    ) THEN
        EXECUTE 'DROP TRIGGER transaction_programs_trigger ON transactions';
    END IF;
EXCEPTION WHEN undefined_table THEN
    -- Table might not exist yet in some envs
    NULL;
END $$;

-- Drop trigger function if it exists
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_proc WHERE proname = 'update_transaction_programs'
    ) THEN
        EXECUTE 'DROP FUNCTION update_transaction_programs()';
    END IF;
EXCEPTION WHEN undefined_function THEN
    NULL;
END $$;
