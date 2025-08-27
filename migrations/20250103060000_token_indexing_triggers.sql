-- Robust token indexing triggers: populate token_accounts, token_mints, and token_balances (sparse) from transactions

-- Known token-related program IDs (APL Token and SPL Token)
DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_proc WHERE proname = 'populate_token_entities_from_tx'
    ) THEN
        -- noop, just to ensure block compiles
        PERFORM 1;
    END IF;
END $$;

-- Replace legacy simplistic trigger if present
DROP TRIGGER IF EXISTS token_balances_trigger ON transactions;
DROP FUNCTION IF EXISTS update_token_balances_from_transaction();
DROP TRIGGER IF EXISTS token_indexing_trigger ON transactions;

CREATE OR REPLACE FUNCTION populate_token_entities_from_tx()
RETURNS TRIGGER AS $$
DECLARE
    inst JSONB;
    keys JSONB;
    accs JSONB;
    program_id TEXT;
    acct TEXT;
    mint TEXT;
    owner TEXT;
BEGIN
    IF NEW.data IS NULL THEN
        RETURN NEW;
    END IF;

    keys := COALESCE(
        NEW.data#>'{message,account_keys}',
        NEW.data#>'{message,keys}',
        '[]'::jsonb
    );

    -- Iterate instructions defensively
    FOR inst IN SELECT * FROM jsonb_array_elements(COALESCE(NEW.data#>'{message,instructions}', '[]'::jsonb)) LOOP
        -- Derive program id from index or embedded field
        program_id := NULL;
        BEGIN
            IF (inst ? 'program_id_index') THEN
                program_id := canonical_program_id(keys -> ((inst->>'program_id_index')::int));
            ELSIF (inst ? 'program_id') THEN
                CASE jsonb_typeof(inst->'program_id')
                    WHEN 'string' THEN program_id := canonical_program_id(inst->'program_id');
                    WHEN 'array' THEN program_id := canonical_program_id(inst->'program_id');
                    WHEN 'object' THEN program_id := canonical_program_id((inst->'program_id')->'pubkey');
                    ELSE program_id := NULL;
                END CASE;
            END IF;
        EXCEPTION WHEN others THEN
            program_id := NULL;
        END;

        IF program_id IS NULL THEN
            CONTINUE;
        END IF;

        -- Only token programs we care about
        IF program_id NOT IN (
            normalize_program_id('7ZMyUmgbNckx7G5BCrdmX2XUasjDAk5uhcMpDbUDxHQ3'), -- APL Token
            normalize_program_id('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA')  -- SPL Token
        ) THEN
            CONTINUE;
        END IF;

        accs := COALESCE(inst->'accounts', '[]'::jsonb);

        -- 1) Attempt to index token accounts for initialize_account-like shape: [token_account, mint, owner]
        IF jsonb_array_length(accs) >= 3 THEN
            BEGIN
                -- Account 0
                CASE jsonb_typeof(accs->0)
                    WHEN 'number' THEN acct := normalize_program_id(keys -> ((accs->>0)::int));
                    WHEN 'object' THEN acct := normalize_program_id((accs->0)->'pubkey');
                    ELSE acct := NULL;
                END CASE;
                -- Account 1 (mint candidate)
                CASE jsonb_typeof(accs->1)
                    WHEN 'number' THEN mint := normalize_program_id(keys -> ((accs->>1)::int));
                    WHEN 'object' THEN mint := normalize_program_id((accs->1)->'pubkey');
                    ELSE mint := NULL;
                END CASE;
                -- Account 2 (owner)
                CASE jsonb_typeof(accs->2)
                    WHEN 'number' THEN owner := normalize_program_id(keys -> ((accs->>2)::int));
                    WHEN 'object' THEN owner := normalize_program_id((accs->2)->'pubkey');
                    ELSE owner := NULL;
                END CASE;
            EXCEPTION WHEN others THEN
                acct := NULL; mint := NULL; owner := NULL;
            END;

            IF acct IS NOT NULL AND mint IS NOT NULL THEN
                PERFORM upsert_token_account(acct, mint, owner, program_id);

                -- Seed token_balances with a zero row to enable aggregations
                INSERT INTO token_balances (account_address, mint_address, balance, decimals, owner_address, program_id)
                VALUES (acct, mint, 0, 0, owner, program_id)
                ON CONFLICT (account_address, mint_address) DO UPDATE SET
                    last_updated = CURRENT_TIMESTAMP,
                    owner_address = COALESCE(EXCLUDED.owner_address, token_balances.owner_address),
                    program_id = EXCLUDED.program_id;
            END IF;
        END IF;

        -- 2) Index observed mint accounts conservatively.
        -- Many token instructions reference the mint at position 0 or 1. Insert both candidates as mints; they will be de-duplicated and refined as more txs arrive.
        IF jsonb_array_length(accs) >= 1 THEN
            BEGIN
                CASE jsonb_typeof(accs->0)
                    WHEN 'number' THEN mint := normalize_program_id(keys -> ((accs->>0)::int));
                    WHEN 'object' THEN mint := normalize_program_id((accs->0)->'pubkey');
                    ELSE mint := NULL;
                END CASE;
            EXCEPTION WHEN others THEN
                mint := NULL;
            END;
            IF mint IS NOT NULL THEN
                INSERT INTO token_mints (mint_address, program_id)
                VALUES (mint, program_id)
                ON CONFLICT (mint_address) DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP, program_id = EXCLUDED.program_id;
            END IF;
        END IF;

        IF jsonb_array_length(accs) >= 2 THEN
            BEGIN
                CASE jsonb_typeof(accs->1)
                    WHEN 'number' THEN mint := normalize_program_id(keys -> ((accs->>1)::int));
                    WHEN 'object' THEN mint := normalize_program_id((accs->1)->'pubkey');
                    ELSE mint := NULL;
                END CASE;
            EXCEPTION WHEN others THEN
                mint := NULL;
            END;
            IF mint IS NOT NULL THEN
                INSERT INTO token_mints (mint_address, program_id)
                VALUES (mint, program_id)
                ON CONFLICT (mint_address) DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP, program_id = EXCLUDED.program_id;
            END IF;
        END IF;
    END LOOP;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER token_indexing_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION populate_token_entities_from_tx();

-- Analyze for planner
ANALYZE token_accounts;
ANALYZE token_mints;
ANALYZE token_balances;
