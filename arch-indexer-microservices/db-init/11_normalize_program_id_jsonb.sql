-- Provide a jsonb overload for normalize_program_id to accept JSON inputs directly
CREATE OR REPLACE FUNCTION normalize_program_id(input_id jsonb)
RETURNS text AS $$
DECLARE
    s text;
BEGIN
    IF input_id IS NULL THEN
        RETURN NULL;
    END IF;

    IF jsonb_typeof(input_id) = 'string' THEN
        s := input_id #>> '{}';
        RETURN normalize_program_id(s);
    ELSIF jsonb_typeof(input_id) = 'array' THEN
        -- For arrays (e.g., byte arrays), delegate to text version which knows how to parse
        s := input_id::text;
        RETURN normalize_program_id(s);
    ELSE
        -- Fallback: cast to text and delegate
        s := input_id::text;
        RETURN normalize_program_id(s);
    END IF;
END;
$$ LANGUAGE plpgsql IMMUTABLE;
