-- Backfill token_accounts and token_mints from existing transactions

-- Ensure helper exists
DO $$ BEGIN
    PERFORM 1 FROM pg_proc WHERE proname = 'normalize_program_id';
END $$;

-- Backfill token_mints candidates at position 0
WITH tx AS (
    SELECT 
        t.txid,
        COALESCE(t.data#>'{message,account_keys}', t.data#>'{message,keys}') AS keys,
        jsonb_array_elements(COALESCE(t.data#>'{message,instructions}', '[]'::jsonb)) AS inst
    FROM transactions t
),
 tok AS (
    SELECT 
        COALESCE(
            canonical_program_id(keys -> NULLIF((inst->>'program_id_index')::int, NULL)),
            CASE 
                WHEN jsonb_typeof(inst->'program_id') = 'string' THEN canonical_program_id(inst->'program_id')
                WHEN jsonb_typeof(inst->'program_id') = 'array' THEN canonical_program_id(inst->'program_id')
                WHEN jsonb_typeof(inst->'program_id') = 'object' THEN canonical_program_id((inst->'program_id')->'pubkey')
                ELSE NULL
            END
        ) AS program_id,
        CASE jsonb_typeof(inst->'accounts')
            WHEN 'array' THEN inst->'accounts'
            ELSE '[]'::jsonb
        END AS accs,
        keys
    FROM tx
)
INSERT INTO token_mints (mint_address, program_id)
SELECT DISTINCT 
    CASE jsonb_typeof(accs->0)
        WHEN 'number' THEN normalize_program_id(keys -> ((accs->>0)::int))
        WHEN 'object' THEN normalize_program_id((accs->0)->'pubkey')
        ELSE NULL
    END AS mint_address,
    program_id
FROM tok
WHERE program_id IN (
  normalize_program_id('7ZMyUmgbNckx7G5BCrdmX2XUasjDAk5uhcMpDbUDxHQ3'),
  normalize_program_id('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA')
)
  AND (
    CASE jsonb_typeof(accs->0)
        WHEN 'number' THEN normalize_program_id(keys -> ((accs->>0)::int))
        WHEN 'object' THEN normalize_program_id((accs->0)->'pubkey')
        ELSE NULL
    END
  ) IS NOT NULL
ON CONFLICT (mint_address) DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP, program_id = EXCLUDED.program_id;

-- Backfill token_mints candidates at position 1
WITH tx AS (
    SELECT 
        t.txid,
        COALESCE(t.data#>'{message,account_keys}', t.data#>'{message,keys}') AS keys,
        jsonb_array_elements(COALESCE(t.data#>'{message,instructions}', '[]'::jsonb)) AS inst
    FROM transactions t
),
 tok AS (
    SELECT 
        COALESCE(
            canonical_program_id(keys -> NULLIF((inst->>'program_id_index')::int, NULL)),
            CASE 
                WHEN jsonb_typeof(inst->'program_id') = 'string' THEN canonical_program_id(inst->'program_id')
                WHEN jsonb_typeof(inst->'program_id') = 'array' THEN canonical_program_id(inst->'program_id')
                WHEN jsonb_typeof(inst->'program_id') = 'object' THEN canonical_program_id((inst->'program_id')->'pubkey')
                ELSE NULL
            END
        ) AS program_id,
        CASE jsonb_typeof(inst->'accounts')
            WHEN 'array' THEN inst->'accounts'
            ELSE '[]'::jsonb
        END AS accs,
        keys
    FROM tx
)
INSERT INTO token_mints (mint_address, program_id)
SELECT DISTINCT 
    CASE jsonb_typeof(accs->1)
        WHEN 'number' THEN normalize_program_id(keys -> ((accs->>1)::int))
        WHEN 'object' THEN normalize_program_id((accs->1)->'pubkey')
        ELSE NULL
    END AS mint_address,
    program_id
FROM tok
WHERE program_id IN (
  normalize_program_id('7ZMyUmgbNckx7G5BCrdmX2XUasjDAk5uhcMpDbUDxHQ3'),
  normalize_program_id('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA')
)
  AND (
    CASE jsonb_typeof(accs->1)
        WHEN 'number' THEN normalize_program_id(keys -> ((accs->>1)::int))
        WHEN 'object' THEN normalize_program_id((accs->1)->'pubkey')
        ELSE NULL
    END
  ) IS NOT NULL
ON CONFLICT (mint_address) DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP, program_id = EXCLUDED.program_id;

-- Backfill token_accounts rows where accounts look like [account, mint, owner]
WITH tx AS (
    SELECT 
        t.txid,
        COALESCE(t.data#>'{message,account_keys}', t.data#>'{message,keys}') AS keys,
        jsonb_array_elements(COALESCE(t.data#>'{message,instructions}', '[]'::jsonb)) AS inst
    FROM transactions t
),
 tok AS (
    SELECT 
        COALESCE(
            canonical_program_id(keys -> NULLIF((inst->>'program_id_index')::int, NULL)),
            CASE 
                WHEN jsonb_typeof(inst->'program_id') = 'string' THEN canonical_program_id(inst->'program_id')
                WHEN jsonb_typeof(inst->'program_id') = 'array' THEN canonical_program_id(inst->'program_id')
                WHEN jsonb_typeof(inst->'program_id') = 'object' THEN canonical_program_id((inst->'program_id')->'pubkey')
                ELSE NULL
            END
        ) AS program_id,
        CASE jsonb_typeof(inst->'accounts')
            WHEN 'array' THEN inst->'accounts'
            ELSE '[]'::jsonb
        END AS accs,
        keys,
        NULLIF((inst->'data'->>0)::int, NULL) AS opcode
    FROM tx
)
INSERT INTO token_accounts(token_account_hex, mint_address_hex, owner_address_hex, program_id_hex)
SELECT DISTINCT 
    CASE jsonb_typeof(accs->0)
        WHEN 'number' THEN normalize_program_id(keys -> ((accs->>0)::int))
        WHEN 'object' THEN normalize_program_id((accs->0)->'pubkey')
        ELSE NULL
    END AS token_account_hex,
    CASE jsonb_typeof(accs->1)
        WHEN 'number' THEN normalize_program_id(keys -> ((accs->>1)::int))
        WHEN 'object' THEN normalize_program_id((accs->1)->'pubkey')
        ELSE NULL
    END AS mint_address_hex,
    CASE jsonb_typeof(accs->2)
        WHEN 'number' THEN normalize_program_id(keys -> ((accs->>2)::int))
        WHEN 'object' THEN normalize_program_id((accs->2)->'pubkey')
        ELSE NULL
    END AS owner_address_hex,
    program_id AS program_id_hex
FROM tok
WHERE program_id IN (
  normalize_program_id('7ZMyUmgbNckx7G5BCrdmX2XUasjDAk5uhcMpDbUDxHQ3'),
  normalize_program_id('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA')
)
  AND opcode = 1
  AND jsonb_array_length(accs) >= 3
  AND (
    CASE jsonb_typeof(accs->0)
        WHEN 'number' THEN normalize_program_id(keys -> ((accs->>0)::int))
        WHEN 'object' THEN normalize_program_id((accs->0)->'pubkey')
        ELSE NULL
    END
  ) IS NOT NULL
  AND (
    CASE jsonb_typeof(accs->1)
        WHEN 'number' THEN normalize_program_id(keys -> ((accs->>1)::int))
        WHEN 'object' THEN normalize_program_id((accs->1)->'pubkey')
        ELSE NULL
    END
  ) IS NOT NULL
ON CONFLICT (token_account_hex) DO UPDATE SET
    mint_address_hex = EXCLUDED.mint_address_hex,
    owner_address_hex = COALESCE(EXCLUDED.owner_address_hex, token_accounts.owner_address_hex),
    program_id_hex = EXCLUDED.program_id_hex,
    last_seen_at = CURRENT_TIMESTAMP;

-- Seed token_balances sparsely for discovered accounts
INSERT INTO token_balances (account_address, mint_address, balance, decimals, owner_address, program_id)
SELECT DISTINCT 
    token_account_hex, mint_address_hex, 0, 0, owner_address_hex, program_id_hex
FROM token_accounts
ON CONFLICT (account_address, mint_address) DO UPDATE SET last_updated = CURRENT_TIMESTAMP;

ANALYZE token_accounts;
ANALYZE token_mints;
ANALYZE token_balances;
