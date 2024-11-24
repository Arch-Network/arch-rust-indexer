-- Step 1: Remove the default value
ALTER TABLE transactions
ALTER COLUMN status DROP DEFAULT;

-- Step 2: Alter the column type to JSONB
ALTER TABLE transactions
ALTER COLUMN status TYPE JSONB USING to_jsonb(status);

-- Step 3: Set a new JSONB default value
ALTER TABLE transactions
ALTER COLUMN status SET DEFAULT '0'::jsonb;