use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tokio::task::spawn;
use tracing::{error, info, warn};

use crate::arch_rpc::websocket::{WebSocketClient, WebSocketEvent};
use crate::arch_rpc::ArchRpcClient;
use crate::config::settings::Settings;
use crate::indexer::realtime_processor::RealtimeProcessor;
use crate::api::websocket_server::WebSocketServer;
use crate::indexer::sync::ChainSync;
use crate::indexer::block_processor::BlockProcessor;
use sqlx::PgPool;

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
            self.start_realtime_sync().await?;
        } else {
            info!("⚠️ Real-time sync disabled, using traditional polling only");
        }

        // Always start traditional sync as fallback
        self.start_traditional_sync().await?;

        Ok(())
    }

    async fn start_realtime_sync(&self) -> Result<()> {
        let websocket_url = self.settings.arch_node.websocket_url.clone();
        let websocket_settings = self.settings.websocket.clone();
        
        // Create channels for communication
        let (event_tx, event_rx) = mpsc::channel::<WebSocketEvent>(1000);
        let (_status_tx, mut status_rx) = mpsc::channel::<RealtimeStatus>(100);

        // Start WebSocket client
        let websocket_client = WebSocketClient::new(websocket_settings, websocket_url);
        let client_handle = spawn(async move {
            if let Err(e) = websocket_client.start(event_tx).await {
                error!("WebSocket client failed: {}", e);
            }
        });

        // Start real-time processor
        let rpc_client = Arc::new(ArchRpcClient::new(self.settings.arch_node.url.clone()));
        // Attempt to get a global websocket server if one exists in application state later;
        // for now pass None and allow main to wire a server-enabled instance when available.
        let processor = RealtimeProcessor::new(Arc::clone(&self.pool), rpc_client, None);
        let processor_handle = tokio::spawn(async move {
            if let Err(e) = processor.start(event_rx).await {
                error!("Real-time processor failed: {}", e);
            }
        });

        // Start status monitor
        let status_monitor_handle = tokio::spawn(async move {
            while let Some(status) = status_rx.recv().await {
                match status {
                    RealtimeStatus::BlockReceived { height, timestamp } => {
                        info!("📦 Real-time block received: height={}, timestamp={}", height, timestamp);
                    }
                    RealtimeStatus::TransactionReceived { hash, timestamp } => {
                        info!("💳 Real-time transaction received: hash={}, timestamp={}", hash, timestamp);
                    }
                    RealtimeStatus::ConnectionStatus { connected } => {
                        if connected {
                            info!("🔗 WebSocket connection established");
                        } else {
                            warn!("🔌 WebSocket connection lost");
                        }
                    }
                }
            }
        });

        // Wait for all components
        tokio::select! {
            _ = client_handle => {
                error!("WebSocket client task ended unexpectedly");
            }
            _ = processor_handle => {
                error!("Real-time processor task ended unexpectedly");
            }
            _ = status_monitor_handle => {
                error!("Status monitor task ended unexpectedly");
            }
            // Add timeout to prevent indefinite hanging
            _ = tokio::time::sleep(Duration::from_secs(300)) => { // 5 minute timeout
                warn!("HybridSync real-time sync timeout reached, continuing with traditional sync");
            }
        }

        Ok(())
    }

    async fn start_traditional_sync(&self) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let current_height = Arc::clone(&self.current_height);
        let is_realtime_active = Arc::clone(&self.is_realtime_active);
        let last_realtime_update = Arc::clone(&self.last_realtime_update);
        let settings = Arc::clone(&self.settings);

        tokio::spawn(async move {
            info!("🚀 Starting traditional bulk sync for historical blocks...");
            
            // Create RPC client for fetching blocks
            let rpc_client = Arc::new(ArchRpcClient::new(settings.arch_node.url.clone()));
            
            loop {
                let realtime_active = is_realtime_active.load(Ordering::Relaxed);
                let last_update = last_realtime_update.load(Ordering::Relaxed);
                let now = chrono::Utc::now().timestamp();

                if realtime_active && (now - last_update) < 30 {
                    // Real-time sync is active and recent, use shorter polling interval
                    info!("🔄 Real-time sync active, using short polling interval");
                    sleep(Duration::from_secs(5)).await;
                } else {
                    // Real-time sync inactive or stale, run bulk sync
                    info!("🔄 Running bulk sync for historical blocks...");
                    
                    if let Err(e) = run_bulk_sync(&pool, &rpc_client).await {
                        error!("Bulk sync failed: {}", e);
                        sleep(Duration::from_secs(60)).await; // Wait longer on error
                    } else {
                        sleep(Duration::from_secs(30)).await;
                    }
                }
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

/// Run bulk sync to fetch and process historical blocks
async fn run_bulk_sync(pool: &PgPool, rpc_client: &Arc<ArchRpcClient>) -> Result<()> {
    info!("🔄 Starting bulk sync for historical blocks...");
    
    // Get the last processed block height from database
    let last_processed_height = sqlx::query!(
        "SELECT COALESCE(MAX(height), -1) as height FROM blocks"
    )
    .fetch_one(pool)
    .await?
    .height
    .unwrap_or(-1); // Handle Option<i64>
    
    // Get current network height from RPC
    let network_height = rpc_client.get_block_count().await?;
    
    info!("📊 Bulk sync: last processed height={}, network height={}", 
          last_processed_height, network_height);
    
    if last_processed_height >= network_height {
        info!("✅ Already synced to latest height");
        return Ok(());
    }
    
    // Calculate how many blocks we need to sync
    let blocks_to_sync = network_height - last_processed_height;
    info!("🔄 Need to sync {} blocks", blocks_to_sync);
    
    // Create block processor with dummy Redis client (not actually used)
    let dummy_redis = redis::Client::open("redis://127.0.0.1:6379")
        .unwrap_or_else(|_| redis::Client::open("redis://127.0.0.1:6379").unwrap());
    
    let processor = BlockProcessor::new(
        pool.clone(),
        dummy_redis,
        rpc_client.clone(),
    );
    
    // Sync in batches to avoid overwhelming the system
    let batch_size = 10; // Process 10 blocks at a time
    let mut current_height = last_processed_height + 1;
    
    while current_height <= network_height {
        let end_height = std::cmp::min(current_height + batch_size - 1, network_height);
        let batch_heights: Vec<i64> = (current_height..=end_height).collect();
        
        info!("🔄 Processing batch: heights {} to {}", current_height, end_height);
        
        // Process this batch of blocks
        match processor.process_blocks_batch(batch_heights).await {
            Ok(blocks) => {
                info!("✅ Successfully processed {} blocks in batch", blocks.len());
                current_height = end_height + 1;
            }
            Err(e) => {
                error!("❌ Failed to process batch starting at height {}: {}", current_height, e);
                // Try to continue with next batch, but log the error
                current_height = end_height + 1;
            }
        }
        
        // Small delay between batches to be nice to the RPC
        sleep(Duration::from_millis(100)).await;
    }
    
    info!("🎉 Bulk sync completed! Synced from height {} to {}", 
          last_processed_height + 1, network_height);
    
    Ok(())
}

#[derive(Debug, Clone)]
pub enum RealtimeStatus {
    BlockReceived { height: i64, timestamp: i64 },
    TransactionReceived { hash: String, timestamp: i64 },
    ConnectionStatus { connected: bool },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_atomic_operations() {
        let current_height = Arc::new(AtomicI64::new(0));
        let is_realtime_active = Arc::new(AtomicBool::new(false));
        let last_realtime_update = Arc::new(AtomicI64::new(0));

        // Test atomic operations
        current_height.store(100, Ordering::Relaxed);
        is_realtime_active.store(true, Ordering::Relaxed);
        last_realtime_update.store(1234567890, Ordering::Relaxed);

        assert_eq!(current_height.load(Ordering::Relaxed), 100);
        assert_eq!(is_realtime_active.load(Ordering::Relaxed), true);
        assert_eq!(last_realtime_update.load(Ordering::Relaxed), 1234567890);
    }
}
