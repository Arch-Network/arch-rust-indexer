use super::block_processor::BlockProcessor;
use super::hybrid_sync::HybridSync;
use anyhow::Result;
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};

pub struct ChainSync {
    pub processor: Arc<BlockProcessor>,
    current_height: AtomicI64,
    batch_size: usize,
    concurrent_batches: usize,
    realtime_sync: Option<HybridSync>,
    hybrid_mode: bool,
}

impl ChainSync {
    pub fn new(
        processor: Arc<BlockProcessor>,
        starting_height: i64,
        batch_size: usize,
        concurrent_batches: usize,
        realtime_sync: Option<HybridSync>,
        hybrid_mode: bool,
    ) -> Self {
        info!("Initializing ChainSync with starting height: {}, batch_size: {}, concurrent_batches: {}, hybrid_mode: {}", 
              starting_height, batch_size, concurrent_batches, hybrid_mode);
        
        Self {
            processor,
            current_height: AtomicI64::new(starting_height),
            batch_size,
            concurrent_batches,
            realtime_sync,
            hybrid_mode,
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting chain sync...");
        
        // Start real-time sync if available
        if let Some(realtime_sync) = &self.realtime_sync {
            let realtime_sync = realtime_sync.clone();
            tokio::spawn(async move {
                if let Err(e) = realtime_sync.start().await {
                    error!("Real-time sync failed: {}", e);
                }
            });
            info!("Real-time sync started in background");
        }
        
        // Get current network height
        let network_height = self.processor.arch_client.get_block_count().await?;
        info!("Current network height: {}", network_height);
        
        let current = self.current_height.load(Ordering::Relaxed);
        let mut target_height = network_height;
        
        // Always start from the configured starting height (typically 0 for genesis)
        if current == 0 {
            info!("Starting fresh sync from genesis (height 0)");
        }
        
        let missing_blocks_check_interval = Duration::from_secs(300); // Check every 5 minutes
        let missing_programs_check_interval = Duration::from_secs(600); // Check every 10 minutes
        let health_check_interval = Duration::from_secs(60); // Health check every minute

        let mut last_missing_check = std::time::Instant::now();
        let mut last_programs_check = std::time::Instant::now();
        let mut last_health_check = std::time::Instant::now();
        let mut consecutive_errors = 0;
        let max_consecutive_errors = 5;

        loop {
            // Health check - verify node is still responsive
            if last_health_check.elapsed() >= health_check_interval {
                if let Ok(ready) = self.processor.arch_client.is_node_ready().await {
                    if !ready {
                        warn!("Node not ready, waiting for reconnection...");
                        if let Err(e) = self.wait_for_connection().await {
                            error!("Failed to reconnect: {}", e);
                            consecutive_errors += 1;
                        }
                    } else {
                        consecutive_errors = 0; // Reset error counter on successful health check
                    }
                }
                last_health_check = std::time::Instant::now();
            }

            // Check if we need to update target height
            if let Ok(new_height) = self.processor.arch_client.get_block_count().await {
                if new_height > target_height {
                    info!("Network height updated: {} -> {}", target_height, new_height);
                    target_height = new_height;
                }
            }

            match self.sync_blocks().await {
                Ok(_) => {
                    consecutive_errors = 0; // Reset error counter on successful sync
                    
                    // Check for missing blocks periodically
                    if last_missing_check.elapsed() >= missing_blocks_check_interval {
                        if let Ok(missing) = self.check_for_missing_blocks().await {
                            if !missing.is_empty() {
                                info!("Found {} missing blocks, processing...", missing.len());
                                if let Err(e) = self.process_missing_blocks(missing).await {
                                    error!("Error processing missing blocks: {}", e);
                                }
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

                    // In hybrid mode, use shorter delays since real-time sync handles new blocks
                    let delay = if self.hybrid_mode && self.realtime_sync.is_some() {
                        Duration::from_millis(50) // Faster polling in hybrid mode
                    } else {
                        Duration::from_millis(100) // Standard polling
                    };
                    
                    // Only add delay if we're caught up (not in bulk sync mode)
                    if current >= target_height - 1000 {
                        sleep(delay).await;
                    }
                    continue;
                }
                Err(e) => {
                    consecutive_errors += 1;
                    error!("Sync error (attempt {}/{}): {}", consecutive_errors, max_consecutive_errors, e);
                    
                    if consecutive_errors >= max_consecutive_errors {
                        error!("Too many consecutive errors, attempting to reconnect...");
                        if let Err(e) = self.wait_for_connection().await {
                            error!("Failed to reconnect: {}", e);
                            // Wait longer before retrying
                            sleep(Duration::from_secs(30)).await;
                        }
                        consecutive_errors = 0;
                    } else {
                        // Exponential backoff for transient errors
                        let delay = Duration::from_secs(2_u64.pow(consecutive_errors as u32));
                        warn!("Waiting {} seconds before retry...", delay.as_secs());
                        sleep(delay).await;
                    }
                    continue;
                }
            }
        }
    }

    async fn process_missing_blocks(&self, missing_heights: Vec<i64>) -> Result<()> {
        info!("Processing {} missing blocks...", missing_heights.len());
        
        // Process missing blocks in smaller batches to avoid overwhelming the system
        let batch_size = 100;
        for chunk in missing_heights.chunks(batch_size) {
            let mut futures = Vec::new();
            
            for &height in chunk {
                let processor = Arc::clone(&self.processor);
                let future = async move {
                    if let Ok(hash) = processor.arch_client.get_block_hash(height).await {
                        if let Ok(block) = processor.arch_client.get_block(&hash, height).await {
                            if let Err(e) = processor.process_block_direct(block).await {
                                error!("Error processing missing block {}: {}", height, e);
                            }
                        }
                    }
                };
                futures.push(future);
            }
            
            // Process chunk concurrently
            futures_util::future::join_all(futures).await;
            
            // Small delay between chunks
            sleep(Duration::from_millis(50)).await;
        }
        
        info!("Finished processing missing blocks");
        Ok(())
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
        
        if min_height == 0 && max_height == 0 {
            return Ok(Vec::new()); // No blocks yet
        }
        
        let chunk_size = 100_000; // Check 100k blocks at a time
        let mut missing_blocks = Vec::new();

        // Process in chunks
        for chunk_start in (min_height..=max_height).step_by(chunk_size) {
            let chunk_end = (chunk_start + chunk_size as i64 - 1).min(max_height);
            
            let heights = sqlx::query!(
                r#"
                SELECT height FROM blocks 
                WHERE height >= $1 AND height <= $2 
                ORDER BY height
                "#,
                chunk_start,
                chunk_end
            )
            .fetch_all(&self.processor.pool)
            .await?;

            let existing_heights: std::collections::HashSet<i64> = heights.iter().map(|r| r.height).collect();
            
            for height in chunk_start..=chunk_end {
                if !existing_heights.contains(&height) {
                    missing_blocks.push(height);
                }
            }
            
            // Small delay between chunks to avoid overwhelming the database
            sleep(Duration::from_millis(10)).await;
        }

        info!("Found {} missing blocks between heights {} and {}", missing_blocks.len(), min_height, max_height);
        Ok(missing_blocks)
    }

    async fn sync_blocks(&self) -> Result<()> {
        let start_time = std::time::Instant::now();
        let current = self.current_height.load(Ordering::Relaxed);
        let target_height = self.processor.arch_client.get_block_count().await?;
        
        if current >= target_height {
            // Wait for new blocks, but only briefly during bulk sync
            let wait_time = if target_height - current < 1000 { Duration::from_secs(1) } else { Duration::from_millis(100) };
            sleep(wait_time).await;
            return Ok(());
        }

        let end_height = (current + self.batch_size as i64).min(target_height);
        let heights: Vec<i64> = (current..=end_height).collect();
        
        info!("Syncing blocks {} to {} ({} blocks)", current, end_height, heights.len());

        // Determine if we're in bulk sync mode (far behind or configured)
        let is_bulk_sync = target_height - current > 10000;
        let chunk_size = if is_bulk_sync { self.concurrent_batches * 2 } else { self.concurrent_batches };
        
        // Process blocks in concurrent batches
        let chunks: Vec<Vec<i64>> = heights.chunks(chunk_size).map(|c| c.to_vec()).collect();
        
        // In bulk sync mode, process chunks more aggressively
        let chunk_delay = if is_bulk_sync { Duration::from_millis(10) } else { Duration::from_millis(50) };
        
        for (chunk_index, chunk) in chunks.iter().enumerate() {
            let mut futures = Vec::new();
            
            for &height in chunk {
                let processor = Arc::clone(&self.processor);
                let future = async move {
                    match processor.arch_client.get_block_hash(height).await {
                        Ok(hash) => {
                            match processor.arch_client.get_block(&hash, height).await {
                                Ok(block) => {
                                    if let Err(e) = processor.process_block_direct(block).await {
                                        error!("Error processing block {}: {}", height, e);
                                        return Err(e);
                                    }
                                    Ok(())
                                },
                                Err(e) => {
                                    error!("Error getting block {}: {}", height, e);
                                    Err(e)
                                }
                            }
                        },
                        Err(e) => {
                            error!("Error getting block hash for height {}: {}", height, e);
                            Err(e)
                        }
                    }
                };
                futures.push(future);
            }
            
            // Process chunk concurrently
            let results = futures_util::future::join_all(futures).await;
            
            // Check for errors
            let errors: Vec<_> = results.iter().filter_map(|r| r.as_ref().err()).collect();
            if !errors.is_empty() {
                warn!("{} errors in batch, continuing with next batch", errors.len());
            }
            
            // Small delay between chunks to prevent overwhelming the system
            if chunk_index < chunks.len() - 1 {
                sleep(chunk_delay).await;
            }
        }

        // Update current height
        self.current_height.store(end_height + 1, Ordering::Relaxed);
        
        // Calculate and log sync progress
        let progress = ((end_height as f64 / target_height as f64) * 100.0).min(100.0);
        let elapsed = start_time.elapsed();
        let blocks_per_second = heights.len() as f64 / elapsed.as_secs_f64();
        let remaining_blocks = target_height - end_height;
        let estimated_time_seconds = remaining_blocks as f64 / blocks_per_second;
        
        info!("Completed sync batch: height {} -> {} ({} blocks, {:.2}% complete)", 
              current, end_height, heights.len(), progress);
        info!("Sync speed: ~{:.0} blocks/sec, ETA: {:.1} hours", 
              blocks_per_second, estimated_time_seconds / 3600.0);
        
        Ok(())
    }

    async fn wait_for_connection(&self) -> Result<()> {
        info!("Waiting for node connection...");
        
        let mut attempts = 0;
        let max_attempts = 10;
        
        while attempts < max_attempts {
            if let Ok(ready) = self.processor.arch_client.is_node_ready().await {
                if ready {
                    info!("Node connection restored");
                    return Ok(());
                }
            }
            
            attempts += 1;
            let delay = Duration::from_secs(2_u64.pow(attempts as u32));
            warn!("Connection attempt {}/{} failed, waiting {} seconds...", attempts, max_attempts, delay.as_secs());
            sleep(delay).await;
        }
        
        Err(anyhow::anyhow!("Failed to restore connection after {} attempts", max_attempts))
    }
}