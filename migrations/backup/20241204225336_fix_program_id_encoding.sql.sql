-- First drop existing trigger
DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
DROP FUNCTION IF EXISTS update_transaction_programs CASCADE;

-- Create updated function with proper byte array handling
CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
BEGIN
    WITH RECURSIVE program_ids AS (
        SELECT DISTINCT 
            CASE 
                WHEN jsonb_typeof(inst.value->'program_id') = 'string' 
                    THEN inst.value->>'program_id'
                WHEN jsonb_typeof(inst.value->'program_id') = 'array' THEN 
                    encode(
                        decode(string_agg(
                            lpad(
                                CASE 
                                    WHEN (v::text)::int < 0 THEN ((v::text)::int + 256)::text
                                    ELSE (v::text)::int::text
                                END,
                                2, '0'
                            ),
                            ''
                        ), 'hex')::bytea,
                        'hex'
                    )
                ELSE NULL
            END as pid
        FROM jsonb_array_elements(
            CASE 
                WHEN jsonb_typeof(NEW.data->'message'->'instructions') = 'array' 
                THEN NEW.data->'message'->'instructions'
                ELSE '[]'::jsonb
            END
        ) inst,
        LATERAL jsonb_array_elements_text(inst.value->'program_id') v
        WHERE inst.value->>'program_id' IS NOT NULL
    )
    INSERT INTO programs (program_id)
    SELECT pid FROM program_ids 
    WHERE pid IS NOT NULL
    ON CONFLICT (program_id) 
    DO UPDATE SET 
        last_seen_at = CURRENT_TIMESTAMP,
        transaction_count = programs.transaction_count + 1;

    -- Link programs to transaction
    INSERT INTO transaction_programs (txid, program_id)
    SELECT NEW.txid, pid
    FROM program_ids
    WHERE pid IS NOT NULL;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Recreate the trigger
CREATE TRIGGER transaction_programs_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_transaction_programs();

-- Fix existing byte array program IDs
WITH RECURSIVE to_fix AS (
    SELECT 
        program_id,
        encode(
            decode(string_agg(
                lpad(
                    CASE 
                        WHEN num::int < 0 THEN (num::int + 256)::text
                        ELSE num::int::text
                    END,
                    2, '0'
                ),
                ''
            ), 'hex')::bytea,
            'hex'
        ) as fixed_program_id
    FROM programs,
    LATERAL regexp_split_to_table(
        trim(both '[]' from program_id), 
        ',\s*'
    ) AS num
    WHERE program_id LIKE '[%]'
    GROUP BY program_id
),
update_programs AS (
    UPDATE programs p
    SET program_id = f.fixed_program_id
    FROM to_fix f
    WHERE p.program_id = f.program_id
    RETURNING p.program_id, f.program_id as old_program_id
)
UPDATE transaction_programs tp
SET program_id = up.program_id
FROM update_programs up
WHERE tp.program_id = up.old_program_id;