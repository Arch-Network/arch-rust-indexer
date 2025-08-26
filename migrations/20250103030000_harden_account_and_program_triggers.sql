-- Replace account participation trigger to support varied JSON shapes
CREATE OR REPLACE FUNCTION populate_account_participation()
RETURNS TRIGGER AS $$
DECLARE
    v_key jsonb;
    v_hex text;
BEGIN
    -- Prefer message.account_keys; fallback to message.keys
    FOR v_key IN SELECT * FROM jsonb_array_elements(
        COALESCE(NEW.data#>'{message,account_keys}', NEW.data#>'{message,keys}', '[]'::jsonb)
    ) LOOP
        BEGIN
            v_hex := normalize_program_id(v_key);
            IF v_hex IS NOT NULL THEN
                INSERT INTO account_participation(address_hex, txid, block_height, created_at)
                VALUES (v_hex, NEW.txid, NEW.block_height, NEW.created_at)
                ON CONFLICT DO NOTHING;
            END IF;
        EXCEPTION WHEN OTHERS THEN
            CONTINUE;
        END;
    END LOOP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Replace program extraction trigger to use program_id_index against account_keys
CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
DECLARE
    v_inst jsonb;
    v_keys jsonb;
    v_idx int;
    v_pid text;
BEGIN
    v_keys := COALESCE(NEW.data#>'{message,account_keys}', NEW.data#>'{message,keys}', '[]'::jsonb);

    CREATE TEMP TABLE IF NOT EXISTS temp_programs (
        program_id text PRIMARY KEY
    ) ON COMMIT DROP;

    -- Prefer compiled instructions; fallback to instructions
    FOR v_inst IN SELECT * FROM jsonb_array_elements(
        COALESCE(NEW.data#>'{message,instructions}', NEW.data#>'{message,compiled_instructions}', '[]'::jsonb)
    ) LOOP
        BEGIN
            v_idx := NULLIF((v_inst->>'program_id_index')::int, NULL);
            IF v_idx IS NOT NULL AND v_idx >= 0 THEN
                v_pid := normalize_program_id(v_keys -> v_idx);
            ELSE
                -- Direct program_id field
                v_pid := CASE
                    WHEN jsonb_typeof(v_inst->'program_id') = 'string' THEN normalize_program_id(v_inst->>'program_id')
                    WHEN jsonb_typeof(v_inst->'program_id') = 'array' THEN normalize_program_id(v_inst->'program_id')
                    ELSE NULL
                END;
            END IF;

            IF v_pid IS NOT NULL THEN
                INSERT INTO temp_programs(program_id) VALUES (v_pid) ON CONFLICT DO NOTHING;
            END IF;
        EXCEPTION WHEN OTHERS THEN
            CONTINUE;
        END;
    END LOOP;

    INSERT INTO programs (program_id)
    SELECT program_id FROM temp_programs
    ON CONFLICT (program_id) DO UPDATE
        SET last_seen_at = CURRENT_TIMESTAMP,
            transaction_count = programs.transaction_count + 1;

    INSERT INTO transaction_programs(txid, program_id)
    SELECT NEW.txid, program_id FROM temp_programs
    ON CONFLICT DO NOTHING;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Recreate triggers to ensure ordering
DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
CREATE TRIGGER transaction_programs_trigger
AFTER INSERT ON transactions
FOR EACH ROW EXECUTE FUNCTION update_transaction_programs();

DROP TRIGGER IF EXISTS account_participation_trigger ON transactions;
CREATE TRIGGER account_participation_trigger
AFTER INSERT ON transactions
FOR EACH ROW EXECUTE FUNCTION populate_account_participation();
