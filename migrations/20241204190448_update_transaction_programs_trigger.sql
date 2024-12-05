-- First drop any existing triggers and functions
DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
DROP TRIGGER IF EXISTS update_transaction_programs_trigger ON transactions;
DROP FUNCTION IF EXISTS update_transaction_programs CASCADE;

-- Create the updated function
CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
BEGIN
    -- Insert program IDs from the transaction data
    WITH program_ids AS (
        SELECT DISTINCT jsonb_array_elements(
            CASE 
                WHEN jsonb_typeof(NEW.data->'message'->'instructions') = 'array' 
                THEN NEW.data->'message'->'instructions'
                ELSE '[]'::jsonb
            END
        )->>'program_id' as pid
        WHERE jsonb_typeof(NEW.data->'message'->'instructions') = 'array'
    )
    INSERT INTO programs (program_id)
    SELECT pid FROM program_ids WHERE pid IS NOT NULL
    ON CONFLICT (program_id) 
    DO UPDATE SET 
        last_seen_at = CURRENT_TIMESTAMP,
        transaction_count = programs.transaction_count + 1;

    -- Link programs to transaction using the same CTE definition
    WITH program_ids AS (
        SELECT DISTINCT jsonb_array_elements(
            CASE 
                WHEN jsonb_typeof(NEW.data->'message'->'instructions') = 'array' 
                THEN NEW.data->'message'->'instructions'
                ELSE '[]'::jsonb
            END
        )->>'program_id' as pid
        WHERE jsonb_typeof(NEW.data->'message'->'instructions') = 'array'
    )
    INSERT INTO transaction_programs (txid, program_id)
    SELECT NEW.txid, pid
    FROM program_ids
    WHERE pid IS NOT NULL;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create the trigger
CREATE TRIGGER transaction_programs_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_transaction_programs();