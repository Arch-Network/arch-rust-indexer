-- migrations/20241205170000_fix_program_id_duplication.sql
DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
DROP FUNCTION IF EXISTS update_transaction_programs CASCADE;

CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
DECLARE
    inst jsonb;
    program_id text;
BEGIN
    -- Process each instruction
    FOR inst IN SELECT * FROM jsonb_array_elements(
        CASE 
            WHEN jsonb_typeof(NEW.data#>'{message,instructions}') = 'array' 
            THEN NEW.data#>'{message,instructions}'
            ELSE '[]'::jsonb
        END
    )
    LOOP
        -- Extract and normalize program_id based on type
        program_id := CASE
            WHEN jsonb_typeof(inst->'program_id') = 'string' THEN
                normalize_program_id(inst->>'program_id')
            WHEN jsonb_typeof(inst->'program_id') = 'array' THEN
                normalize_program_id(inst->'program_id'::text)
            ELSE NULL
        END;

        -- Insert into programs if we got a valid program_id
        IF program_id IS NOT NULL THEN
            INSERT INTO programs (program_id)
            VALUES (program_id)
            ON CONFLICT (program_id) 
            DO UPDATE SET 
                last_seen_at = CURRENT_TIMESTAMP,
                transaction_count = programs.transaction_count + 1;

            -- Link program to transaction
            INSERT INTO transaction_programs (txid, program_id)
            VALUES (NEW.txid, program_id)
            ON CONFLICT DO NOTHING;
        END IF;
    END LOOP;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger
CREATE TRIGGER transaction_programs_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_transaction_programs();