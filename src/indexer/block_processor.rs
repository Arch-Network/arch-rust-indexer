use anyhow::Result;
use dashmap::DashMap;
use futures::stream;
use futures::StreamExt;
use std::fmt::Write;
use std::sync::Arc;
use tracing::error;
use crate::arch_rpc::Block as ArchBlock;
use crate::arch_rpc::ArchRpcClient;
use crate::arch_rpc::Block;
use crate::db::models::Transaction;
use chrono::NaiveDateTime;
use sqlx::PgPool;
use std::sync::atomic::{AtomicU64, AtomicI64};
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::atomic::Ordering;
use std::time::Duration;

pub struct BlockProcessor {
    pool: PgPool,
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

    pub fn get_current_block_height(&self) -> i64 {
        self.current_block_height.load(Ordering::Relaxed)
    }

    pub fn get_average_block_time(&self) -> u64 {
        self.average_block_time.load(Ordering::Relaxed)
    }

    pub fn get_sync_start_time(&self) -> u64 {
        self.sync_start_time.load(Ordering::Relaxed)
    }

    async fn fetch_blocks_batch(&self, heights: Vec<i64>) -> Result<Vec<(i64, Block)>> {
        let futures: Vec<_> = heights
            .into_iter()
            .map(|height| {
                let client = Arc::clone(&self.arch_client);
                async move {
                    match async {
                        let block_hash = client.get_block_hash(height).await?;
                        let block = client.get_block(&block_hash, height).await?;
                        Ok::<_, anyhow::Error>((height, block))
                    }.await {
                        Ok(result) => Some(result),
                        Err(e) => {
                            error!("Failed to fetch block {}: {:?}", height, e);
                            None
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
        let start_time = std::time::Instant::now();

        let start = std::time::Instant::now();
        let heights_clone = heights.clone();
        let blocks = self.fetch_blocks_batch(heights_clone).await?;
    
        // Batch insert blocks
        let mut tx = self.pool.begin().await?;

        fn convert_timestamp(unix_timestamp: i64) -> NaiveDateTime {
            // If timestamp is in milliseconds, convert to seconds
            let timestamp_secs = if unix_timestamp > 1_000_000_000_000 {
                unix_timestamp / 1000
            } else {
                unix_timestamp
            };
        
            NaiveDateTime::from_timestamp_opt(timestamp_secs, 0)
                .unwrap_or_else(|| NaiveDateTime::from_timestamp_opt(0, 0).unwrap())
        }
    
        for (height, block) in &blocks {
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
        }

        tx.commit().await?;
    
        metrics::histogram!("batch_processing_time", start.elapsed().as_secs_f64(), 
            "batch_size" => heights.len().to_string()
        );

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
                // Clone the Arc reference to the ArchRpcClient to ensure it's not dropped before the async task completes
                let client: Arc<ArchRpcClient> = Arc::clone(&self.arch_client);
                // Clone the txid to use within the async closure
                let txid_clone = txid.clone();
                async move {
                    match client.get_processed_transaction(&txid_clone).await {
                        Ok(tx) => Some(Transaction {
                            txid: txid_clone,
                            block_height: height,
                            data: tx.runtime_transaction,
                            status: if tx.status == "Processing" { 0 } else { 1 },
                            bitcoin_txids: tx.bitcoin_txids.unwrap_or_default(),
                            created_at: chrono::Utc::now().naive_utc(),
                        }),
                        Err(e) => {
                            error!("Failed to fetch transaction {}: {:?}", txid, e);
                            None
                        }
                    }
                }
            })
            .buffer_unordered(10) // Process 10 transactions concurrently
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


    pub async fn process_block(&self, height: i64) -> Result<Block> {
        let start_time = std::time::Instant::now();

        let block_hash = self.arch_client.get_block_hash(height).await?;
        let block = self.arch_client.get_block(&block_hash, height).await?;
        let transactions = self.fetch_block_transactions(height).await?;
        
        // Start a database transaction
        let mut tx = self.pool.begin().await?;

        pub fn convert_timestamp(unix_timestamp: i64) -> NaiveDateTime {
            // If timestamp is in milliseconds, convert to seconds
            let timestamp_secs = if unix_timestamp > 1_000_000_000_000 {
                unix_timestamp / 1000
            } else {
                unix_timestamp
            };
            
            NaiveDateTime::from_timestamp_opt(timestamp_secs, 0)
                .unwrap_or_else(|| NaiveDateTime::from_timestamp_opt(0, 0).unwrap())
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
            let mut copy = String::new();
            for tx in &transactions {
                let data_json = serde_json::to_string(&tx.data).expect("Failed to serialize transaction data");
                writeln!(
                    &mut copy,
                    "{}\t{}\t{}\t{}\t{}",
                    tx.txid,
                    tx.block_height,
                    data_json,
                    tx.status,
                    tx.bitcoin_txids.join(",")
                )?;
            }

            let copy_statement = format!(
                "COPY transactions (txid, block_height, data, status, bitcoin_txids) FROM STDIN"
            );
            
            sqlx::query(&copy_statement)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;

        self.update_sync_metrics(height, start_time.elapsed());
        
        Ok(block)
    }
}