-- Drop existing trigger and function
DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
DROP FUNCTION IF EXISTS update_transaction_programs CASCADE;

-- Add performance optimization indexes
CREATE INDEX IF NOT EXISTS idx_programs_last_seen_at ON programs (last_seen_at);
CREATE INDEX IF NOT EXISTS idx_programs_transaction_count ON programs (transaction_count);

-- Create optimized trigger function with batch processing
CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
DECLARE
    inst jsonb;
    program_id text;
BEGIN
    -- Create temporary table for batch processing
    CREATE TEMP TABLE IF NOT EXISTS temp_programs (
        program_id text PRIMARY KEY
    ) ON COMMIT DROP;

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
                normalize_program_id(
                    array_to_string(
                        ARRAY(
                            SELECT jsonb_array_elements_text(inst->'program_id')
                        ),
                        ','
                    )
                )
            ELSE NULL
        END;

        -- Insert into temp table if valid
        IF program_id IS NOT NULL THEN
            BEGIN
                INSERT INTO temp_programs (program_id)
                VALUES (program_id)
                ON CONFLICT DO NOTHING;
            EXCEPTION WHEN OTHERS THEN
                RAISE WARNING 'Error processing program_id % for transaction %: %', 
                    program_id, NEW.txid, SQLERRM;
                CONTINUE;
            END;
        END IF;
    END LOOP;

    -- Batch update programs table
    BEGIN
        INSERT INTO programs (program_id)
        SELECT program_id FROM temp_programs
        ON CONFLICT (program_id) DO UPDATE SET
            last_seen_at = CURRENT_TIMESTAMP,
            transaction_count = programs.transaction_count + 1;
    EXCEPTION WHEN OTHERS THEN
        RAISE WARNING 'Error updating programs for transaction %: %', NEW.txid, SQLERRM;
    END;

    -- Batch insert into transaction_programs
    BEGIN
        INSERT INTO transaction_programs (txid, program_id)
        SELECT NEW.txid, program_id FROM temp_programs
        ON CONFLICT DO NOTHING;
    EXCEPTION WHEN OTHERS THEN
        RAISE WARNING 'Error linking programs to transaction %: %', NEW.txid, SQLERRM;
    END;

    -- Drop temporary table (will happen automatically on commit, but being explicit)
    DROP TABLE IF EXISTS temp_programs;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger
CREATE TRIGGER transaction_programs_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_transaction_programs();

-- Update statistics
ANALYZE programs;
ANALYZE transaction_programs;