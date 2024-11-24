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

        loop {
            // Continuously check for a new target_height every second
            sleep(Duration::from_secs(1)).await;
            let new_target_height = self.processor.arch_client.get_block_count().await?;
            if new_target_height > target_height {
                target_height = new_target_height;
            }

            if current > target_height {
                // If current is greater than target, continue polling for new blocks
                continue;
            }

            // Check for missing blocks every 10 iterations
            if current % 10 == 0 {
                if let Ok(missing_blocks) = self.check_for_missing_blocks().await {
                    if !missing_blocks.is_empty() {
                        info!("Found missing blocks: {:?}", missing_blocks);
                        // Fetch and process missing blocks
                        for height in missing_blocks {
                            self.processor.process_block(height).await?;
                        }
                    }
                }
            }

            // Existing batch processing logic
            let batch_starts: Vec<_> = (0..self.concurrent_batches)
                .map(|i| current + (i as i64 * self.batch_size as i64))
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
        // Query to find missing block heights
        let missing_blocks: Vec<i64> = query_scalar!(
            r#"
            WITH bounds AS (
                SELECT MIN(height) AS min_height, MAX(height) AS max_height
                FROM blocks
            )
            SELECT gs.height
            FROM generate_series((SELECT min_height FROM bounds), (SELECT max_height FROM bounds)) AS gs(height)
            WHERE NOT EXISTS (
                SELECT 1 FROM blocks WHERE height = gs.height
            )
            "#
        )
        .fetch_all(&self.processor.pool)
        .await?
        .into_iter()
        .filter_map(|height| height) // Filter out None values
        .collect();
    
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