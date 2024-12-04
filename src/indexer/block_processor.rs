use anyhow::Result;
use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;
use dashmap::DashMap;
use futures::stream;
use futures::StreamExt;
use std::fmt::Write;
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
    pub fn new(pool: PgPool, redis: redis::Client, arch_client: ArchRpcClient) -> Self {
        Self {
            pool,
            block_cache: DashMap::new(),
            redis,
            arch_client: Arc::new(arch_client),
            sync_start_time: AtomicU64::new(SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64),
            current_block_height: AtomicI64::new(0),
            average_block_time: AtomicU64::new(0),
        }
    }

    pub async fn process_transaction(&self, transaction: &Transaction, tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), anyhow::Error> {
        // First insert the transaction
        let data_json = serde_json::to_value(&transaction.data)
            .expect("Failed to serialize transaction data");
        
        let bitcoin_txids: Option<&[String]> = transaction.bitcoin_txids.as_deref();
        let created_at_utc = Utc.from_utc_datetime(&transaction.created_at);
        
        // Insert the transaction
        sqlx::query!(
            r#"
            INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (txid) DO UPDATE 
            SET block_height = $2, data = $3, status = $4, bitcoin_txids = $5, created_at = $6
            "#,
            transaction.txid,
            transaction.block_height,
            data_json,
            serde_json::Value::String(transaction.status.to_string()),
            bitcoin_txids,
            created_at_utc
        )
        .execute(&mut **tx)
        .await?;

        // Extract program IDs directly from the transaction data
        if let Some(message) = transaction.data.get("message") {
            if let Some(instructions) = message.get("instructions") {
                if let Some(instructions_array) = instructions.as_array() {
                    for instruction in instructions_array {
                        if let Some(program_id) = instruction.get("program_id") {
                            let program_id_str = match program_id {
                                serde_json::Value::String(s) => Some(s.clone()),
                                serde_json::Value::Array(arr) => {
                                    let bytes: Vec<u8> = arr.iter()
                                        .filter_map(|v| v.as_u64().map(|n| n as u8))
                                        .collect();
                                    Some(bs58::encode(bytes).into_string())
                                },
                                _ => None
                            };

                            if let Some(program_id) = program_id_str {
                                // Update programs table
                                sqlx::query!(
                                    r#"
                                    INSERT INTO programs (program_id)
                                    VALUES ($1)
                                    ON CONFLICT (program_id) 
                                    DO UPDATE SET 
                                        last_seen_at = CURRENT_TIMESTAMP,
                                        transaction_count = programs.transaction_count + 1
                                    "#,
                                    program_id
                                )
                                .execute(&mut **tx)
                                .await?;
                                
                                // Insert into transaction_programs
                                sqlx::query!(
                                    r#"
                                    INSERT INTO transaction_programs (txid, program_id)
                                    VALUES ($1, $2)
                                    ON CONFLICT DO NOTHING
                                    "#,
                                    transaction.txid,
                                    program_id
                                )
                                .execute(&mut **tx)
                                .await?;
                            }
                        }
                    }
                }
            }
        }

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

        println!("Processing blocks batch {:?}", heights);
        
        let start_time = std::time::Instant::now();
        let heights_clone = heights.clone();
        let blocks = self.fetch_blocks_batch(heights_clone).await?;

        println!("Fetched {} blocks", blocks.len());
    
        // Start database transaction
        let mut tx = self.pool.begin().await?;

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
    
        // Process blocks and their transactions
        for (height, block) in &blocks {
            // Insert block (existing code)
            sqlx::query!(
                r#"
                INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (height) DO UPDATE 
                SET hash = $2, timestamp = $3, bitcoin_block_height = $4
                "#,
                height,
                block.hash,
                convert_timestamp(block.timestamp),
                block.bitcoin_block_height
            )
            .execute(&mut *tx)
            .await?;

            // println!("Processed block {}", height);
    
            // Process transactions if present
            if !block.transactions.is_empty() {
                println!("Fetching transactions for block {}", height);
                println!("Block transactions: {:?}", block.transactions);

                let transactions = self.fetch_block_transactions(*height).await?;

                println!("Found {} transactions in block {}", transactions.len(), height);
                
                // Use batch insert for efficient insertion
                if !transactions.is_empty() {
                    for transaction in &transactions {
                        self.process_transaction(transaction, &mut tx).await?;
                    } 
                }
            }

            // Add this line to update the height
            self.update_current_height(*height);
        }
    
        tx.commit().await?;
    
        // Update metrics
        if let Some(&max_height) = heights.iter().max() {
            let avg_time = start_time.elapsed().div_f64(heights.len() as f64);
            self.update_sync_metrics(max_height, avg_time);
        }
    
        Ok(blocks.into_iter().map(|(_, block)| block).collect())
    }
    pub fn update_sync_metrics(&self, height: i64, block_time: Duration) {
        self.current_block_height.store(height, Ordering::Relaxed);
        self.average_block_time.store(block_time.as_millis() as u64, Ordering::Relaxed);
    }

    async fn fetch_block_transactions(&self, height: i64) -> Result<Vec<Transaction>, anyhow::Error> {
        let block_hash = self.arch_client.get_block_hash(height).await?;
        let block = self.arch_client.get_block(&block_hash, height).await?;
        
        let transactions = stream::iter(block.transactions)
        .map(|txid| {
            let client: Arc<ArchRpcClient> = Arc::clone(&self.arch_client);
            let txid_clone = txid.clone();
            async move {
                match client.get_processed_transaction(&txid_clone).await {
                    Ok(tx) => {
                        // Handle bitcoin_txids similar to Node.js code
                        //let bitcoin_txids = tx.bitcoin_txids.as_ref().map(|txids| txids.join(",")).unwrap_or_else(|| "{}".to_string());

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
                        error!("Failed to fetch transaction {}: {:?}", txid, e);
                        None
                    }
                }
            }
        })
        .buffer_unordered(10)
        .filter_map(|result| async move { result })
        .collect()
        .await;
    
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

        let block_hash = self.arch_client.get_block_hash(height).await?;
        let block = self.arch_client.get_block(&block_hash, height).await?;
        let transactions = self.fetch_block_transactions(height).await?;
        
        // Start a database transaction
        let mut tx = self.pool.begin().await?;

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

        // Prepare block insert
        sqlx::query!(
            r#"
            INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (height) DO UPDATE 
            SET hash = $2, timestamp = $3, bitcoin_block_height = $4
            "#,
            height,
            block_hash,
            convert_timestamp(block.timestamp),
            block.bitcoin_block_height
        )
        .execute(&mut *tx)
        .await?;

        // Batch insert transactions using COPY
        if !transactions.is_empty() {
            for transaction in &transactions {
                self.process_transaction(transaction, &mut tx).await?;
            }
        }

        tx.commit().await?;

        self.update_current_height(height);
        
        self.update_sync_metrics(height, start_time.elapsed());
        
        Ok(block)
    }

    pub fn update_current_height(&self, height: i64) {
        self.current_block_height.store(height, Ordering::SeqCst);
    }

    pub async fn sync_missing_program_data(&self) -> Result<(), anyhow::Error> {
        println!("Starting to sync missing program data...");
        
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
                    println!("Processed {} transactions", count);
                    tx.commit().await?;
                    tx = self.pool.begin().await?;
                }
            }
        }

        tx.commit().await?;
        println!("Finished syncing program data. Processed {} total transactions", count);
        Ok(())
    }
}