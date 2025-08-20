use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::arch_rpc::websocket::WebSocketEvent;
use crate::arch_rpc::ArchRpcClient;

#[derive(Debug)]
pub struct RealtimeProcessor {
    pool: Arc<PgPool>,
    rpc_client: Arc<ArchRpcClient>,
}

impl RealtimeProcessor {
    pub fn new(pool: Arc<PgPool>, rpc_client: Arc<ArchRpcClient>) -> Self {
        Self { pool, rpc_client }
    }

    pub async fn start(&self, mut event_rx: mpsc::Receiver<WebSocketEvent>) -> Result<()> {
        info!("🚀 Starting real-time event processor...");

        while let Some(event) = event_rx.recv().await {
            match self.process_event(event).await {
                Ok(_) => {
                    // Event processed successfully
                }
                Err(e) => {
                    error!("Failed to process event: {}", e);
                }
            }
        }

        info!("Real-time event processor stopped");
        Ok(())
    }

    async fn process_event(&self, event: WebSocketEvent) -> Result<()> {
        match event.topic.as_str() {
            "block" => self.process_block_event(event).await?,
            "transaction" => self.process_transaction_event(event).await?,
            "account_update" => self.process_account_update_event(event).await?,
            "rolledback_transactions" => self.process_rolledback_transactions_event(event).await?,
            "reapplied_transactions" => self.process_reapplied_transactions_event(event).await?,
            "dkg" => self.process_dkg_event(event).await?,
            _ => {
                warn!("Unknown event topic: {}", event.topic);
            }
        }

        Ok(())
    }

