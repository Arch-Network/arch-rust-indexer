-- Drop existing trigger
DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
DROP FUNCTION IF EXISTS update_transaction_programs CASCADE;

-- Create updated function with logging
CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
BEGIN
    RAISE NOTICE 'Processing trigger for transaction: %', NEW.txid;
    
    BEGIN
        -- Insert program IDs from the transaction data
        WITH program_ids AS (
            SELECT DISTINCT jsonb_array_elements(
                CASE 
                    WHEN jsonb_typeof(NEW.data#>'{message,instructions}') = 'array' 
                    THEN NEW.data#>'{message,instructions}'
                    ELSE '[]'::jsonb
                END
            )->>'program_id' as pid
            WHERE jsonb_typeof(NEW.data#>'{message,instructions}') = 'array'
        )
        INSERT INTO programs (program_id)
        SELECT pid FROM program_ids WHERE pid IS NOT NULL
        ON CONFLICT (program_id) 
        DO UPDATE SET 
            last_seen_at = CURRENT_TIMESTAMP,
            transaction_count = programs.transaction_count + 1;
    EXCEPTION WHEN OTHERS THEN
        RAISE WARNING 'Error updating programs for transaction %: %', NEW.txid, SQLERRM;
    END;

    BEGIN
        -- Link programs to transaction
        WITH program_ids AS (
            SELECT DISTINCT jsonb_array_elements(
                CASE 
                    WHEN jsonb_typeof(NEW.data#>'{message,instructions}') = 'array' 
                    THEN NEW.data#>'{message,instructions}'
                    ELSE '[]'::jsonb
                END
            )->>'program_id' as pid
            WHERE jsonb_typeof(NEW.data#>'{message,instructions}') = 'array'
        )
        INSERT INTO transaction_programs (txid, program_id)
        SELECT NEW.txid, pid
        FROM program_ids
        WHERE pid IS NOT NULL
        ON CONFLICT DO NOTHING;
    EXCEPTION WHEN OTHERS THEN
        RAISE WARNING 'Error linking programs for transaction %: %', NEW.txid, SQLERRM;
    END;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger
CREATE TRIGGER transaction_programs_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_transaction_programs();