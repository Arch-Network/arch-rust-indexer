-- migrations/20241205170000_fix_trigger_normalization.sql
DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
DROP FUNCTION IF EXISTS update_transaction_programs CASCADE;

-- Create updated function with proper handling
CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
BEGIN
    -- Insert program IDs from the transaction data
    WITH RECURSIVE extracted_programs AS (
        SELECT DISTINCT 
            normalize_program_id(
                jsonb_array_elements(
                    CASE 
                        WHEN jsonb_typeof(NEW.data#>'{message,instructions}') = 'array' 
                        THEN NEW.data#>'{message,instructions}'
                        ELSE '[]'::jsonb
                    END
                )->>'program_id'
            ) as program_id
        WHERE jsonb_typeof(NEW.data#>'{message,instructions}') = 'array'
    )
    INSERT INTO programs (program_id)
    SELECT program_id 
    FROM extracted_programs 
    WHERE program_id IS NOT NULL
    ON CONFLICT (program_id) 
    DO UPDATE SET 
        last_seen_at = CURRENT_TIMESTAMP,
        transaction_count = programs.transaction_count + 1;

    -- Link programs to transaction
    WITH extracted_programs AS (
        SELECT DISTINCT 
            normalize_program_id(
                jsonb_array_elements(
                    CASE 
                        WHEN jsonb_typeof(NEW.data#>'{message,instructions}') = 'array' 
                        THEN NEW.data#>'{message,instructions}'
                        ELSE '[]'::jsonb
                    END
                )->>'program_id'
            ) as program_id
        WHERE jsonb_typeof(NEW.data#>'{message,instructions}') = 'array'
    )
    INSERT INTO transaction_programs (txid, program_id)
    SELECT NEW.txid, program_id
    FROM extracted_programs
    WHERE program_id IS NOT NULL
    ON CONFLICT DO NOTHING;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger
CREATE TRIGGER transaction_programs_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_transaction_programs();