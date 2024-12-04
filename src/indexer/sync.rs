use anyhow::Result;
use futures::stream::{self, StreamExt};
use sqlx::query_scalar;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::time::{Duration, sleep};
use tracing::{info, warn, error};

use tokio_retry::strategy::{ExponentialBackoff, jitter};
use tokio_retry::Retry;

use super::block_processor::BlockProcessor;

pub struct ChainSync {
    pub processor: Arc<BlockProcessor>,
    current_height: AtomicI64,
    batch_size: usize,
    concurrent_batches: usize,
}

impl ChainSync {
    pub fn new(
        processor: Arc<BlockProcessor>,
        starting_height: i64,
        batch_size: usize,
        concurrent_batches: usize,
    ) -> Self {
        Self {
            processor,
            current_height: AtomicI64::new(starting_height),
            batch_size,
            concurrent_batches,
        }
    }

    pub async fn start(&self) -> Result<()> {
        let mut current = self.current_height.load(Ordering::Relaxed);
        let mut target_height = self.processor.arch_client.get_block_count().await?;
        let missing_blocks_check_interval = Duration::from_secs(300); // Check every 5 minutes

        // Run initial missing blocks check on startup
        info!("Running initial missing blocks check on startup...");
        let missing_blocks = self.check_for_missing_blocks().await?;
        
        if !missing_blocks.is_empty() {
            info!("Found {} missing blocks, resyncing...", missing_blocks.len());
            for chunk in missing_blocks.chunks(self.batch_size) {
                let heights: Vec<i64> = chunk.to_vec();
                match self.processor.process_blocks_batch(heights).await {
                    Ok(_) => info!("Resynced blocks {}-{}", chunk[0], chunk[chunk.len()-1]),
                    Err(e) => error!("Error resyncing blocks: {}", e),
                }
            }
        } else {
            info!("No missing blocks found on startup");
        }

        let mut last_missing_check = std::time::Instant::now();

        loop {
            // Check for missing blocks periodically
            if last_missing_check.elapsed() >= missing_blocks_check_interval {
                info!("Checking for missing blocks...");
                let missing_blocks = self.check_for_missing_blocks().await?;
                
                if !missing_blocks.is_empty() {
                    info!("Found {} missing blocks, resyncing...", missing_blocks.len());
                    // Process missing blocks in batches
                    for chunk in missing_blocks.chunks(self.batch_size) {
                        let heights: Vec<i64> = chunk.to_vec();
                        match self.processor.process_blocks_batch(heights).await {
                            Ok(_) => info!("Resynced blocks {}-{}", chunk[0], chunk[chunk.len()-1]),
                            Err(e) => error!("Error resyncing blocks: {}", e),
                        }
                    }
                }
                last_missing_check = std::time::Instant::now();
            }

            let new_target_height = self.processor.arch_client.get_block_count().await?;
            
            // Update target height if new blocks exist
            if new_target_height > target_height {
                target_height = new_target_height;
            }
    
            // Skip processing if we're caught up
            if current >= target_height {
                sleep(Duration::from_secs(1)).await;
                continue;
            }
    
            // Use smaller batches when near the tip
            let remaining_blocks = target_height - current;
            let effective_batch_size = if remaining_blocks < (self.batch_size as i64) {
                1 // Process one by one when near the tip
            } else {
                self.batch_size
            };
    
            let effective_concurrent_batches = if remaining_blocks < (self.batch_size as i64) {
                1 // Use single batch when near the tip
            } else {
                self.concurrent_batches
            };
    
            // Rest of the batch processing logic...
            let batch_starts: Vec<_> = (0..effective_concurrent_batches)
                .map(|i| current + (i as i64 * effective_batch_size as i64))
                .filter(|&start| start <= target_height)
                .collect();
    
            let batch_futures: Vec<_> = batch_starts
                .into_iter()
                .map(|start| {
                    let end = (start + self.batch_size as i64 - 1).min(target_height);
                    let heights: Vec<_> = (start..=end).collect();
                    let processor = Arc::clone(&self.processor);
    
                    async move {
                        let retry_strategy = ExponentialBackoff::from_millis(10)
                            .map(jitter) // add jitter to delays
                            .take(5); // retry up to 5 times
    
                        Retry::spawn(retry_strategy, || async {
                            processor.process_blocks_batch(heights.clone()).await
                        })
                        .await
                    }
                })
                .collect();
    
            // Process batches concurrently
            let results = futures::future::join_all(batch_futures).await;
    
            // Update progress
            for result in results {
                if let Ok(blocks) = result {
                    if let Some(last_block) = blocks.last() {
                        self.current_height.store(last_block.height, Ordering::Relaxed);
                    }
                }
            }
    
            current = self.current_height.load(Ordering::Relaxed);
        }
    }