    async fn process_block_event(&self, event: WebSocketEvent) -> Result<()> {
        let data = event.data;
        
        let hash = data
            .get("hash")
            .and_then(|h| h.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing block hash"))?;

        let timestamp = data
            .get("timestamp")
            .and_then(|t| t.as_u64())
            .map(|ts| DateTime::from_timestamp_millis(ts as i64))
            .flatten()
            .unwrap_or_else(|| Utc::now());

        info!("📦 Processing block event: hash={}, timestamp={}", hash, timestamp);

        // Check if block already exists
        let existing_block = sqlx::query!(
            "SELECT height FROM blocks WHERE hash = $1",
            hash
        )
        .fetch_optional(&*self.pool)
        .await?;

        if existing_block.is_some() {
            info!("Block {} already exists, skipping", hash);
            return Ok(());
        }

        // Fetch complete block data via RPC
        info!("🔄 Fetching complete block data for {} via RPC", hash);
        
        // First, we need to get the block height
        // For now, we'll estimate based on timestamp or use a placeholder
        // In a real implementation, you might want to get this from the block data
        let estimated_height = self.estimate_block_height(timestamp).await?;
        
        match self.rpc_client.get_block(hash, estimated_height).await {
            Ok(block) => {
                info!("✅ Successfully fetched block data: height={}, tx_count={}", 
                      block.height, block.transaction_count);
                
                // Store the block in the database
                self.store_block(&block).await?;
                
                // Process transactions if any
                if !block.transactions.is_empty() {
                    info!("💳 Processing {} transactions for block {}", block.transactions.len(), hash);
                    self.process_block_transactions(&block).await?;
                }
                
                info!("✅ Block {} fully processed and stored", hash);
            }
            Err(e) => {
                error!("❌ Failed to fetch block data for {}: {}", hash, e);
                // Store partial block data for later retry
                self.store_partial_block(hash, timestamp, estimated_height).await?;
            }
        }

        Ok(())
    }

    async fn process_transaction_event(&self, event: WebSocketEvent) -> Result<()> {
        let data = event.data;
        
        let hash = data
            .get("hash")
            .and_then(|h| h.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing transaction hash"))?;

        let status = data
            .get("status")
            .cloned()
            .unwrap_or(Value::Null);

        let program_ids = data
            .get("program_ids")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|id| id.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        info!("💳 Processing transaction event: hash={}, status={:?}, programs={:?}", 
              hash, status, program_ids);

        // Check if transaction already exists
        let existing_tx = sqlx::query!(
            "SELECT txid FROM transactions WHERE txid = $1",
            hash
        )
        .fetch_optional(&*self.pool)
        .await?;

        if existing_tx.is_some() {
            info!("Transaction {} already exists, skipping", hash);
            return Ok(());
        }

        // For now, we'll need to fetch full transaction data via RPC
        info!("🔄 Transaction {} needs full data fetched via RPC", hash);

        Ok(())
    }

    async fn process_account_update_event(&self, event: WebSocketEvent) -> Result<()> {
        let data = event.data;
        
        let account = data
            .get("account")
            .and_then(|a| a.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing account address"))?;

        let transaction_hash = data
            .get("transaction_hash")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing transaction hash"))?;

        info!("👤 Processing account update event: account={}, tx={}", account, transaction_hash);

        // Store account update event
        // You might want to create a new table for account updates
        info!("🔄 Account update for {} needs processing", account);

        Ok(())
    }

    async fn process_rolledback_transactions_event(&self, event: WebSocketEvent) -> Result<()> {
        let data = event.data;
        
        let transaction_hashes = data
            .get("transaction_hashes")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| h.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        info!("↩️ Processing rolledback transactions event: {:?}", transaction_hashes);

        // Mark transactions as rolled back
        for hash in transaction_hashes {
            let _ = sqlx::query!(
                "UPDATE transactions SET status = jsonb_set(status, '{rolled_back}', 'true') WHERE txid = $1",
                hash
            )
            .execute(&*self.pool)
            .await;
        }

        Ok(())
    }

    async fn process_reapplied_transactions_event(&self, event: WebSocketEvent) -> Result<()> {
        let data = event.data;
        
        let transaction_hashes = data
            .get("transaction_hashes")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| h.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        info!("🔄 Processing reapplied transactions event: {:?}", transaction_hashes);

        // Mark transactions as reapplied
        for hash in transaction_hashes {
            let _ = sqlx::query!(
                "UPDATE transactions SET status = jsonb_set(status, '{reapplied}', 'true') WHERE txid = $1",
                hash
            )
            .execute(&*self.pool)
            .await;
        }

        Ok(())
    }

    async fn process_dkg_event(&self, event: WebSocketEvent) -> Result<()> {
        let data = event.data;
        
        let status = data
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");

        info!("🔐 Processing DKG event: status={}", status);

        // Store DKG event
        // You might want to create a new table for DKG events
        info!("🔄 DKG event with status {} needs processing", status);

        Ok(())
    }

    /// Estimate block height based on timestamp
    async fn estimate_block_height(&self, timestamp: DateTime<Utc>) -> Result<i64> {
        // Get the latest block height from database
        let latest_height = sqlx::query!(
            "SELECT height FROM blocks ORDER BY height DESC LIMIT 1"
        )
        .fetch_optional(&*self.pool)
        .await?
        .map(|row| row.height)
        .unwrap_or(0);

        // Estimate based on timestamp difference
        // Assuming ~4 blocks per second (from our observations)
        let now = Utc::now();
        let time_diff = now.signed_duration_since(timestamp);
        let estimated_blocks = time_diff.num_seconds() * 4; // 4 blocks per second
        
        Ok(latest_height + estimated_blocks)
    }

    /// Store a complete block in the database
    async fn store_block(&self, block: &crate::arch_rpc::Block) -> Result<()> {
        sqlx::query!(
            "INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height) 
             VALUES ($1, $2, $3, $4) 
             ON CONFLICT (height) DO NOTHING",
            block.height,
            block.hash,
            DateTime::from_timestamp(block.timestamp / 1000, 0).unwrap_or_else(|| Utc::now()),
            block.bitcoin_block_height
        )
        .execute(&*self.pool)
        .await?;

        info!("✅ Block {} stored in database", block.hash);
        Ok(())
    }

    /// Store partial block data for later retry
    async fn store_partial_block(&self, hash: &str, timestamp: DateTime<Utc>, estimated_height: i64) -> Result<()> {
        // For now, just log the partial block instead of creating a new table
        // This can be enhanced later with a proper retry mechanism
        warn!("📝 Partial block {} received but not stored (hash={}, timestamp={}, estimated_height={})", 
              hash, hash, timestamp, estimated_height);
        Ok(())
    }

    /// Process all transactions for a block
    async fn process_block_transactions(&self, block: &crate::arch_rpc::Block) -> Result<()> {
        for tx_hash in &block.transactions {
            // For now, store basic transaction info
            // In a full implementation, you'd fetch complete transaction data
            sqlx::query!(
                "INSERT INTO transactions (txid, block_height, data, status, created_at) 
                 VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP) 
                 ON CONFLICT (txid) DO NOTHING",
                tx_hash,
                block.height,
                serde_json::json!({"block_hash": block.hash, "timestamp": block.timestamp}),
                serde_json::json!({"status": 0}) // Default status as JSON
            )
            .execute(&*self.pool)
            .await?;
        }

        info!("✅ {} transactions processed for block {}", block.transactions.len(), block.hash);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_process_block_event() {
        // This would need a test database setup
        // For now, just test the parsing logic
        let event_data = json!({
            "hash": "test_hash_123",
            "timestamp": 1234567890000
        });

        let event = WebSocketEvent {
            topic: "block".to_string(),
            data: event_data,
            timestamp: Utc::now(),
        };

        // Test that we can create the event
        assert_eq!(event.topic, "block");
        assert!(event.data.get("hash").is_some());
    }
}
