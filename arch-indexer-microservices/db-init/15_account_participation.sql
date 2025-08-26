-- Account participation table to accelerate account pages
CREATE TABLE IF NOT EXISTS account_participation (
    address_hex TEXT NOT NULL,
    txid TEXT NOT NULL REFERENCES transactions(txid) ON DELETE CASCADE,
    block_height BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (address_hex, txid)
);

CREATE INDEX IF NOT EXISTS idx_account_participation_address ON account_participation(address_hex);
CREATE INDEX IF NOT EXISTS idx_account_participation_created_at ON account_participation(created_at DESC);

-- Trigger to populate account participation on insert of transactions
CREATE OR REPLACE FUNCTION populate_account_participation()
RETURNS TRIGGER AS $$
DECLARE
    acc TEXT;
BEGIN
    FOR acc IN
        SELECT encode(k::bytea, 'hex')
        FROM jsonb_array_elements_text(NEW.data->'message'->'account_keys') k
    LOOP
        BEGIN
            INSERT INTO account_participation(address_hex, txid, block_height, created_at)
            VALUES (acc, NEW.txid, NEW.block_height, NEW.created_at)
            ON CONFLICT DO NOTHING;
        EXCEPTION WHEN OTHERS THEN
            CONTINUE;
        END;
    END LOOP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS account_participation_trigger ON transactions;
CREATE TRIGGER account_participation_trigger
AFTER INSERT ON transactions
FOR EACH ROW EXECUTE FUNCTION populate_account_participation();