    async fn check_for_missing_blocks(&self) -> Result<Vec<i64>> {
        // Get the overall bounds
        let bounds = sqlx::query!(
            r#"
            SELECT MIN(height) AS min_height, MAX(height) AS max_height
            FROM blocks
            "#
        )
        .fetch_one(&self.processor.pool)
        .await?;

        let min_height = bounds.min_height.unwrap_or(0);
        let max_height = bounds.max_height.unwrap_or(0);
        let chunk_size = 100_000; // Check 100k blocks at a time
        let mut missing_blocks = Vec::new();

        // Process in chunks
        for chunk_start in (min_height..=max_height).step_by(chunk_size) {
            let chunk_end = (chunk_start + chunk_size as i64 - 1).min(max_height);
            
            let chunk_missing = sqlx::query_scalar!(
                r#"
                WITH chunk_bounds AS (
                    SELECT 
                        $1::bigint as chunk_start,
                        $2::bigint as chunk_end
                ),
                expected AS (
                    SELECT generate_series(chunk_start, chunk_end) as height
                    FROM chunk_bounds
                )
                SELECT e.height
                FROM expected e
                LEFT JOIN blocks b ON b.height = e.height
                WHERE b.height IS NULL
                ORDER BY e.height
                "#,
                chunk_start,
                chunk_end
            )
            .fetch_all(&self.processor.pool)
            .await?;

            missing_blocks.extend(chunk_missing.into_iter().flatten());

            // Log progress periodically
            if missing_blocks.len() % 1000 == 0 {
                info!(
                    "Checking for gaps: {:.1}% ({}/{})", 
                    (chunk_end - min_height) as f64 / (max_height - min_height) as f64 * 100.0,
                    chunk_end,
                    max_height
                );
            }
        }

        Ok(missing_blocks)
    }

    async fn sync_blocks(&self) -> Result<()> {
        let target_height = self.get_target_height().await?;
        tracing::info!("Target height: {}", target_height);
        let current = self.current_height.load(Ordering::Relaxed);

        if current >= target_height {
            return Ok(());
        }

        let processor = Arc::clone(&self.processor);

        stream::iter((current..=target_height).step_by(self.batch_size))
            .map(|batch_start| {
                let batch_end = (batch_start + self.batch_size as i64 - 1).min(target_height);
                let processor = Arc::clone(&processor);
                
                async move {
                    let mut results = Vec::new();
                    for height in batch_start..=batch_end {
                        match processor.process_block(height).await {
                            Ok(block) => results.push(Ok(block)),
                            Err(e) => {
                                error!("Error processing block {}: {:?}", height, e);
                                results.push(Err(e));
                            }
                        }
                    }
                    results
                }
            })
            .buffer_unordered(self.concurrent_batches)
            .for_each(|batch_results| async {
                for result in batch_results {
                    match result {
                        Ok(block) => {
                            self.current_height.store(block.height, Ordering::Relaxed);
                            // info!("Processed block {}", block.height);
                        }
                        Err(e) => warn!("Failed to process block: {:?}", e),
                    }
                }
            })
            .await;

        Ok(())
    }

    async fn get_target_height(&self) -> Result<i64> {
        let latest_height = self.processor.arch_client.get_block_count().await?;
        
        // Log sync progress
        let current = self.current_height.load(Ordering::Relaxed);
        let progress = if latest_height > 0 {
            (current as f64 / latest_height as f64 * 100.0).round()
        } else {
            0.0
        };
        
        info!(
            "Sync progress: {:.2}% ({}/{})", 
            progress, 
            current, 
            latest_height
        );
    
        Ok(latest_height)
    }
}