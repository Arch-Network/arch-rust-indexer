-- Add program tracking tables
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

-- Add indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_transaction_programs_program_id 
    ON transaction_programs(program_id);
CREATE INDEX IF NOT EXISTS idx_programs_last_seen 
    ON programs(last_seen_at);
CREATE INDEX IF NOT EXISTS idx_programs_transaction_count 
    ON programs(transaction_count DESC);

-- Add function to extract and update program IDs from transaction data
CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
BEGIN
    -- Insert program IDs from the transaction data
    WITH program_ids AS (
        SELECT DISTINCT jsonb_array_elements(
            CASE 
                WHEN jsonb_typeof(NEW.data->'message'->'instructions') = 'array' 
                THEN NEW.data->'message'->'instructions'
                ELSE '[]'::jsonb
            END
        )->>'program_id' as pid
        WHERE jsonb_typeof(NEW.data->'message'->'instructions') = 'array'
    )
    INSERT INTO programs (program_id)
    SELECT pid FROM program_ids WHERE pid IS NOT NULL
    ON CONFLICT (program_id) 
    DO UPDATE SET 
        last_seen_at = CURRENT_TIMESTAMP,
        transaction_count = programs.transaction_count + 1;

    -- Link programs to transaction
    INSERT INTO transaction_programs (txid, program_id)
    SELECT NEW.txid, pid
    FROM program_ids
    WHERE pid IS NOT NULL;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger to automatically update program information
CREATE TRIGGER transaction_programs_trigger
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_transaction_programs();