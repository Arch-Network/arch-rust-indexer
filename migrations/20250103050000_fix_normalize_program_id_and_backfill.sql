-- Ensure normalize_program_id functions produce canonical hex for JSON byte arrays and strings
-- and make triggers populate participation/programs on INSERT or UPDATE.

BEGIN;

-- 1) Fix text overload: return lowercase hex
CREATE OR REPLACE FUNCTION public.normalize_program_id(input_id text)
RETURNS text
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
BEGIN
    IF input_id IS NULL OR input_id = '' THEN
        RETURN NULL;
    END IF;

    -- Already hex: even-length hex string
    IF input_id ~ '^[0-9A-Fa-f]+$' AND length(input_id) % 2 = 0 THEN
        RETURN lower(input_id);
    END IF;

    -- JSON-like byte array: [n1, n2, ...]
    IF input_id LIKE '[%' THEN
        RETURN (
            SELECT string_agg(lpad(to_hex(((num)::int + 256) % 256), 2, '0'), '')
            FROM regexp_split_to_table(trim(both '[]' from input_id), ',\s*') AS num
        );
    END IF;

    -- Fallback: unknown format (avoid broken base58 in DB)
    RETURN NULL;
END;
$$;

-- 2) Fix jsonb overload: delegate to text version
CREATE OR REPLACE FUNCTION public.normalize_program_id(input_id jsonb)
RETURNS text
LANGUAGE plpgsql
IMMUTABLE
AS $$
DECLARE s text;
BEGIN
    IF input_id IS NULL THEN
        RETURN NULL;
    END IF;

    IF jsonb_typeof(input_id) = 'string' THEN
        s := input_id #>> '{}';
        RETURN normalize_program_id(s);
    ELSIF jsonb_typeof(input_id) = 'array' THEN
        s := input_id::text;
        RETURN normalize_program_id(s);
    ELSE
        s := input_id::text;
        RETURN normalize_program_id(s);
    END IF;
END;
$$;

-- 3) Ensure triggers fire on INSERT OR UPDATE
DO $$ BEGIN
    IF to_regclass('public.account_participation') IS NOT NULL THEN
        DROP TRIGGER IF EXISTS account_participation_trigger ON transactions;
        CREATE TRIGGER account_participation_trigger
        AFTER INSERT OR UPDATE ON transactions
        FOR EACH ROW EXECUTE FUNCTION populate_account_participation();
    END IF;

    IF to_regclass('public.transaction_programs') IS NOT NULL THEN
        DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
        CREATE TRIGGER transaction_programs_trigger
        AFTER INSERT OR UPDATE ON transactions
        FOR EACH ROW EXECUTE FUNCTION update_transaction_programs();
    END IF;
END $$;

-- 4) Backfill participation from existing transactions (idempotent)
INSERT INTO account_participation (address_hex, txid, block_height, created_at)
SELECT DISTINCT
    CASE 
        WHEN jsonb_typeof(acc.value) = 'string' THEN normalize_program_id(trim(both '"' from (acc.value)::text))
        ELSE normalize_program_id((acc.value)::text)
    END AS address_hex,
    t.txid,
    t.block_height,
    t.created_at
FROM transactions t
CROSS JOIN LATERAL jsonb_array_elements(COALESCE(t.data#>'{message,account_keys}', t.data#>'{message,keys}', '[]'::jsonb)) AS acc(value)
WHERE normalize_program_id(acc.value) IS NOT NULL
ON CONFLICT (address_hex, txid) DO NOTHING;

COMMIT;
