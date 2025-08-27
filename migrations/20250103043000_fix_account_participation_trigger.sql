-- Replace populate_account_participation to handle byte-array and string keys via normalize_program_id(jsonb)

CREATE OR REPLACE FUNCTION populate_account_participation()
RETURNS TRIGGER AS $$
DECLARE
    addr_hex TEXT;
BEGIN
    FOR addr_hex IN
        SELECT normalize_program_id(v)
        FROM jsonb_array_elements(COALESCE(NEW.data#>'{message,account_keys}', '[]'::jsonb)) AS v
    LOOP
        IF addr_hex IS NOT NULL THEN
            BEGIN
                INSERT INTO account_participation(address_hex, txid, block_height, created_at)
                VALUES (addr_hex, NEW.txid, NEW.block_height, NEW.created_at)
                ON CONFLICT DO NOTHING;
            EXCEPTION WHEN OTHERS THEN
                CONTINUE;
            END;
        END IF;
    END LOOP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Ensure trigger fires on both INSERT and UPDATE
DROP TRIGGER IF EXISTS account_participation_trigger ON transactions;
CREATE TRIGGER account_participation_trigger
AFTER INSERT OR UPDATE ON transactions
FOR EACH ROW EXECUTE FUNCTION populate_account_participation();

