use anyhow::Result;
use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;
use dashmap::DashMap;
use futures::stream;
use futures::StreamExt;
use std::sync::Arc;
use tracing::error;
use crate::arch_rpc::ArchRpcClient;
use crate::arch_rpc::Block;
use crate::db::models::Transaction;
use sqlx::PgPool;
use std::sync::atomic::{AtomicU64, AtomicI64};
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::atomic::Ordering;
use std::time::Duration;
use hex;
use tracing::info;
use crate::arch_rpc::ProcessedTransaction;

pub struct BlockProcessor {
    pub pool: PgPool,
    block_cache: DashMap<i64, Block>,
    redis: redis::Client,
    pub arch_client: Arc<ArchRpcClient>,
    sync_start_time: AtomicU64,
    current_block_height: AtomicI64,
    average_block_time: AtomicU64,
}

impl BlockProcessor {
    pub fn new(pool: PgPool, redis: redis::Client, arch_client: Arc<ArchRpcClient>) -> Self {
        info!("Initializing BlockProcessor...");
        Self {
            pool,
            block_cache: DashMap::new(),
            redis,
            arch_client,
            sync_start_time: AtomicU64::new(SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64),
            current_block_height: AtomicI64::new(0),
            average_block_time: AtomicU64::new(0),
        }
    }

