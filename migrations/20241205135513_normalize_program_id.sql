-- First, create the normalize_program_id function
CREATE OR REPLACE FUNCTION normalize_program_id(input_id TEXT)
RETURNS TEXT AS $$
BEGIN
    -- If it looks like a hex string already, return it
    IF input_id ~ '^[0-9a-f]{2,}$' THEN
        RETURN input_id;
    END IF;
    
    -- If it's a base58 string, convert it to hex
    BEGIN
        RETURN encode(decode(input_id, 'base58'), 'hex');
    EXCEPTION WHEN OTHERS THEN
        -- If it's a byte array string like '[1,2,3]'
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
$$ LANGUAGE plpgsql;

-- First, create a consolidated mapping of all program IDs
CREATE TEMP TABLE program_id_consolidated AS
WITH normalized_mappings AS (
    SELECT 
        program_id as old_id,
        normalize_program_id(program_id) as new_id,
        last_seen_at,
        transaction_count,
        first_seen_at
    FROM programs
    WHERE normalize_program_id(program_id) IS NOT NULL
),
consolidated AS (
    SELECT 
        new_id,
        MIN(first_seen_at) as first_seen_at,
        MAX(last_seen_at) as last_seen_at,
        SUM(transaction_count) as transaction_count,
        array_agg(old_id) as old_ids
    FROM normalized_mappings
    GROUP BY new_id
)
SELECT * FROM consolidated;

-- Update transaction_programs to use new IDs
UPDATE transaction_programs tp
SET program_id = c.new_id
FROM program_id_consolidated c
WHERE tp.program_id = ANY(c.old_ids);

-- Delete all old program records that will be replaced
DELETE FROM programs p
WHERE EXISTS (
    SELECT 1 
    FROM program_id_consolidated c
    WHERE p.program_id = ANY(c.old_ids)
);

-- Insert consolidated records
INSERT INTO programs (program_id, first_seen_at, last_seen_at, transaction_count)
SELECT 
    new_id,
    first_seen_at,
    last_seen_at,
    transaction_count
FROM program_id_consolidated;

-- Clean up
DROP TABLE program_id_consolidated;