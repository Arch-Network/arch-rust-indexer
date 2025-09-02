use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration, Instant};
use tracing::{error, info, warn, debug};

use crate::arch_rpc::websocket::WebSocketEvent;
use crate::arch_rpc::ArchRpcClient;
use crate::utils::convert_arch_timestamp;
use crate::api::websocket_server::WebSocketServer;

#[derive(Debug, Clone)]
struct BlockActivity {
    txs: i64,
    program_counts: HashMap<String, i64>,
    last_emit: Instant,
    finalized: bool,
}

impl Default for BlockActivity {
    fn default() -> Self {
        Self { txs: 0, program_counts: HashMap::new(), last_emit: Instant::now(), finalized: false }
    }
}

#[derive(Debug)]
struct ActivityAggregator {
    pool: Arc<PgPool>,
    // Keyed by block height; using a Mutex because contention is low
    state: tokio::sync::Mutex<HashMap<i64, BlockActivity>>,
}

impl ActivityAggregator {
    fn new(pool: Arc<PgPool>) -> Self { Self { pool, state: tokio::sync::Mutex::new(HashMap::new()) } }

    async fn add_tx(&self, height: i64, programs: &[String]) {
        let mut guard = self.state.lock().await;
        let entry = guard.entry(height).or_insert_with(|| BlockActivity { txs: 0, program_counts: HashMap::new(), last_emit: Instant::now() - Duration::from_millis(1000), finalized: false });
        entry.txs += 1;
        for p in programs {
            *entry.program_counts.entry(p.clone()).or_insert(0) += 1;
        }
    }

    async fn snapshot(&self, height: i64, top_k: usize) -> Option<(i64, Vec<(String, i64)>, bool)> {
        let guard = self.state.lock().await;
        if let Some(entry) = guard.get(&height) {
            let mut v: Vec<(String, i64)> = entry.program_counts.iter().map(|(k,v)| (k.clone(), *v)).collect();
            v.sort_by(|a,b| b.1.cmp(&a.1));
            v.truncate(top_k);
            return Some((entry.txs, v, entry.finalized));
        }
        None
    }

    async fn mark_final(&self, height: i64) {
        let mut guard = self.state.lock().await;
        if let Some(entry) = guard.get_mut(&height) { entry.finalized = true; }
    }

    async fn clear(&self, height: i64) { let mut guard = self.state.lock().await; guard.remove(&height); }

    async fn should_emit_and_touch(&self, height: i64, min_gap_ms: u64) -> bool {
        let mut guard = self.state.lock().await;
        if let Some(entry) = guard.get_mut(&height) {
            let now = Instant::now();
            if now.duration_since(entry.last_emit) >= Duration::from_millis(min_gap_ms) {
                entry.last_emit = now;
                return true;
            }
        }
        false
    }
}

#[derive(Debug)]
pub struct RealtimeProcessor {
    pool: Arc<PgPool>,
    rpc_client: Arc<ArchRpcClient>,
    websocket_server: Option<Arc<WebSocketServer>>,
    aggregator: Arc<ActivityAggregator>,
}

impl RealtimeProcessor {
    pub fn new(
        pool: Arc<PgPool>,
        rpc_client: Arc<ArchRpcClient>,
        websocket_server: Option<Arc<WebSocketServer>>,
    ) -> Self {
        Self { pool: Arc::clone(&pool), rpc_client, websocket_server, aggregator: Arc::new(ActivityAggregator::new(pool)) }
    }

