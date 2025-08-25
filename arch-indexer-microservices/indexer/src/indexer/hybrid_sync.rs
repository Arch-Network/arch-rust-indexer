use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use tokio::time::Duration;
use tracing::{error, info};

use crate::config::Settings;
use sqlx::PgPool;

use crate::arch_rpc::ArchRpcClient;
use crate::arch_rpc::websocket::WebSocketClient;
use crate::utils::convert_arch_timestamp;
use serde_json::Value as JsonValue;

#[derive(Debug, Clone)]
pub struct HybridSync {
    settings: Arc<Settings>,
    pool: Arc<PgPool>,
    current_height: Arc<AtomicI64>,
    is_realtime_active: Arc<AtomicBool>,
    last_realtime_update: Arc<AtomicI64>,
}

impl HybridSync {
    pub fn new(settings: Arc<Settings>, pool: Arc<PgPool>) -> Self {
        Self {
            settings,
            pool,
            current_height: Arc::new(AtomicI64::new(0)),
            is_realtime_active: Arc::new(AtomicBool::new(false)),
            last_realtime_update: Arc::new(AtomicI64::new(0)),
        }
    }

    pub fn is_websocket_enabled(&self) -> bool {
        self.settings.websocket.enabled && self.settings.indexer.enable_realtime
    }

    pub async fn start(&self) -> Result<()> {
        info!("🚀 Starting Hybrid Sync Manager...");

        if self.settings.indexer.enable_realtime && self.settings.websocket.enabled {
            info!("✅ Real-time WebSocket sync enabled");
            if let Err(e) = self.start_realtime_sync().await {
                error!("Failed to start real-time sync: {}", e);
            }
        } else {
            info!("⚠️ Real-time sync disabled, using traditional polling only");
        }

        // Always start traditional sync as fallback
        info!("🔄 Starting traditional sync...");
        if let Err(e) = self.start_traditional_sync().await {
            error!("Failed to start traditional sync: {}", e);
        } else {
            info!("✅ Traditional sync started successfully");
        }

        Ok(())
    }

