-- Drop existing trigger and function
DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
DROP FUNCTION IF EXISTS update_transaction_programs CASCADE;

-- Create or replace the normalize_program_id function with improved handling
CREATE OR REPLACE FUNCTION normalize_program_id(input_id TEXT)
RETURNS TEXT AS $$
BEGIN
    -- If it's NULL or empty, return NULL
    IF input_id IS NULL OR input_id = '' THEN
        RETURN NULL;
    END IF;

    -- If already a valid hex string, return it
    IF input_id ~ '^[0-9a-f]{2,}$' THEN
        RETURN input_id;
    END IF;
    
    -- Try base58 decode first
    BEGIN
        RETURN encode(decode_base58(input_id), 'hex');
    EXCEPTION WHEN OTHERS THEN
        -- If base58 fails, try byte array format
        IF input_id LIKE '[%]' THEN
            RETURN encode(
                decode(
                    string_agg(
                        lpad(
                            CASE 
                                WHEN num::int < 0 THEN (num::int + 256)::text
                                ELSE num::int::text
                            END,
                            2, '0'
                        ),
                        ''
                    ),
                    'hex'
                ),
                'hex'
            ) FROM regexp_split_to_table(
                trim(both '[]' from input_id), 
                ',\s*'
            ) AS num;
        END IF;
    END;
    
    RETURN NULL;
END;
$$ LANGUAGE plpgsql IMMUTABLE STRICT;

-- Create updated trigger function with improved error handling
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
            BEGIN
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
            EXCEPTION WHEN OTHERS THEN
                RAISE WARNING 'Error processing program_id % for transaction %: %', 
                    program_id, NEW.txid, SQLERRM;
            END;
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

-- Create index for program_id pattern matching if it doesn't exist
CREATE INDEX IF NOT EXISTS idx_programs_program_id_pattern 
ON programs (program_id text_pattern_ops);