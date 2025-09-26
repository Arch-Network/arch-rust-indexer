-- Initial schema setup
CREATE TABLE IF NOT EXISTS blocks (
    height BIGINT PRIMARY KEY,
    hash TEXT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    bitcoin_block_height BIGINT
);

CREATE TABLE IF NOT EXISTS transactions (
    txid TEXT PRIMARY KEY,
    block_height BIGINT NOT NULL,
    data JSONB NOT NULL,
    status JSONB NOT NULL DEFAULT '0'::jsonb,
    bitcoin_txids TEXT[] DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (block_height) REFERENCES blocks(height)
);

-- Create base indexes
CREATE INDEX IF NOT EXISTS idx_transactions_block_height ON transactions(block_height);
CREATE INDEX IF NOT EXISTS idx_transactions_created_at ON transactions(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_blocks_bitcoin_block_height ON blocks(bitcoin_block_height);
CREATE INDEX IF NOT EXISTS idx_blocks_timestamp ON blocks(timestamp);

-- Create program tracking tables
CREATE TABLE IF NOT EXISTS programs (
    program_id TEXT PRIMARY KEY,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    transaction_count BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS transaction_programs (
    txid TEXT REFERENCES transactions(txid) ON DELETE CASCADE,
    program_id TEXT REFERENCES programs(program_id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (txid, program_id)
);

-- Create program-related indexes
CREATE INDEX IF NOT EXISTS idx_transaction_programs_program_id ON transaction_programs(program_id);
CREATE INDEX IF NOT EXISTS idx_programs_last_seen ON programs(last_seen_at);
CREATE INDEX IF NOT EXISTS idx_programs_transaction_count ON programs(transaction_count DESC);
CREATE INDEX IF NOT EXISTS idx_programs_program_id_pattern ON programs(program_id text_pattern_ops);

-- Create base58 decode function
CREATE OR REPLACE FUNCTION decode_base58(text) RETURNS bytea AS $$
DECLARE
    alphabet text := '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
    base58_len int := length(alphabet);
    input_len int := length($1);
    value numeric := 0;
    i int;
    ch text;
    position int;
    hex_str text;
BEGIN
    -- Convert base58 to numeric
    FOR i IN 1..input_len LOOP
        ch := substr($1, i, 1);
        position := position(ch in alphabet) - 1;
        IF position < 0 THEN
            RETURN NULL;
        END IF;
        value := value * base58_len + position;
    END LOOP;

    -- Convert numeric to hex string
    hex_str := '';
    WHILE value > 0 LOOP
        hex_str := substr('0123456789abcdef', (value % 16)::integer + 1, 1) || hex_str;
        value := value / 16;
    END LOOP;

    -- Ensure even number of hex chars
    IF length(hex_str) % 2 = 1 THEN
        hex_str := '0' || hex_str;
    END IF;
    
    RETURN decode(hex_str, 'hex');
END;
$$ LANGUAGE plpgsql IMMUTABLE STRICT;

-- Create normalize_program_id function
CREATE OR REPLACE FUNCTION normalize_program_id(input_id TEXT)
RETURNS TEXT AS $$
BEGIN
    -- If it's NULL or empty, return NULL
    IF input_id IS NULL OR input_id = '' THEN
        RETURN NULL;
    END IF;

    -- If already a valid hex string, return it
    IF input_id ~ '^[0-9a-f]{2,}$' THEN
        RETURN input_id;
    END IF;
    
    -- Try base58 decode first
    BEGIN
        RETURN encode(decode_base58(input_id), 'hex');
    EXCEPTION WHEN OTHERS THEN
        -- If base58 fails, try byte array format
        IF input_id LIKE '[%]' THEN
            RETURN encode(
                decode(
                    string_agg(
                        lpad(
                            CASE 
                                WHEN num::int < 0 THEN (num::int + 256)::text
                                ELSE num::int::text
                            END,
                            2, '0'
                        ),
                        ''
                    ),
                    'hex'
                ),
                'hex'
            ) FROM regexp_split_to_table(
                trim(both '[]' from input_id), 
                ',\s*'
            ) AS num;
        END IF;
    END;
    
    RETURN NULL;
END;
$$ LANGUAGE plpgsql IMMUTABLE STRICT;

-- Create optimized trigger function
CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
DECLARE
    inst jsonb;
    program_id text;
BEGIN
    -- Create temporary table for batch processing
    CREATE TEMP TABLE IF NOT EXISTS temp_programs (
        program_id text PRIMARY KEY
    ) ON COMMIT DROP;

    -- Process each instruction (canonicalize to Arch fixed IDs)
    FOR inst IN SELECT * FROM jsonb_array_elements(
        CASE 
            WHEN jsonb_typeof(NEW.data#>'{message,instructions}') = 'array' 
            THEN NEW.data#>'{message,instructions}'
            ELSE '[]'::jsonb
        END
    )
    LOOP
        -- Extract and canonicalize program_id based on type
        program_id := NULL;
        BEGIN
            IF (inst ? 'program_id_index') THEN
                program_id := canonical_program_id((NEW.data#>'{message,account_keys}') -> ((inst->>'program_id_index')::int));
            ELSIF (inst ? 'program_id') THEN
                IF jsonb_typeof(inst->'program_id') = 'object' THEN
                    program_id := canonical_program_id((inst->'program_id')->'pubkey');
                ELSE
                    program_id := canonical_program_id(inst->'program_id');
                END IF;
            END IF;
        EXCEPTION WHEN others THEN program_id := NULL; END;

        -- Insert into temp table if valid
        IF program_id IS NOT NULL THEN
            BEGIN
                INSERT INTO temp_programs (program_id)
                VALUES (program_id)
                ON CONFLICT DO NOTHING;
            EXCEPTION WHEN OTHERS THEN
                RAISE WARNING 'Error processing program_id % for transaction %: %', 
                    program_id, NEW.txid, SQLERRM;
                CONTINUE;
            END;
        END IF;
    END LOOP;

    -- Batch update programs table
    BEGIN
        INSERT INTO programs (program_id)
        SELECT temp_programs.program_id FROM temp_programs
        ON CONFLICT ON CONSTRAINT programs_pkey DO UPDATE SET
            last_seen_at = CURRENT_TIMESTAMP,
            transaction_count = programs.transaction_count + 1;
    EXCEPTION WHEN OTHERS THEN
        RAISE WARNING 'Error updating programs for transaction %: %', NEW.txid, SQLERRM;
    END;

    -- Batch insert into transaction_programs
    BEGIN
        INSERT INTO transaction_programs (txid, program_id)
        SELECT NEW.txid, temp_programs.program_id FROM temp_programs
        ON CONFLICT DO NOTHING;
    EXCEPTION WHEN OTHERS THEN
        RAISE WARNING 'Error linking programs to transaction %: %', NEW.txid, SQLERRM;
    END;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger
CREATE TRIGGER transaction_programs_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_transaction_programs();

-- Update statistics
ANALYZE programs;
ANALYZE transaction_programs; 