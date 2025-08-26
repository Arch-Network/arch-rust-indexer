-- Backfill account_participation from existing transactions
-- Requires normalize_program_id(jsonb) to be present

CREATE TABLE IF NOT EXISTS account_participation (
    address_hex TEXT NOT NULL,
    txid TEXT NOT NULL REFERENCES transactions(txid) ON DELETE CASCADE,
    block_height BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (address_hex, txid)
);

INSERT INTO account_participation (address_hex, txid, block_height, created_at)
SELECT DISTINCT normalize_program_id(v.key) AS address_hex, t.txid, t.block_height, t.created_at
FROM transactions t
CROSS JOIN LATERAL jsonb_array_elements(COALESCE(t.data#>'{message,account_keys}', t.data#>'{message,keys}', '[]'::jsonb)) AS v(key)
WHERE normalize_program_id(v.key) IS NOT NULL
ON CONFLICT (address_hex, txid) DO NOTHING;