    fn normalize_program_id(program_id: &serde_json::Value) -> Option<String> {
        match program_id {
            serde_json::Value::String(s) => {
                // If already hex, return as-is
                if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(s.to_lowercase());
                }
                // Try base58 decode
                if let Ok(bytes) = bs58::decode(s).into_vec() {
                    return Some(hex::encode(bytes));
                }
                None
            },
            serde_json::Value::Array(arr) => {
                let bytes: Vec<u8> = arr.iter()
                    .filter_map(|v| v.as_i64().map(|n| {
                        if n < 0 { (n + 256) as u8 } else { n as u8 }
                    }))
                    .collect();
                Some(hex::encode(bytes))
            },
            _ => None
        }
    }

    pub async fn process_transaction(&self, transaction: &Transaction, tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), anyhow::Error> {
        // First check if transaction already exists
        let exists = sqlx::query!(
            "SELECT EXISTS(SELECT 1 FROM transactions WHERE txid = $1)",
            transaction.txid
        )
        .fetch_one(&mut **tx)
        .await?
        .exists
        .unwrap_or(false);

        if exists {
            tracing::debug!("Transaction {} already processed, skipping", transaction.txid);
            return Ok(());
        }

        tracing::debug!("Processing transaction: {}", transaction.txid);
        
        let data_json = serde_json::to_value(&transaction.data)
            .expect("Failed to serialize transaction data");
        
        let bitcoin_txids: Option<&[String]> = transaction.bitcoin_txids.as_deref();
        let created_at_utc = Utc.from_utc_datetime(&transaction.created_at);
        
        // Insert the transaction
        tracing::debug!("Inserting transaction into database: {}", transaction.txid);
        match sqlx::query!(
            r#"
            INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (txid) DO UPDATE 
            SET block_height = $2, data = $3, status = $4, bitcoin_txids = $5, created_at = $6
            "#,
            transaction.txid,
            transaction.block_height as i32,
            data_json,
            transaction.status,
            bitcoin_txids,
            created_at_utc
        )
        .execute(&mut **tx)
        .await {
            Ok(_) => tracing::debug!("Successfully inserted/updated transaction: {}", transaction.txid),
            Err(e) => {
                tracing::error!("Error inserting transaction {}: {}", transaction.txid, e);
                return Err(e.into());
            }
        }

        // Extract and process program IDs manually since we can't rely on trigger
        tracing::debug!("Extracting program IDs for transaction: {}", transaction.txid);
        if let Some(message) = transaction.data.get("message") {
            if let Some(instructions) = message.get("instructions") {
                if let Some(instructions_array) = instructions.as_array() {
                    for (i, instruction) in instructions_array.iter().enumerate() {
                        if let Some(program_id) = instruction.get("program_id") {
                            tracing::debug!("Processing instruction {} with program_id: {:?}", i, program_id);
                            if let Some(hex_program_id) = BlockProcessor::normalize_program_id(program_id) {
                                tracing::debug!("Normalized program_id to hex: {:?}", hex_program_id);
                                // Update programs table
                                match sqlx::query!(
                                    r#"
                                    INSERT INTO programs (program_id)
                                    VALUES ($1)
                                    ON CONFLICT (program_id) 
                                    DO UPDATE SET 
                                        last_seen_at = CURRENT_TIMESTAMP,
                                        transaction_count = programs.transaction_count + 1
                                    "#,
                                    hex_program_id
                                )
                                .execute(&mut **tx)
                                .await {
                                    Ok(_) => tracing::debug!("Successfully inserted/updated program: {:?}", hex_program_id),
                                    Err(e) => tracing::warn!("Failed to insert program {:?}: {}", hex_program_id, e)
                                };
                                
                                // Insert into transaction_programs
                                match sqlx::query!(
                                    r#"
                                    INSERT INTO transaction_programs (txid, program_id)
                                    VALUES ($1, $2)
                                    ON CONFLICT DO NOTHING
                                    "#,
                                    transaction.txid,
                                    hex_program_id
                                )
                                .execute(&mut **tx)
                                .await {
                                    Ok(_) => tracing::debug!("Successfully linked transaction {} to program {:?}", transaction.txid, hex_program_id),
                                    Err(e) => tracing::warn!("Failed to link transaction to program: {}", e)
                                };
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn process_transactions_batch(&self, transactions: Vec<Transaction>) -> Result<()> {
        if transactions.is_empty() {
            return Ok(());
        }
        
        tracing::info!("Processing {} transactions in batch", transactions.len());
        
        // Use a single transaction for the entire batch
        let mut tx = self.pool.begin().await?;
        
        // Check which transactions already exist
        let txids: Vec<String> = transactions.iter().map(|t| t.txid.clone()).collect();
        let existing_txids: Vec<String> = sqlx::query_scalar!(
            "SELECT txid FROM transactions WHERE txid = ANY($1)",
            &txids
        )
        .fetch_all(&mut *tx)
        .await?;
        
        let existing_set: std::collections::HashSet<&str> = existing_txids.iter().map(|s| s.as_str()).collect();
        
        // Filter out existing transactions
        let new_transactions: Vec<&Transaction> = transactions.iter()
            .filter(|t| !existing_set.contains(t.txid.as_str()))
            .collect();
        
        if new_transactions.is_empty() {
            tracing::debug!("All {} transactions already exist", transactions.len());
            tx.commit().await?;
            return Ok(());
        }
        
        tracing::info!("Inserting {} new transactions", new_transactions.len());
        
        // Build batch insert query
        let mut query = String::from(
            "INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids, created_at) VALUES "
        );
        
        let values: Vec<String> = new_transactions.iter()
            .map(|tx| {
                let data_json = serde_json::to_string(&tx.data).unwrap_or_else(|_| "{}".to_string());
                let bitcoin_txids_str = tx.bitcoin_txids.as_ref()
                    .map(|txids| format!("{{{}}}", txids.join(",")))
                    .unwrap_or_else(|| "{}".to_string());
                
                format!(
                    "('{}', {}, '{}', {}, '{}', '{}')",
                    tx.txid, tx.block_height, data_json, tx.status, bitcoin_txids_str, tx.created_at
                )
            })
            .collect();
        
        query.push_str(&values.join(","));
        query.push_str(" ON CONFLICT (txid) DO NOTHING");
        
        // Execute batch insert
        sqlx::query(&query).execute(&mut *tx).await?;
        
        tx.commit().await?;
        tracing::info!("Successfully processed {} transactions in batch", new_transactions.len());
        
        Ok(())
    }

    pub fn get_current_block_height(&self) -> i64 {
        self.current_block_height.load(Ordering::SeqCst)
    }

    pub fn get_average_block_time(&self) -> u64 {
        self.average_block_time.load(Ordering::Relaxed)
    }

    pub fn get_sync_start_time(&self) -> u64 {
        self.sync_start_time.load(Ordering::Relaxed)
    }

    async fn fetch_blocks_batch(&self, heights: Vec<i64>) -> Result<Vec<(i64, Block)>> {
        let retry_limit = 3;
        let retry_delay = Duration::from_secs(1);
        let backoff_multiplier = 2; // Each retry will wait longer

        let futures: Vec<_> = heights
            .into_iter()
            .map(|height| {
                let client = Arc::clone(&self.arch_client);
                async move {
                    let mut attempts = 0;
                    let mut current_delay = retry_delay;

                    loop {
                        match async {
                            let block_hash = client.get_block_hash(height).await?;
                            let block = client.get_block(&block_hash, height).await?;
                            Ok::<_, anyhow::Error>((height, block))
                        }.await {
                            Ok(result) => return Some(result),
                            Err(e) => {
                                attempts += 1;
                                if attempts >= retry_limit {
                                    error!(
                                        "Failed to fetch block {} after {} attempts: {}",
                                        height, retry_limit, e
                                    );
                                    return None;
                                } else {
                                    error!(
                                        "Error fetching block {}: {}. Retrying {}/{} after {} ms",
                                        height,
                                        e,
                                        attempts,
                                        retry_limit,
                                        current_delay.as_millis()
                                    );
                                    tokio::time::sleep(current_delay).await;
                                    // Increase delay for next retry
                                    current_delay *= backoff_multiplier;
                                }
                            }
                        }
                    }
                }
            })
            .collect();

        // Process up to 50 block requests concurrently
        let results: Vec<_> = stream::iter(futures)
            .buffer_unordered(50)
            .filter_map(|result| async move { result })
            .collect()
            .await;

        Ok(results)
    }

    pub async fn process_blocks_batch(&self, heights: Vec<i64>) -> Result<Vec<Block>> {
        let mut processed_heights = std::collections::HashSet::new();
        let mut results = Vec::new();

        for height in heights {
            if processed_heights.contains(&height) {
                continue;
            }

            match self.process_block(height).await {
                Ok(block) => {
                    processed_heights.insert(height);
                    results.push(block);
                }
                Err(e) => {
                    error!("Failed to process block {}: {}", height, e);
                    // Optionally break or continue based on error type
                }
            }
        }

        Ok(results)
    }

    pub fn update_sync_metrics(&self, height: i64, block_time: Duration) {
        self.current_block_height.store(height, Ordering::Relaxed);
        self.average_block_time.store(block_time.as_millis() as u64, Ordering::Relaxed);
    }

    async fn fetch_block_transactions(&self, height: i64) -> Result<Vec<Transaction>, anyhow::Error> {
        tracing::debug!("Fetching transactions for height: {}", height);
        
        let block_hash = match self.arch_client.get_block_hash(height).await {
            Ok(hash) => {
                tracing::debug!("Retrieved block hash: {} for height: {}", hash, height);
                hash
            },
            Err(e) => {
                tracing::error!("Failed to get block hash for height {}: {}", height, e);
                return Err(anyhow::anyhow!("Failed to get block hash: {}", e));
            }
        };
        
        let block = match self.arch_client.get_block(&block_hash, height).await {
            Ok(block) => {
                tracing::debug!("Retrieved block with {} transactions for height: {}", block.transactions.len(), height);
                block
            },
            Err(e) => {
                tracing::error!("Failed to get block for height {}: {}", height, e);
                return Err(anyhow::anyhow!("Failed to get block: {}", e));
            }
        };
        
        let transactions: Vec<Transaction> = stream::iter(block.transactions)
            .map(|txid| {
                let client = Arc::clone(&self.arch_client);
                let txid_clone = txid.clone();
                async move {
                    tracing::debug!("Fetching transaction: {}", txid_clone);
                    match client.get_processed_transaction(&txid_clone).await {
                        Ok(tx) => {
                            tracing::debug!("Processed transaction: {}", txid_clone);
                            Some(Transaction {
                                txid: txid_clone,
                                block_height: height,
                                data: tx.runtime_transaction,
                                status: tx.status,
                                bitcoin_txids: tx.bitcoin_txids,
                                created_at: chrono::Utc::now().naive_utc(),
                            })
                        },
                        Err(e) => {
                            tracing::warn!("Failed to fetch transaction {}: {}", txid_clone, e);
                            None
                        }
                    }
                }
            })
            .buffer_unordered(50) // Increased from 10 to 50
            .filter_map(|result| async move { result })
            .collect()
            .await;

        tracing::debug!("Completed fetching {} transactions for height: {}", transactions.len(), height);
        Ok(transactions)
    }

    pub async fn get_last_processed_height(&self) -> Result<Option<i64>> {
        let height = sqlx::query!(
            "SELECT MAX(height) as last_height FROM blocks"
        )
        .fetch_one(&self.pool)
        .await?
        .last_height;

        Ok(height)
    }

    pub async fn process_block(&self, height: i64) -> Result<Block, anyhow::Error> {
        let start_time = std::time::Instant::now();

        // Get block hash with detailed error handling (check cache first)
        let block_hash = if let Some(cached_block) = self.block_cache.get(&height) {
            tracing::debug!("Cache hit for block height {}", height);
            cached_block.hash.clone()
        } else {
            tracing::debug!("Fetching block hash for height: {}", height);
            match self.arch_client.get_block_hash(height).await {
                Ok(hash) => {
                    tracing::debug!("Retrieved block hash: {} for height {}", hash, height);
                    hash
                },
                Err(e) => {
                    tracing::error!("Failed to get block hash for height {}: {}", height, e);
                    return Err(anyhow::anyhow!("Failed to get block hash: {}", e));
                }
            }
        };

        // Get block with detailed error handling (check cache first)
        let block = if let Some(cached_block) = self.block_cache.get(&height) {
            if cached_block.hash == block_hash {
                tracing::debug!("Cache hit for block data at height {}", height);
                cached_block.clone()
            } else {
                tracing::debug!("Cache miss - hash mismatch for height {}", height);
                match self.arch_client.get_block(&block_hash, height).await {
                    Ok(block) => {
                        tracing::debug!("Retrieved block: height={}, tx_count={}", block.height, block.transactions.len());
                        // Cache the block
                        self.block_cache.insert(height, block.clone());
                        block
                    },
                    Err(e) => {
                        tracing::error!("Failed to get block for hash {}: {}", block_hash, e);
                        return Err(anyhow::anyhow!("Failed to get block: {}", e));
                    }
                }
            }
        } else {
            tracing::debug!("Fetching block data for hash: {}", block_hash);
            match self.arch_client.get_block(&block_hash, height).await {
                Ok(block) => {
                    tracing::debug!("Retrieved block: height={}, tx_count={}", block.height, block.transactions.len());
                    // Cache the block
                    self.block_cache.insert(height, block.clone());
                    block
                },
                Err(e) => {
                    tracing::error!("Failed to get block for hash {}: {}", block_hash, e);
                    return Err(anyhow::anyhow!("Failed to get block: {}", e));
                }
            }
        };

        // Fetch transactions with detailed error handling
        tracing::debug!("Fetching transactions for block height: {}", height);
        let transactions = match self.fetch_block_transactions(height).await {
            Ok(txs) => {
                tracing::debug!("Retrieved {} transactions for height {}", txs.len(), height);
                txs
            },
            Err(e) => {
                tracing::error!("Failed to fetch transactions for height {}: {}", height, e);
                return Err(anyhow::anyhow!("Failed to fetch transactions: {}", e));
            }
        };
        
        // Start database transaction
        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::error!("Failed to begin database transaction: {}", e);
                return Err(anyhow::anyhow!("Database transaction error: {}", e));
            }
        };

        tracing::debug!("Processing block: height={}, hash={}, tx_count={}", height, block_hash, block.transactions.len());

        fn convert_timestamp(unix_timestamp: i64) -> DateTime<Utc> {
            // If timestamp is in milliseconds, convert to seconds
            let timestamp_secs = if unix_timestamp > 1_000_000_000_000 {
                unix_timestamp / 1000
            } else {
                unix_timestamp
            };

            // Convert the Unix timestamp to a DateTime<Utc>
            chrono::DateTime::<Utc>::from_timestamp(timestamp_secs, 0)
                .unwrap_or(chrono::DateTime::<Utc>::from_timestamp(0, 0).unwrap())
        }

        // Insert block with detailed error handling
        match sqlx::query!(
            r#"
            INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (height) DO UPDATE 
            SET hash = $2, timestamp = $3, bitcoin_block_height = $4
            "#,
            height,
            block_hash,
            convert_timestamp(block.timestamp),
            block.bitcoin_block_height.unwrap_or(0)
        )
        .execute(&mut *tx)
        .await {
            Ok(_) => tracing::debug!("Inserted/updated block {}", height),
            Err(e) => {
                tracing::error!("Failed to insert block {}: {}", height, e);
                return Err(anyhow::anyhow!("Block insertion error: {}", e));
            }
        }

        // Process transactions
        if !transactions.is_empty() {
            tracing::debug!("Processing {} transactions for block {}", transactions.len(), height);
            for transaction in transactions.iter() {
                if let Err(e) = self.process_transaction(transaction, &mut tx).await {
                    tracing::error!("Failed to process transaction {}: {}", transaction.txid, e);
                    return Err(anyhow::anyhow!("Transaction processing error: {}", e));
                }
            }
        }

        // Commit transaction
        if let Err(e) = tx.commit().await {
            tracing::error!("Failed to commit database transaction: {}", e);
            return Err(anyhow::anyhow!("Failed to commit transaction: {}", e));
        }

        tracing::info!("Processed block {} with {} transactions", height, transactions.len());
        
        self.update_current_height(height);
        self.update_sync_metrics(height, start_time.elapsed());
        
        Ok(block)
    }

    /// Process a Block object directly (for optimized sync)
    pub async fn process_block_direct(&self, block: Block) -> Result<(), anyhow::Error> {
        let start_time = std::time::Instant::now();
        let height = block.height;
        let block_hash = &block.hash;

        tracing::debug!("Processing block directly: height={}, hash={}, tx_count={}", height, block_hash, block.transactions.len());

        // Start database transaction
        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::error!("Failed to begin database transaction: {}", e);
                return Err(anyhow::anyhow!("Database transaction error: {}", e));
            }
        };

        fn convert_timestamp(unix_timestamp: i64) -> DateTime<Utc> {
            // If timestamp is in milliseconds, convert to seconds
            let timestamp_secs = if unix_timestamp > 1_000_000_000_000 {
                unix_timestamp / 1000
            } else {
                unix_timestamp
            };

            // Convert the Unix timestamp to a DateTime<Utc>
            chrono::DateTime::<Utc>::from_timestamp(timestamp_secs, 0)
                .unwrap_or(chrono::DateTime::<Utc>::from_timestamp(0, 0).unwrap())
        }

        // Insert block with enhanced data
        match sqlx::query!(
            r#"
            INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height, merkle_root, previous_block_hash)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (height) DO UPDATE 
            SET hash = $2, timestamp = $3, bitcoin_block_height = $4, merkle_root = $5, previous_block_hash = $6
            "#,
            height,
            block_hash,
            convert_timestamp(block.timestamp),
            block.bitcoin_block_height.unwrap_or(0),
            "", // TODO: Get merkle_root from RPC response
            ""  // TODO: Get previous_block_hash from RPC response
        )
        .execute(&mut *tx)
        .await {
            Ok(_) => tracing::debug!("Inserted/updated block {}", height),
            Err(e) => {
                tracing::error!("Failed to insert block {}: {}", height, e);
                return Err(anyhow::anyhow!("Block insertion error: {}", e));
            }
        }

        // Process transactions if any
        if !block.transactions.is_empty() {
            tracing::debug!("Processing {} transactions for block {}", block.transactions.len(), height);
            
            // Fetch transaction details for each txid
            for txid in &block.transactions {
                if let Ok(transaction) = self.arch_client.get_processed_transaction(txid).await {
                    if let Err(e) = self.process_transaction_direct(&transaction, &mut tx, txid, height).await {
                        tracing::error!("Failed to process transaction {}: {}", txid, e);
                        // Continue processing other transactions
                    }
                } else {
                    tracing::warn!("Failed to fetch transaction details for {}", txid);
                }
            }
        }

        // Commit transaction
        if let Err(e) = tx.commit().await {
            tracing::error!("Failed to commit database transaction: {}", e);
            return Err(anyhow::anyhow!("Failed to commit transaction: {}", e));
        }

        tracing::info!("Processed block {} with {} transactions", height, block.transactions.len());
        
        self.update_current_height(height);
        self.update_sync_metrics(height, start_time.elapsed());
        
        Ok(())
    }

    /// Process a ProcessedTransaction directly (for optimized sync)
    async fn process_transaction_direct(&self, transaction: &ProcessedTransaction, tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, actual_txid: &str, block_height: i64) -> Result<(), anyhow::Error> {
        // Use the actual transaction ID from the block data
        let txid = actual_txid.to_string();

        // Extract compute units from logs if available
        let compute_units = if let Some(logs) = transaction.runtime_transaction.get("logs") {
            if let Some(logs_array) = logs.as_array() {
                logs_array.iter()
                    .filter_map(|log| log.as_str())
                    .find_map(|log| {
                        if log.contains("Consumed") {
                            log.split_whitespace()
                                .filter_map(|word| word.parse::<i32>().ok())
                                .next()
                        } else {
                            None
                        }
                    })
            } else {
                None
            }
        } else {
            None
        };

        // Insert transaction with enhanced data
        match sqlx::query!(
            r#"
            INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids, created_at, logs, rollback_status, accounts_tags, compute_units_consumed)
            VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP, $6, $7, $8, $9)
            ON CONFLICT (txid) DO UPDATE 
            SET data = $3, status = $4, bitcoin_txids = $5, logs = $6, rollback_status = $7, accounts_tags = $8, compute_units_consumed = $9
            "#,
            txid,
            block_height, // Use the actual block height from context
            serde_json::to_value(&transaction.runtime_transaction)?,
            serde_json::to_value(&transaction.status)?,
            transaction.bitcoin_txids.as_ref().map(|txids| txids.as_slice()),
            serde_json::to_value(&transaction.runtime_transaction.get("logs").unwrap_or(&serde_json::Value::Array(vec![])))?,
            serde_json::to_value("NotRolledback")?, // Default rollback status
            serde_json::to_value(&transaction.accounts_tags)?,
            compute_units
        )
        .execute(&mut **tx)
        .await {
            Ok(_) => tracing::debug!("Inserted/updated transaction {}", txid),
            Err(e) => {
                tracing::error!("Failed to insert transaction {}: {}", txid, e);
                return Err(anyhow::anyhow!("Transaction insertion error: {}", e));
            }
        }

        // Process program IDs from accounts_tags
        for account_tag in &transaction.accounts_tags {
            if let Some(program_id) = account_tag.get("program_id").and_then(|id| id.as_str()) {
                // Insert program
                if let Err(e) = sqlx::query!(
                    r#"
                    INSERT INTO programs (program_id)
                    VALUES ($1)
                    ON CONFLICT (program_id) DO UPDATE SET 
                        last_seen_at = CURRENT_TIMESTAMP,
                        transaction_count = programs.transaction_count + 1
                    "#,
                    program_id
                )
                .execute(&mut **tx)
                .await {
                    tracing::error!("Failed to insert program {}: {}", program_id, e);
                }

                // Insert transaction-program relationship
                if let Err(e) = sqlx::query!(
                    r#"
                    INSERT INTO transaction_programs (txid, program_id)
                    VALUES ($1, $2)
                    ON CONFLICT (txid, program_id) DO NOTHING
                    "#,
                    txid,
                    program_id
                )
                .execute(&mut **tx)
                .await {
                    tracing::error!("Failed to insert transaction-program relationship: {}", e);
                }
            }
        }

        Ok(())
    }

    pub fn update_current_height(&self, height: i64) {
        self.current_block_height.store(height, Ordering::SeqCst);
    }

    pub async fn sync_mempool(&self) -> Result<(), anyhow::Error> {
        tracing::info!("Starting mempool sync...");
        
        let mut tx = self.pool.begin().await?;
        
        // Get current mempool transaction IDs
        let mempool_txids = self.arch_client.get_mempool_txids().await?;
        tracing::info!("Found {} transactions in mempool", mempool_txids.len());
        
        // Get existing mempool transactions from database
        let existing_txids: Vec<String> = sqlx::query_scalar!(
            "SELECT txid FROM mempool_transactions"
        )
        .fetch_all(&mut *tx)
        .await?;
        
        let existing_set: std::collections::HashSet<&str> = existing_txids.iter().map(|s| s.as_str()).collect();
        
        // Process new mempool transactions
        let mut new_count = 0;
        for txid in &mempool_txids {
            if !existing_set.contains(txid.as_str()) {
                if let Ok(Some(mempool_entry)) = self.arch_client.get_mempool_entry(txid).await {
                    // Extract transaction data and metadata
                    let data = serde_json::to_value(&mempool_entry)?;
                    let fee_priority = mempool_entry.get("fee").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let size_bytes = mempool_entry.get("size").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    
                    // Insert into mempool table
                    if let Err(e) = sqlx::query!(
                        r#"
                        INSERT INTO mempool_transactions (txid, data, fee_priority, size_bytes)
                        VALUES ($1, $2, $3, $4)
                        ON CONFLICT (txid) DO UPDATE 
                        SET data = $2, fee_priority = $3, size_bytes = $4, added_at = CURRENT_TIMESTAMP
                        "#,
                        txid,
                        data,
                        fee_priority,
                        size_bytes
                    )
                    .execute(&mut *tx)
                    .await {
                        tracing::warn!("Failed to insert mempool transaction {}: {}", txid, e);
                    } else {
                        new_count += 1;
                    }
                }
            }
        }
        
        // Remove transactions that are no longer in mempool
        let mempool_set: std::collections::HashSet<&str> = mempool_txids.iter().map(|s| s.as_str()).collect();
        let to_remove: Vec<String> = existing_txids.iter()
            .filter(|txid| !mempool_set.contains(txid.as_str()))
            .cloned()
            .collect();
        
        if !to_remove.is_empty() {
            sqlx::query!(
                "DELETE FROM mempool_transactions WHERE txid = ANY($1)",
                &to_remove
            )
            .execute(&mut *tx)
            .await?;
            
            tracing::info!("Removed {} transactions from mempool tracking", to_remove.len());
        }
        
        tx.commit().await?;
        tracing::info!("Mempool sync completed. Added {} new transactions, removed {} old ones", new_count, to_remove.len());
        
        Ok(())
    }

    pub async fn sync_missing_program_data(&self) -> Result<(), anyhow::Error> {
        tracing::info!("Starting to sync missing program data...");
        
        let mut tx = self.pool.begin().await?;
        
        // Simplified query that doesn't rely on CTEs
        let rows = sqlx::query!(
            r#"
            SELECT 
                t.txid,
                jsonb_array_elements(
                    CASE 
                        WHEN jsonb_typeof(t.data#>'{message,instructions}') = 'array' 
                        THEN t.data#>'{message,instructions}' 
                        ELSE '[]'::jsonb 
                    END
                )->>'program_id' as program_id
            FROM transactions t
            WHERE NOT EXISTS (
                SELECT 1 FROM transaction_programs tp 
                WHERE tp.txid = t.txid
            )
            "#
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut count = 0;
        for row in rows {
            if let Some(program_id) = row.program_id {
                // Insert into programs and transaction_programs tables
                sqlx::query!(
                    r#"
                    INSERT INTO programs (program_id)
                    VALUES ($1)
                    ON CONFLICT (program_id) DO UPDATE SET 
                        last_seen_at = CURRENT_TIMESTAMP,
                        transaction_count = programs.transaction_count + 1
                    "#,
                    program_id
                )
                .execute(&mut *tx)
                .await?;
                
                sqlx::query!(
                    r#"
                    INSERT INTO transaction_programs (txid, program_id)
                    VALUES ($1, $2)
                    ON CONFLICT DO NOTHING
                    "#,
                    row.txid,
                    program_id
                )
                .execute(&mut *tx)
                .await?;

                count += 1;
                if count % 1000 == 0 {
                    tracing::info!("Processed {} transactions", count);
                    tx.commit().await?;
                    tx = self.pool.begin().await?;
                }
            }
        }

        tx.commit().await?;
        tracing::info!("Finished syncing program data. Processed {} total transactions", count);
        Ok(())
    }
}