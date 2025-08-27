-- Provide a canonical resolver that maps ASCII alias labels to fixed program IDs

CREATE OR REPLACE FUNCTION canonical_program_id(v jsonb)
RETURNS text AS $$
DECLARE
    hex text;
    label text;
BEGIN
    -- First, derive hex using existing normalization
    hex := normalize_program_id(v);
    IF hex IS NULL THEN
        RETURN NULL;
    END IF;

    -- Try to interpret bytes as UTF-8 label
    BEGIN
        label := convert_from(decode(hex, 'hex'), 'UTF8');
    EXCEPTION WHEN others THEN
        label := NULL;
    END;

    IF label IS NOT NULL THEN
        IF label LIKE 'spl-token%' OR label LIKE 'spl_token%' THEN
            RETURN normalize_program_id('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA');
        ELSIF label LIKE 'apl-token%' OR label LIKE 'apl_token%' THEN
            RETURN normalize_program_id('AplToken111111111111111111111111');
        ELSIF label LIKE 'spl-associated-token-account%' OR label LIKE 'spl_associated_token_account%' THEN
            -- SPL associated token account program (for completeness)
            RETURN normalize_program_id('ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL');
        ELSIF label LIKE 'apl-associated-token-account%' OR label LIKE 'apl_associated_token_account%' THEN
            RETURN normalize_program_id('AssociatedTokenAccount1111111111');
        END IF;
    END IF;

    -- Default: return the hex derived id
    RETURN hex;
END;
$$ LANGUAGE plpgsql IMMUTABLE;
