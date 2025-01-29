-- Create base58 decode function
CREATE OR REPLACE FUNCTION decode_base58(text) RETURNS bytea AS $$
DECLARE
    alphabet text := '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
    base58_len int := length(alphabet);
    input_len int := length($1);
    value numeric := 0;
    i int;
    ch text;
    position int;
    hex_str text;
BEGIN
    -- Convert base58 to numeric
    FOR i IN 1..input_len LOOP
        ch := substr($1, i, 1);
        position := position(ch in alphabet) - 1;
        IF position < 0 THEN
            RETURN NULL;
        END IF;
        value := value * base58_len + position;
    END LOOP;

    -- Convert numeric to hex string, handling large numbers
    hex_str := '';
    WHILE value > 0 LOOP
        hex_str := substr('0123456789abcdef', (value % 16)::integer + 1, 1) || hex_str;
        value := value / 16;
    END LOOP;

    -- Ensure even number of hex chars
    IF length(hex_str) % 2 = 1 THEN
        hex_str := '0' || hex_str;
    END IF;
    
    RETURN decode(hex_str, 'hex');
END;
$$ LANGUAGE plpgsql IMMUTABLE STRICT;

-- Create a temporary table to store the mappings
CREATE TEMP TABLE transaction_program_fixes AS
WITH program_mappings AS (
    SELECT DISTINCT
        tp.program_id as old_id,
        CASE 
            -- If it's already a valid hex string, keep it
            WHEN tp.program_id ~ '^[0-9a-f]{2,}$' THEN tp.program_id
            -- Try to convert everything else as base58
            ELSE encode(decode_base58(tp.program_id), 'hex')
        END as new_id
    FROM transaction_programs tp
    WHERE tp.program_id !~ '^[0-9a-f]{2,}$'  -- Only process non-hex strings
)
SELECT * FROM program_mappings
WHERE new_id IS NOT NULL;

-- Insert missing program_ids into programs table
INSERT INTO programs (program_id)
SELECT new_id FROM transaction_program_fixes
ON CONFLICT DO NOTHING;

-- Update transaction_programs with normalized program_ids
UPDATE transaction_programs tp
SET program_id = f.new_id
FROM transaction_program_fixes f
WHERE tp.program_id = f.old_id;

-- Clean up
DROP TABLE transaction_program_fixes;
DROP FUNCTION IF EXISTS decode_base58(text);