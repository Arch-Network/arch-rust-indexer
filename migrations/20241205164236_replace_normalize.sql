-- Drop existing function if it exists (with any parameter names)
DROP FUNCTION IF EXISTS normalize_program_id(TEXT);

-- Create or replace the normalize_program_id function
CREATE OR REPLACE FUNCTION normalize_program_id(input_id TEXT)
RETURNS TEXT AS $$
BEGIN
    -- If already a valid hex string, return it
    IF input_id ~ '^[0-9a-f]{64}$' THEN
        RETURN input_id;
    END IF;
    
    -- Try base58 decode first
    BEGIN
        RETURN encode(decode_base58(input_id), 'hex');
    EXCEPTION WHEN OTHERS THEN
        -- If base58 fails, try byte array format
        IF input_id LIKE '[%]' THEN
            RETURN encode(
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
            ) FROM regexp_split_to_table(
                trim(both '[]' from input_id), 
                ',\s*'
            ) AS num;
        END IF;
    END;
    
    RETURN NULL;
END;
$$ LANGUAGE plpgsql IMMUTABLE STRICT;