-- Create a temporary table to store the mappings
CREATE TEMP TABLE transaction_program_fixes AS
WITH program_mappings AS (
    SELECT DISTINCT
        tp.program_id as old_id,
        normalize_program_id(tp.program_id) as new_id
    FROM transaction_programs tp
    WHERE normalize_program_id(tp.program_id) IS NOT NULL
        AND tp.program_id != normalize_program_id(tp.program_id)
)
SELECT * FROM program_mappings;

-- Update transaction_programs with normalized program_ids
UPDATE transaction_programs tp
SET program_id = f.new_id
FROM transaction_program_fixes f
WHERE tp.program_id = f.old_id;

-- Clean up
DROP TABLE transaction_program_fixes;