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
        let missing_programs_check_interval = Duration::from_secs(600); // Check every 10 minutes

        let mut last_missing_check = std::time::Instant::now();
        let mut last_programs_check = std::time::Instant::now();
        let mut initial_sync_complete = false;

        loop {
            match self.sync_blocks().await {
                Ok(_) => {
                    // Check for missing blocks periodically
                    if last_missing_check.elapsed() >= missing_blocks_check_interval {
                        if let Ok(missing) = self.check_for_missing_blocks().await {
                            if !missing.is_empty() {
                                info!("Found {} missing blocks, processing...", missing.len());
                                // Process missing blocks...
                            }
                        }
                        last_missing_check = std::time::Instant::now();
                    }

                    // Check for missing program data periodically
                    if last_programs_check.elapsed() >= missing_programs_check_interval {
                        if let Err(e) = self.processor.sync_missing_program_data().await {
                            error!("Error syncing program data: {}", e);
                        }
                        last_programs_check = std::time::Instant::now();
                    }

                    continue;
                }
                Err(e) => {
                    error!("Sync error: {}. Attempting to reconnect...", e);
                    if let Err(e) = self.wait_for_connection().await {
                        error!("Failed to reconnect: {}", e);
                    }
                    continue;
                }
            }
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

    async fn wait_for_connection(&self) -> Result<()> {
        let mut delay = Duration::from_secs(1);
        let max_delay = Duration::from_secs(60);

        loop {
            match self.processor.arch_client.is_node_ready().await {
                Ok(true) => return Ok(()),
                _ => {
                    error!("Node connection lost. Retrying in {} seconds", delay.as_secs());
                    tokio::time::sleep(delay).await;
                    delay = std::cmp::min(delay * 2, max_delay);
                }
            }
        }
    }
}