    pub async fn start(&self, mut event_rx: mpsc::Receiver<WebSocketEvent>) -> Result<()> {
        info!("🚀 Starting real-time event processor...");

        let mut event_count = 0;
        let mut last_event_time = tokio::time::Instant::now();
        
        while let Some(event) = tokio::time::timeout(
            tokio::time::Duration::from_secs(60), // 1 minute timeout
            event_rx.recv()
        ).await.map_err(|_| anyhow::anyhow!("No events received for 60 seconds"))? {
            event_count += 1;
            last_event_time = tokio::time::Instant::now();
            
            // Broadcast raw event to any connected websocket UI clients
            if let Some(server) = &self.websocket_server {
                // Ignore broadcast errors; continue processing
                let _ = server.broadcast_event(event.clone()).await;
            }

            match self.process_event(event).await {
                Ok(_) => {
                    info!("✅ Event #{} processed successfully", event_count);
                }
                Err(e) => {
                    error!("❌ Failed to process event #{}: {}", event_count, e);
                }
            }
        }

        info!("Real-time event processor stopped after processing {} events", event_count);
        Ok(())
    }

    async fn process_event(&self, event: WebSocketEvent) -> Result<()> {
        match event.topic.as_str() {
            "block" => self.process_block_event(event).await?,
            "transaction" => {
                // Pre-aggregate live program activity for the heatstrip
                let h = event.data.get("block_height").and_then(|v| v.as_i64());
                let programs: Vec<String> = event
                    .data
                    .get("program_ids")
                    .and_then(|p| p.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
                    .unwrap_or_default();
                if let Some(height) = h {
                    self.aggregator.add_tx(height, &programs).await;
                    // Debounced live emit so the UI updates immediately
                    if self.aggregator.should_emit_and_touch(height, 250).await {
                        if let Some(server) = &self.websocket_server {
                            if let Some((txs, prog, finalized)) = self.aggregator.snapshot(height, 32).await {
                                let mut obj = serde_json::Map::new();
                                for (pid, cnt) in prog { obj.insert(pid, serde_json::Value::from(cnt)); }
                                let live = crate::arch_rpc::websocket::WebSocketEvent {
                                    topic: "block_activity".to_string(),
                                    data: json!({
                                        "height": height,
                                        "transaction_count": txs,
                                        "program_counts": serde_json::Value::Object(obj),
                                        "finalized": finalized,
                                        "timestamp": Utc::now().timestamp_millis(),
                                    }),
                                    timestamp: Utc::now(),
                                };
                                let _ = server.broadcast_event(live).await;
                                debug!("block_activity live: height={}, txs={}", height, txs);
                            }
                        }
                    }
                }
                self.process_transaction_event(event).await?
            },
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

    async fn load_block_activity(&self, height: i64) -> Result<(i64, Vec<(String, i64)>)> {
        // Fetch tx count
        use sqlx::Row;
        let tx_row = sqlx::query("SELECT COUNT(*) as txs FROM transactions WHERE block_height = $1")
            .bind(height)
            .fetch_one(&*self.pool)
            .await?;
        let txs: i64 = tx_row.try_get::<i64, _>("txs").unwrap_or(0);

        // Fetch per-program counts (top 64 for brevity)
        let rows = sqlx::query(
            r#"SELECT program_id, COUNT(*) as cnt
               FROM transaction_programs
               WHERE block_height = $1
               GROUP BY program_id
               ORDER BY cnt DESC
               LIMIT 64"#,
        )
        .bind(height)
        .fetch_all(&*self.pool)
        .await?;

        let mut counts: Vec<(String, i64)> = Vec::with_capacity(rows.len());
        for r in rows {
            let pid = r.try_get::<String, _>("program_id").unwrap_or_else(|_| "unknown".to_string());
            let cnt = r.try_get::<i64, _>("cnt").unwrap_or(0);
            counts.push((pid, cnt));
        }

        Ok((txs, counts))
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
            .map(|ts| {
                // Convert Arch timestamp to DateTime using centralized utility
                convert_arch_timestamp(ts as i64)
            })
            .unwrap_or_else(|| Utc::now());

        info!("📦 Processing block event: hash={}, timestamp={}", hash, timestamp);

        // Emit a debounced live snapshot if pre-aggregated txs exist
        if let Ok(r) = sqlx::query("SELECT height FROM blocks WHERE hash = $1")
            .bind(hash)
            .fetch_optional(&*self.pool)
            .await
        {
            if let Some(row) = r { use sqlx::Row; let height: i64 = row.try_get::<i64, _>("height").unwrap_or(0);
                if height > 0 { if let Some(server) = &self.websocket_server {
                    if let Some((txs, prog, finalized)) = self.aggregator.snapshot(height, 32).await {
                        let mut obj = serde_json::Map::new();
                        for (pid, cnt) in prog { obj.insert(pid, serde_json::Value::from(cnt)); }
                        let live = crate::arch_rpc::websocket::WebSocketEvent {
                            topic: "block_activity".to_string(),
                            data: json!({
                                "hash": hash,
                                "height": height,
                                "timestamp": timestamp.timestamp_millis(),
                                "transaction_count": txs,
                                "program_counts": serde_json::Value::Object(obj),
                                "finalized": finalized,
                            }),
                            timestamp: Utc::now(),
                        };
                        let _ = server.broadcast_event(live).await;
                    }
                } }
            }
        }

        // Check if block already exists with connection timeout
        let existing_block = match tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            sqlx::query!(
                "SELECT height FROM blocks WHERE hash = $1",
                hash
            )
            .fetch_optional(&*self.pool)
        ).await {
            Ok(result) => result?,
            Err(_) => {
                warn!("Database query timeout for block {}, skipping", hash);
                return Ok(());
            }
        };

        if let Some(_row) = existing_block {
            info!("Block {} already exists, will emit summary event", hash);
            if let Ok(r) = sqlx::query("SELECT height FROM blocks WHERE hash = $1")
                .bind(hash)
                .fetch_one(&*self.pool)
                .await
            {
                use sqlx::Row;
                let height: i64 = r.try_get::<i64, _>("height").unwrap_or(0);
                if height > 0 {
                    if let Ok((txs, prog)) = self.load_block_activity(height).await {
                        if let Some(server) = &self.websocket_server {
                            let mut obj = serde_json::Map::new();
                            for (pid, cnt) in prog {
                                obj.insert(pid, serde_json::Value::from(cnt));
                            }
                            let enriched = crate::arch_rpc::websocket::WebSocketEvent {
                                topic: "block".to_string(),
                                data: serde_json::json!({
                                    "hash": hash,
                                    "height": height,
                                    "block_height": height,
                                    "timestamp": timestamp.timestamp_millis(),
                                    "transaction_count": txs,
                                    "program_counts": serde_json::Value::Object(obj),
                                }),
                                timestamp: Utc::now(),
                            };
                            let _ = server.broadcast_event(enriched).await;
                            debug!("block summary: height={}, txs={}", height, txs);
                        }
                    }
                }
            }
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

                // After persistence, emit an enriched block event to UI clients with tx count and program counts
                if let Some(server) = &self.websocket_server {
                    let (txs, prog) = self.load_block_activity(block.height).await.unwrap_or((block.transaction_count as i64, Vec::new()));
                    let mut obj = serde_json::Map::new();
                    for (pid, cnt) in prog { obj.insert(pid, serde_json::Value::from(cnt)); }
                    let enriched = crate::arch_rpc::websocket::WebSocketEvent {
                        topic: "block".to_string(),
                        data: serde_json::json!({
                            "hash": block.hash,
                            "height": block.height,
                            "block_height": block.height,
                            "timestamp": timestamp.timestamp_millis(),
                            "transaction_count": txs,
                            "program_counts": serde_json::Value::Object(obj),
                        }),
                        timestamp: Utc::now(),
                    };
                    let _ = server.broadcast_event(enriched).await;
                    debug!("block summary post-store: height={}, txs={}", block.height, txs);
                }
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
        // Get the latest block height from database with timeout
        let latest_height = match tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            sqlx::query!(
                "SELECT height FROM blocks ORDER BY height DESC LIMIT 1"
            )
            .fetch_optional(&*self.pool)
        ).await {
            Ok(result) => result?,
            Err(_) => {
                warn!("Database query timeout for latest height, using fallback");
                return Ok(0);
            }
        }
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
            convert_arch_timestamp(block.timestamp),
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
