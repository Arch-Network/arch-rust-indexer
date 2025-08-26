-- token_accounts maps token account pubkey to its mint and owner for inference
CREATE TABLE IF NOT EXISTS token_accounts (
    token_account_hex TEXT PRIMARY KEY,
    mint_address_hex TEXT NOT NULL,
    owner_address_hex TEXT,
    program_id_hex TEXT NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_token_accounts_owner ON token_accounts(owner_address_hex);
CREATE INDEX IF NOT EXISTS idx_token_accounts_mint ON token_accounts(mint_address_hex);
CREATE INDEX IF NOT EXISTS idx_token_accounts_program ON token_accounts(program_id_hex);

-- lightweight upsert helper
CREATE OR REPLACE FUNCTION upsert_token_account(_acct TEXT, _mint TEXT, _owner TEXT, _program TEXT)
RETURNS VOID AS $$
BEGIN
    INSERT INTO token_accounts(token_account_hex, mint_address_hex, owner_address_hex, program_id_hex, last_seen_at)
    VALUES (_acct, _mint, _owner, _program, CURRENT_TIMESTAMP)
    ON CONFLICT (token_account_hex) DO UPDATE SET
        mint_address_hex = EXCLUDED.mint_address_hex,
        owner_address_hex = COALESCE(EXCLUDED.owner_address_hex, token_accounts.owner_address_hex),
        program_id_hex = EXCLUDED.program_id_hex,
        last_seen_at = CURRENT_TIMESTAMP;
END;
$$ LANGUAGE plpgsql;
