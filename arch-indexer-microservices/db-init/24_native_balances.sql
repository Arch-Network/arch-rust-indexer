-- Native ARCH balance tracking (lamports) persisted at index time

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

CREATE OR REPLACE FUNCTION populate_native_balances_from_tx()
RETURNS TRIGGER AS $$
DECLARE
    inst JSONB;
    keys JSONB;
    accs JSONB;
    program_id TEXT;
    acc0 TEXT;
    acc1 TEXT;
    lam BIGINT;
    data BYTEA;
    tag INT;
    arr_len INT;
BEGIN
    IF NEW.data IS NULL THEN RETURN NEW; END IF;

    keys := COALESCE(NEW.data#>'{message,account_keys}', NEW.data#>'{message,keys}', '[]'::jsonb);

    FOR inst IN SELECT * FROM jsonb_array_elements(COALESCE(NEW.data#>'{message,instructions}', '[]'::jsonb)) LOOP
        -- Resolve canonical program id
        program_id := NULL;
        BEGIN
            IF (inst ? 'program_id_index') THEN
                program_id := canonical_program_id(keys -> ((inst->>'program_id_index')::int));
            ELSIF (inst ? 'program_id') THEN
                program_id := canonical_program_id(inst->'program_id');
            END IF;
        EXCEPTION WHEN others THEN program_id := NULL; END;

        -- System program only
        IF program_id IS NULL THEN CONTINUE; END IF;
        IF NOT (
            program_id = normalize_program_id('11111111111111111111111111111112') OR
            program_id = normalize_program_id('0000000000000000000000000000000000000000000000000000000000000001') OR
            program_id = normalize_program_id('0000000000000000000000000000000000000000000000000000000000000000')
        ) THEN
            CONTINUE;
        END IF;

        accs := COALESCE(inst->'accounts', '[]'::jsonb);
        -- Resolve first two account pubkeys to hex
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

        -- Decode tag and lamports directly from JSON numeric array (little-endian)
        arr_len := COALESCE(jsonb_array_length(COALESCE(inst->'data','[]'::jsonb)), 0);
        IF arr_len >= 4 THEN
            SELECT COALESCE(SUM((v::int) * CASE ord WHEN 1 THEN 1 WHEN 2 THEN 256 WHEN 3 THEN 65536 WHEN 4 THEN 16777216 END), 0)
            INTO tag
            FROM jsonb_array_elements(COALESCE(inst->'data','[]'::jsonb)) WITH ORDINALITY AS t(v, ord)
            WHERE ord BETWEEN 1 AND 4;
        ELSE
            tag := NULL;
        END IF;

        IF arr_len >= 12 THEN
            SELECT SUM((v::numeric) * power(256::numeric, (ord-5)))
            INTO lam
            FROM jsonb_array_elements(COALESCE(inst->'data','[]'::jsonb)) WITH ORDINALITY AS t(v, ord)
            WHERE ord BETWEEN 5 AND 12;
        ELSE
            lam := NULL;
        END IF;

        -- Fallbacks for decoded-form payloads
        IF lam IS NULL AND (inst ? 'lamports') THEN
            BEGIN
                lam := (inst->'lamports'->>'data')::numeric;
            EXCEPTION WHEN others THEN lam := NULL; END;
        END IF;
        IF tag IS NULL AND (inst ? 'discriminator') THEN
            BEGIN
                tag := (inst->'discriminator'->>'data')::int;
            EXCEPTION WHEN others THEN tag := NULL; END;
        END IF;

        IF lam IS NULL THEN CONTINUE; END IF;

        -- CreateAccount (0 or 3) or Transfer (2 or 4)
        IF (tag = 0 OR tag = 3) AND acc0 IS NOT NULL AND acc1 IS NOT NULL THEN
            PERFORM nb_apply_delta(acc0, -lam);
            PERFORM nb_apply_delta(acc1, lam);
        ELSIF (tag = 2 OR tag = 4) AND acc0 IS NOT NULL AND acc1 IS NOT NULL THEN
            PERFORM nb_apply_delta(acc0, -lam);
            PERFORM nb_apply_delta(acc1, lam);
        ELSIF arr_len = 12 AND acc0 IS NOT NULL AND acc1 IS NOT NULL THEN
            PERFORM nb_apply_delta(acc0, -lam);
            PERFORM nb_apply_delta(acc1, lam);
        END IF;
    END LOOP;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER native_balances_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION populate_native_balances_from_tx();

ANALYZE native_balances;
