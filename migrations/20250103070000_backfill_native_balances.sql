-- Backfill native_balances from historical transactions

CREATE TABLE IF NOT EXISTS native_balances (
    address_hex TEXT PRIMARY KEY,
    balance NUMERIC(65,0) NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE OR REPLACE FUNCTION nb_apply_delta(addr TEXT, delta NUMERIC)
RETURNS VOID AS $$
BEGIN
    INSERT INTO native_balances(address_hex, balance, updated_at)
    VALUES (addr, delta, CURRENT_TIMESTAMP)
    ON CONFLICT (address_hex) DO UPDATE SET
        balance = native_balances.balance + EXCLUDED.balance,
        updated_at = CURRENT_TIMESTAMP;
END;
$$ LANGUAGE plpgsql;

DO $$
DECLARE r_row RECORD;
        keys JSONB;
        inst JSONB;
        accs JSONB;
        acc0 TEXT;
        acc1 TEXT;
        program_id TEXT;
        tag_num NUMERIC;
        lam NUMERIC;
        arr_len INT;
BEGIN
    -- Rebuild from scratch for idempotent runs
    TRUNCATE TABLE native_balances;
    FOR r_row IN SELECT t.data FROM transactions t ORDER BY t.created_at ASC LOOP
        keys := COALESCE(r_row.data#>'{message,account_keys}', r_row.data#>'{message,keys}', '[]'::jsonb);
        FOR inst IN SELECT * FROM jsonb_array_elements(COALESCE(r_row.data#>'{message,instructions}', '[]'::jsonb)) LOOP
            -- We won't filter by program id; rely on tag decoding to identify System ops
            program_id := NULL;  -- kept for reference but unused in filter

            accs := COALESCE(inst->'accounts','[]'::jsonb);
            acc0 := CASE jsonb_typeof(accs->0)
                WHEN 'number' THEN canonical_program_id(keys -> ((accs->>0)::int))
                WHEN 'array'  THEN canonical_program_id(accs->0)
                WHEN 'object' THEN canonical_program_id((accs->0)->'pubkey')
                ELSE NULL END;
            acc1 := CASE jsonb_typeof(accs->1)
                WHEN 'number' THEN canonical_program_id(keys -> ((accs->>1)::int))
                WHEN 'array'  THEN canonical_program_id(accs->1)
                WHEN 'object' THEN canonical_program_id((accs->1)->'pubkey')
                ELSE NULL END;

            arr_len := COALESCE(jsonb_array_length(COALESCE(inst->'data','[]'::jsonb)), 0);
            -- Compute tag and lamports directly from JSON number array (little endian) when present
            IF arr_len >= 12 THEN
                SELECT SUM((v::numeric) * power(256::numeric, (ord-1))) INTO tag_num
                FROM jsonb_array_elements(COALESCE(inst->'data','[]'::jsonb)) WITH ORDINALITY AS t(v,ord)
                WHERE ord BETWEEN 1 AND 4;
                SELECT SUM((v::numeric) * power(256::numeric, (ord-5))) INTO lam
                FROM jsonb_array_elements(COALESCE(inst->'data','[]'::jsonb)) WITH ORDINALITY AS t(v,ord)
                WHERE ord BETWEEN 5 AND 12;
            ELSE
                tag_num := NULL;
                lam := NULL;
            END IF;

            -- If decoded form exists (e.g., discriminator + lamports.data), use it
            IF lam IS NULL AND (inst ? 'lamports') THEN
                BEGIN
                    lam := (inst->'lamports'->>'data')::numeric;
                EXCEPTION WHEN others THEN lam := NULL; END;
            END IF;
            IF tag_num IS NULL AND (inst ? 'discriminator') THEN
                BEGIN
                    tag_num := (inst->'discriminator'->>'data')::numeric;
                EXCEPTION WHEN others THEN tag_num := NULL; END;
            END IF;
            IF lam IS NULL THEN CONTINUE; END IF;
            IF acc0 IS NULL OR acc1 IS NULL THEN CONTINUE; END IF;
            IF (tag_num IN (0,3,2,4)) THEN
                PERFORM nb_apply_delta(acc0, -lam);
                PERFORM nb_apply_delta(acc1, lam);
            ELSIF lam IS NOT NULL AND arr_len = 12 THEN
                -- 12-byte fallback transfer
                PERFORM nb_apply_delta(acc0, -lam);
                PERFORM nb_apply_delta(acc1, lam);
            END IF;
        END LOOP;
    END LOOP;
END$$;

ANALYZE native_balances;
