-- Add token balances table for tracking token holdings
CREATE TABLE IF NOT EXISTS token_balances (
    id SERIAL PRIMARY KEY,
    account_address TEXT NOT NULL,
    mint_address TEXT NOT NULL,
    balance NUMERIC(65, 0) NOT NULL DEFAULT 0,
    decimals INTEGER NOT NULL DEFAULT 0,
    owner_address TEXT,
    program_id TEXT NOT NULL,
    last_updated TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(account_address, mint_address)
);

-- Create indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_token_balances_account ON token_balances(account_address);
CREATE INDEX IF NOT EXISTS idx_token_balances_mint ON token_balances(mint_address);
CREATE INDEX IF NOT EXISTS idx_token_balances_program ON token_balances(program_id);
CREATE INDEX IF NOT EXISTS idx_token_balances_owner ON token_balances(owner_address);
CREATE INDEX IF NOT EXISTS idx_token_balances_updated ON token_balances(last_updated);

-- Create token mints table for metadata
CREATE TABLE IF NOT EXISTS token_mints (
    mint_address TEXT PRIMARY KEY,
    program_id TEXT NOT NULL,
    decimals INTEGER NOT NULL DEFAULT 0,
    supply NUMERIC(65, 0) NOT NULL DEFAULT 0,
    is_frozen BOOLEAN DEFAULT FALSE,
    mint_authority TEXT,
    freeze_authority TEXT,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for token mints
CREATE INDEX IF NOT EXISTS idx_token_mints_program ON token_mints(program_id);
CREATE INDEX IF NOT EXISTS idx_token_mints_authority ON token_mints(mint_authority);

-- Create function to update token balances from transaction data
CREATE OR REPLACE FUNCTION update_token_balances_from_transaction()
RETURNS TRIGGER AS $$
DECLARE
    instruction JSONB;
    accounts JSONB;
    program_id TEXT;
    mint_address TEXT;
    account_address TEXT;
    owner_address TEXT;
    balance_change NUMERIC;
    decimals INTEGER;
    instruction_type TEXT;
BEGIN
    -- Only process token program instructions
    IF NEW.data->'message'->'instructions' IS NULL THEN
        RETURN NEW;
    END IF;

    -- Process each instruction
    FOR instruction IN SELECT * FROM jsonb_array_elements(NEW.data->'message'->'instructions')
    LOOP
        -- Get program ID for this instruction
        program_id := normalize_program_id(instruction->>'program_id');
        
        -- Only process known token programs
        IF program_id NOT IN (
            '7ZMyUmgbNckx7G5BCrdmX2XUasjDAk5uhcMpDbUDxHQ3', -- APL Token
            'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'   -- SPL Token
        ) THEN
            CONTINUE;
        END IF;

        -- Get accounts for this instruction
        accounts := NEW.data->'message'->'account_keys';
        
        -- Extract instruction data
        instruction_type := instruction->>'data';
        
        -- Handle different instruction types
        CASE 
            WHEN instruction_type LIKE '%transfer%' THEN
                -- Transfer instruction
                IF jsonb_array_length(accounts) >= 3 THEN
                    account_address := normalize_program_id(accounts->0);
                    mint_address := normalize_program_id(accounts->1);
                    owner_address := normalize_program_id(accounts->2);
                    
                    -- Update balances (simplified - in real implementation you'd parse the actual data)
                    -- This is a placeholder for the actual balance calculation logic
                    INSERT INTO token_balances (account_address, mint_address, balance, decimals, owner_address, program_id)
                    VALUES (account_address, mint_address, 0, 0, owner_address, program_id)
                    ON CONFLICT (account_address, mint_address) 
                    DO UPDATE SET 
                        last_updated = CURRENT_TIMESTAMP,
                        owner_address = EXCLUDED.owner_address;
                END IF;
                
            WHEN instruction_type LIKE '%mint%' THEN
                -- Mint instruction
                IF jsonb_array_length(accounts) >= 2 THEN
                    mint_address := normalize_program_id(accounts->0);
                    account_address := normalize_program_id(accounts->1);
                    
                    -- Insert or update mint info
                    INSERT INTO token_mints (mint_address, program_id, decimals, mint_authority)
                    VALUES (mint_address, program_id, 0, account_address)
                    ON CONFLICT (mint_address) 
                    DO UPDATE SET 
                        last_seen_at = CURRENT_TIMESTAMP;
                END IF;
                
            ELSE
                -- Other instruction types - just track account participation
                IF jsonb_array_length(accounts) >= 1 THEN
                    account_address := normalize_program_id(accounts->0);
                    mint_address := normalize_program_id(accounts->1);
                    
                    IF account_address IS NOT NULL AND mint_address IS NOT NULL THEN
                        INSERT INTO token_balances (account_address, mint_address, balance, decimals, program_id)
                        VALUES (account_address, mint_address, 0, 0, program_id)
                        ON CONFLICT (account_address, mint_address) 
                        DO UPDATE SET last_updated = CURRENT_TIMESTAMP;
                    END IF;
                END IF;
        END CASE;
    END LOOP;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger to automatically update token balances
CREATE TRIGGER token_balances_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_token_balances_from_transaction();

-- Create view for easy token balance queries
CREATE OR REPLACE VIEW account_token_balances AS
SELECT 
    tb.account_address,
    tb.mint_address,
    tb.balance,
    tb.decimals,
    tb.owner_address,
    tb.program_id,
    tm.supply,
    tm.is_frozen,
    tm.mint_authority,
    tb.last_updated,
    tb.created_at
FROM token_balances tb
LEFT JOIN token_mints tm ON tb.mint_address = tm.mint_address
ORDER BY tb.last_updated DESC;

-- Update statistics
ANALYZE token_balances;
ANALYZE token_mints;
