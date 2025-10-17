-- Migration: Add Mempool Transaction Tracking (idempotent)
-- Mirrors migrations/20241215000001_add_mempool_tracking.sql

-- Create mempool transactions table
CREATE TABLE IF NOT EXISTS mempool_transactions (
    txid TEXT PRIMARY KEY,
    data JSONB NOT NULL,
    added_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    fee_priority INTEGER,
    size_bytes INTEGER,
    status TEXT DEFAULT 'pending'
);

-- Create indexes for mempool table
CREATE INDEX IF NOT EXISTS idx_mempool_added_at ON mempool_transactions (added_at);
CREATE INDEX IF NOT EXISTS idx_mempool_fee_priority ON mempool_transactions (fee_priority);
CREATE INDEX IF NOT EXISTS idx_mempool_status ON mempool_transactions (status);
CREATE INDEX IF NOT EXISTS idx_mempool_size ON mempool_transactions (size_bytes);

-- Create view for mempool statistics
CREATE OR REPLACE VIEW mempool_stats AS
SELECT 
    COUNT(*) as total_transactions,
    COUNT(*) FILTER (WHERE status = 'pending') as pending_count,
    COUNT(*) FILTER (WHERE status = 'confirmed') as confirmed_count,
    AVG(fee_priority) as avg_fee_priority,
    AVG(size_bytes) as avg_size_bytes,
    SUM(size_bytes) as total_size_bytes,
    MIN(added_at) as oldest_transaction,
    MAX(added_at) as newest_transaction
FROM mempool_transactions;

-- Analyze the new table
ANALYZE mempool_transactions;
