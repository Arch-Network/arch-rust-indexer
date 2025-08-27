-- Add display_name to programs for friendly labels
ALTER TABLE programs ADD COLUMN IF NOT EXISTS display_name TEXT;

-- Optional index for searching by name (case-insensitive)
CREATE INDEX IF NOT EXISTS idx_programs_display_name_ci ON programs (lower(display_name));

-- Seed friendly names for known built-ins (by canonical hex)
WITH labels(program_id, display_name) AS (
    VALUES
        ('0000000000000000000000000000000000000000000000000000000000000001', 'System Program'),
        (encode(convert_to('BpfLoader11111111111111111111111','UTF8'),'hex'), 'BPF Loader'),
        (encode(convert_to('NativeLoader11111111111111111111','UTF8'),'hex'), 'Native Loader'),
        (encode(convert_to('ComputeBudget1111111111111111111','UTF8'),'hex'), 'Compute Budget Program'),
        (encode(convert_to('StakeProgram11111111111111111111','UTF8'),'hex'), 'Stake Program'),
        (encode(convert_to('VoteProgram111111111111111111111','UTF8'),'hex'), 'Vote Program'),
        (encode(convert_to('AplToken111111111111111111111111','UTF8'),'hex'), 'APL Token Program'),
        (encode(convert_to('AssociatedTokenAccount1111111111','UTF8'),'hex'), 'Associated Token Account')
)
UPDATE programs p
SET display_name = l.display_name
FROM labels l
WHERE p.program_id = l.program_id AND (p.display_name IS NULL OR p.display_name = '');
