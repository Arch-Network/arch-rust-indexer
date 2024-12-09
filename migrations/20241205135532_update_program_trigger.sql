CREATE OR REPLACE FUNCTION update_transaction_programs()
RETURNS TRIGGER AS $$
BEGIN
    WITH RECURSIVE program_ids AS (
        SELECT DISTINCT 
            CASE 
                WHEN jsonb_typeof(inst.value->'program_id') = 'string' THEN 
                    encode(decode(inst.value->>'program_id', 'base58'), 'hex')
                WHEN jsonb_typeof(inst.value->'program_id') = 'array' THEN 
                    encode(
                        decode(
                            string_agg(
                                lpad(
                                    CASE 
                                        WHEN (v::text)::int < 0 THEN ((v::text)::int + 256)::text
                                        ELSE (v::text)::int::text
                                    END,
                                    2, '0'
                                ),
                                ''
                            ),
                            'hex'
                        ),
                        'hex'
                    )
                ELSE NULL
            END as pid
        FROM jsonb_array_elements(
            CASE 
                WHEN jsonb_typeof(NEW.data#>'{message,instructions}') = 'array' 
                THEN NEW.data#>'{message,instructions}'
                ELSE '[]'::jsonb
            END
        ) inst,
        LATERAL jsonb_array_elements_text(
            CASE 
                WHEN jsonb_typeof(inst.value->'program_id') = 'array' 
                THEN inst.value->'program_id'
                ELSE '[]'::jsonb
            END
        ) v
        WHERE inst.value->>'program_id' IS NOT NULL
    )
    INSERT INTO programs (program_id)
    SELECT pid FROM program_ids 
    WHERE pid IS NOT NULL
    ON CONFLICT (program_id) 
    DO UPDATE SET 
        last_seen_at = CURRENT_TIMESTAMP,
        transaction_count = programs.transaction_count + 1;

    INSERT INTO transaction_programs (txid, program_id)
    SELECT NEW.txid, pid
    FROM program_ids
    WHERE pid IS NOT NULL
    ON CONFLICT DO NOTHING;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;