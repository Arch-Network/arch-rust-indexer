-- Migration: Enhance Transaction and Block Data Capture
-- Date: 2024-12-15
-- Description: Add missing fields to capture comprehensive transaction and block data

-- Add missing transaction fields
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS logs JSONB DEFAULT '[]'::jsonb;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS rollback_status JSONB DEFAULT '"NotRolledback"'::jsonb;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS accounts_tags JSONB DEFAULT '[]'::jsonb;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS compute_units_consumed INTEGER;

-- Add missing block fields
ALTER TABLE blocks ADD COLUMN IF NOT EXISTS merkle_root TEXT;
ALTER TABLE blocks ADD COLUMN IF NOT EXISTS previous_block_hash TEXT;

-- Create indexes for new fields
CREATE INDEX IF NOT EXISTS idx_transactions_logs ON transactions USING GIN (logs);
CREATE INDEX IF NOT EXISTS idx_transactions_rollback_status ON transactions USING GIN (rollback_status);
CREATE INDEX IF NOT EXISTS idx_transactions_accounts_tags ON transactions USING GIN (accounts_tags);
CREATE INDEX IF NOT EXISTS idx_transactions_compute_units ON transactions (compute_units_consumed);
CREATE INDEX IF NOT EXISTS idx_blocks_merkle_root ON blocks (merkle_root);
CREATE INDEX IF NOT EXISTS idx_blocks_previous_hash ON blocks (previous_block_hash);

-- Update existing transactions to have default values
UPDATE transactions SET 
    logs = '[]'::jsonb,
    rollback_status = '"NotRolledback"'::jsonb,
    accounts_tags = '[]'::jsonb
WHERE logs IS NULL OR rollback_status IS NULL OR accounts_tags IS NULL;

-- Analyze tables after changes
ANALYZE transactions;
ANALYZE blocks;