    async fn start_realtime_sync(&self) -> Result<()> {
        info!("🔄 Starting start_realtime_sync method...");
        let websocket_url = self.settings.arch_node.websocket_url.clone();
        let websocket_settings = self.settings.websocket.clone();
        let rpc = Arc::new(ArchRpcClient::new(self.settings.arch_node.url.clone()));
        let pool = Arc::clone(&self.pool);
        let is_realtime_active = Arc::clone(&self.is_realtime_active);
        let last_realtime_update = Arc::clone(&self.last_realtime_update);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<crate::arch_rpc::websocket::WebSocketEvent>();
        let ws_client = WebSocketClient::new(websocket_settings, websocket_url);

        // Connection task
        tokio::spawn(async move {
            if let Err(e) = ws_client.connect_and_listen(tx).await {
                error!("WebSocket client exited with error: {}", e);
            }
        });

        // Event processor
        tokio::spawn(async move {
            while let Some(evt) = rx.recv().await {
                // Mark realtime active
                is_realtime_active.store(true, Ordering::Relaxed);
                last_realtime_update.store(chrono::Utc::now().timestamp(), Ordering::Relaxed);

                match evt.topic.as_str() {
                    "block" => {
                        // On block event, fetch latest block via RPC if hash present
                        if let Some(hash) = evt.data.get("hash").and_then(|v| v.as_str()) {
                            // Height may not be present; attempt to get height from tip for now
                            // or we could ignore and let bulk catch up. We'll attempt fetch by hash only.
                            match rpc.get_block(hash, 0).await {
                                Ok(block) => {
                                    // Convert Arch timestamp to DateTime using centralized utility
                                    let timestamp = convert_arch_timestamp(block.timestamp);
                                    if let Err(e) = sqlx::query(
                                        r#"
                                        INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height)
                                        VALUES ($1, $2, $3, $4)
                                        ON CONFLICT (height) DO UPDATE 
                                        SET hash = EXCLUDED.hash, timestamp = EXCLUDED.timestamp, bitcoin_block_height = EXCLUDED.bitcoin_block_height
                                        "#,
                                    )
                                    .bind(block.height)
                                    .bind(&block.hash)
                                    .bind(timestamp)
                                    .bind(block.bitcoin_block_height.unwrap_or(0))
                                    .execute(&*pool)
                                    .await {
                                        error!("Realtime block upsert failed: {}", e);
                                    }
                                }
                                Err(e) => error!("Realtime failed to fetch block by hash: {}", e),
                            }
                        }
                    }
                    "transaction" => {
                        if let Some(hash) = evt.data.get("hash").and_then(|v| v.as_str()) {
                            match rpc.get_processed_transaction(hash).await {
                                Ok(processed) => {
                                    let data = match serde_json::to_value(&processed.runtime_transaction) { Ok(v)=>v, Err(_)=>serde_json::Value::Null };
                                    let status = match serde_json::to_value(&processed.status) { Ok(v)=>v, Err(_)=>serde_json::Value::Null };
                                    let bitcoin_txids: Option<&[String]> = processed.bitcoin_txids.as_deref();
                                    if let Err(e) = sqlx::query(
                                        r#"
                                        INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids, created_at)
                                        VALUES ($1, COALESCE((SELECT MAX(height) FROM blocks), 0), $2, $3, $4, CURRENT_TIMESTAMP)
                                        ON CONFLICT (txid) DO UPDATE SET data = $2, status = $3, bitcoin_txids = $4
                                        "#,
                                    )
                                    .bind(hash)
                                    .bind(&data)
                                    .bind(status)
                                    .bind(bitcoin_txids)
                                    .execute(&*pool)
                                    .await {
                                        error!("Realtime tx upsert failed: {}", e);
                                    }

                                    // Extract and upsert program IDs
                                    let pids = extract_program_ids(&data, Some(&processed.accounts_tags));
                                    for pid in pids {
                                        if let Err(e) = sqlx::query(
                                            r#"
                                            INSERT INTO programs (program_id, first_seen_at, last_seen_at, transaction_count)
                                            VALUES ($1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 1)
                                            ON CONFLICT (program_id) DO UPDATE
                                            SET last_seen_at = CURRENT_TIMESTAMP,
                                                transaction_count = programs.transaction_count + 1
                                            "#
                                        )
                                        .bind(&pid)
                                        .execute(&*pool)
                                        .await { error!("programs upsert failed: {}", e); }

                                        if let Err(e) = sqlx::query(
                                            r#"
                                            INSERT INTO transaction_programs (txid, program_id)
                                            VALUES ($1, $2)
                                            ON CONFLICT DO NOTHING
                                            "#
                                        )
                                        .bind(hash)
                                        .bind(&pid)
                                        .execute(&*pool)
                                        .await { error!("transaction_programs insert failed: {}", e); }
                                    }
                                }
                                Err(e) => error!("Realtime failed to fetch transaction {}: {}", hash, e),
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        info!("✅ start_realtime_sync method completed successfully");
        Ok(())
    }

    async fn start_traditional_sync(&self) -> Result<()> {
        info!("🔄 Starting start_traditional_sync method...");
        let pool = Arc::clone(&self.pool);
        let settings = Arc::clone(&self.settings);

        tokio::spawn(async move {
            let rpc = Arc::new(ArchRpcClient::new(settings.arch_node.url.clone()));
            info!("🌐 Bulk sync using RPC endpoint: {}", settings.arch_node.url);

            // Determine starting height
            let last_height: Option<i64> = sqlx::query_scalar("SELECT MAX(height) FROM blocks")
                .fetch_optional(&*pool)
                .await
                .ok()
                .flatten();
            let mut start_height = last_height.unwrap_or(-1) + 1;

            // Fetch current tip
            let mut tip = match rpc.get_block_count().await {
                Ok(h) => h,
                Err(e) => { error!("Failed to fetch block count: {}", e); return; }
            };

            // If DB height is ahead of node tip, reset to 0 and log
            if start_height > tip {
                info!("DB height {} ahead of node tip {}. Resetting start height to 0.", start_height, tip);
                start_height = 0;
            }

            // Full reindex from the beginning; do not fast-forward

            info!("📈 Bulk sync starting at {} up to {}", start_height, tip);

            loop {
                if start_height > tip { // refresh tip and wait briefly
                    match rpc.get_block_count().await { Ok(h) => tip = h, Err(e) => error!("get_block_count error: {}", e) }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }

                // Process in batches
                let batch_size = 25;
                let end = (start_height + batch_size as i64 - 1).min(tip);
                info!("📦 Processing blocks {}..{}", start_height, end);

                for h in start_height..=end {
                    if let Err(e) = process_block_via_rpc(&pool, &rpc, h).await {
                        error!("Block {} failed: {}", h, e);
                        // backoff before retrying next iteration
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }

                start_height = end + 1;
            }
        });

        Ok(())
    }

    pub fn get_current_height(&self) -> i64 {
        self.current_height.load(Ordering::Relaxed)
    }

    pub fn is_realtime_active(&self) -> bool {
        self.is_realtime_active.load(Ordering::Relaxed)
    }

    pub fn get_last_realtime_update(&self) -> i64 {
        self.last_realtime_update.load(Ordering::Relaxed)
    }
}

fn extract_program_ids(data: &JsonValue, accounts_tags: Option<&[JsonValue]>) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    // Preload account_keys as hex strings if available
    let mut account_keys_hex: Vec<String> = Vec::new();
    if let Some(keys) = data
        .get("message")
        .and_then(|m| m.get("account_keys"))
        .and_then(|v| v.as_array())
    {
        for k in keys {
            // account key may be array of numbers (bytes)
            if let Some(arr) = k.as_array() {
                let bytes: Vec<u8> = arr.iter().filter_map(|x| x.as_u64().map(|n| n as u8)).collect();
                account_keys_hex.push(hex::encode(bytes));
            } else if let Some(s) = k.as_str() { // or hex string
                account_keys_hex.push(s.to_string());
            }
        }
    }

    if let Some(msg) = data.get("message") {
        if let Some(instructions) = msg.get("instructions").and_then(|v| v.as_array()) {
            for ins in instructions {
                if let Some(pid) = ins.get("program_id").and_then(|v| v.as_str()) {
                    ids.push(pid.to_string());
                    continue;
                }
                if let Some(idx) = ins.get("program_id_index").and_then(|v| v.as_u64()) {
                    let i = idx as usize;
                    if i < account_keys_hex.len() {
                        ids.push(account_keys_hex[i].clone());
                    }
                }
            }
        }
    }
    if let Some(tags) = accounts_tags {
        for tag in tags {
            if let Some(pid) = tag.get("program_id").and_then(|v| v.as_str()) {
                ids.push(pid.to_string());
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

async fn process_block_via_rpc(pool: &PgPool, rpc: &Arc<ArchRpcClient>, height: i64) -> Result<()> {
    let hash = rpc.get_block_hash(height).await?;
    let block = rpc.get_block(&hash, height).await?;

    // Convert Arch timestamp to DateTime using centralized utility
    let timestamp = convert_arch_timestamp(block.timestamp);

    // Insert block
    sqlx::query(
        r#"
        INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (height) DO UPDATE 
        SET hash = EXCLUDED.hash, timestamp = EXCLUDED.timestamp, bitcoin_block_height = EXCLUDED.bitcoin_block_height
        "#,
    )
    .bind(height)
    .bind(&hash)
    .bind(timestamp)
    .bind(block.bitcoin_block_height.unwrap_or(0))
    .execute(pool)
    .await?;

    // Fetch transactions and insert
    let txids = block.transactions.clone();
    if !txids.is_empty() {
        let mut tx = pool.begin().await?;
        for txid in txids {
            let processed = rpc.get_processed_transaction(&txid).await?;
            let data = serde_json::to_value(&processed.runtime_transaction)?;
            let status = serde_json::to_value(&processed.status)?;
            let bitcoin_txids = processed.bitcoin_txids.as_ref().map(|v| v.as_slice());
            sqlx::query(
                r#"
                INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids, created_at)
                VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP)
                ON CONFLICT (txid) DO UPDATE 
                SET block_height = $2, data = $3, status = $4, bitcoin_txids = $5
                "#,
            )
            .bind(&txid)
            .bind(height)
            .bind(&data)
            .bind(&status)
            .bind(bitcoin_txids)
            .execute(&mut *tx)
            .await?;

            // Extract and upsert program IDs
            let pids = extract_program_ids(&data, Some(&processed.accounts_tags));
            for pid in pids {
                sqlx::query(
                    r#"
                    INSERT INTO programs (program_id, first_seen_at, last_seen_at, transaction_count)
                    VALUES ($1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 1)
                    ON CONFLICT (program_id) DO UPDATE
                    SET last_seen_at = CURRENT_TIMESTAMP,
                        transaction_count = programs.transaction_count + 1
                    "#
                )
                .bind(&pid)
                .execute(&mut *tx)
                .await?;

                sqlx::query(
                    r#"
                    INSERT INTO transaction_programs (txid, program_id)
                    VALUES ($1, $2)
                    ON CONFLICT DO NOTHING
                    "#
                )
                .bind(&txid)
                .bind(&pid)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
    }

    info!("✅ Processed block {} ({} txs)", height, block.transaction_count);
    Ok(())
}

#[derive(Debug, Clone)]
pub enum RealtimeStatus {
    BlockReceived { height: i64, timestamp: i64 },
    TransactionReceived { hash: String, timestamp: i64 },
    ConnectionStatus { connected: bool },
}
