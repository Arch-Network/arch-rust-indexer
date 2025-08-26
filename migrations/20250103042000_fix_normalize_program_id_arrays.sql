-- Fix normalize_program_id to properly handle JSON array text (e.g., "[55,229,...]")
-- This version converts each decimal byte to a two-digit hex using to_hex()

CREATE OR REPLACE FUNCTION normalize_program_id(input_id TEXT)
RETURNS TEXT AS $$
BEGIN
    IF input_id IS NULL OR input_id = '' THEN
        RETURN NULL;
    END IF;

    -- Already hex: normalize to lowercase
    IF input_id ~ '^[0-9a-fA-F]{2,}$' THEN
        RETURN lower(input_id);
    END IF;

    -- Try base58 decode
    BEGIN
        RETURN encode(decode_base58(input_id), 'hex');
    EXCEPTION WHEN OTHERS THEN
        -- If base58 fails, try bracketed decimal byte array form
        IF input_id LIKE '[%' THEN
            RETURN (
                SELECT lower(string_agg(lpad(to_hex(CASE 
                                                        WHEN num::int < 0 THEN (num::int + 256)
                                                        ELSE num::int 
                                                    END), 2, '0'), ''))
                FROM regexp_split_to_table(
                    trim(both '[]' from input_id), 
                    ',\s*'
                ) AS num
            );
        END IF;
    END;

    RETURN NULL;
END;
$$ LANGUAGE plpgsql IMMUTABLE STRICT;

-- Ensure the jsonb overload delegates to the text version which now supports arrays
CREATE OR REPLACE FUNCTION normalize_program_id(input_id jsonb)
RETURNS text AS $$
BEGIN
    IF input_id IS NULL THEN
        RETURN NULL;
    END IF;
    RETURN normalize_program_id(input_id::text);
END;
$$ LANGUAGE plpgsql IMMUTABLE;
