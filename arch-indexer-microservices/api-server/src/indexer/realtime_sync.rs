use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use super::block_processor::BlockProcessor;
use crate::arch_rpc::{
    websocket::{WebSocketClient, WebSocketEvent, BlockEvent, TransactionEvent, AccountEvent},
    ArchRpcClient,
};

pub struct RealtimeSync {
    processor: Arc<BlockProcessor>,
    websocket_client: WebSocketClient,
    arch_client: Arc<ArchRpcClient>,
}

impl RealtimeSync {
    pub fn new(
        processor: Arc<BlockProcessor>,
        websocket_url: String,
        arch_client: Arc<ArchRpcClient>,
    ) -> Self {
        Self {
            processor,
            websocket_client: WebSocketClient::new(websocket_url),
            arch_client,
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting real-time sync via WebSocket...");
        
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<WebSocketEvent>();
        
        // Spawn WebSocket connection task
        let websocket_client = self.websocket_client.clone();
        let websocket_handle = tokio::spawn(async move {
            if let Err(e) = websocket_client.connect_and_listen(event_tx).await {
                error!("WebSocket connection failed: {}", e);
            }
        });
        
        // Process incoming events
        let processor = Arc::clone(&self.processor);
        let arch_client = Arc::clone(&self.arch_client);
        
        let event_processor_handle = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(e) = Self::process_event(&processor, &arch_client, event).await {
                    error!("Failed to process WebSocket event: {}", e);
                }
            }
        });
        
        // Wait for either task to complete
        tokio::select! {
            _ = websocket_handle => {
                warn!("WebSocket connection task ended");
            }
            _ = event_processor_handle => {
                warn!("Event processor task ended");
            }
        }
        
        Ok(())
    }
    
    async fn process_event(
        processor: &Arc<BlockProcessor>,
        arch_client: &Arc<ArchRpcClient>,
        event: WebSocketEvent,
    ) -> Result<()> {
        match event.topic.as_str() {
            "blocks" => {
                if let Ok(block_event) = serde_json::from_value::<BlockEvent>(event.data.clone()) {
                    info!("Processing real-time block: {} at height {}", 
                          block_event.hash, block_event.height);
                    
                    // Get the full block data from RPC
                    match arch_client.get_block(&block_event.hash, block_event.height).await {
                        Ok(block) => {
                            if let Err(e) = processor.process_block_direct(block).await {
                                error!("Failed to process real-time block {}: {}", block_event.hash, e);
                            } else {
                                info!("Successfully processed real-time block {} at height {}", 
                                      block_event.hash, block_event.height);
                            }
                        }
                        Err(e) => {
                            error!("Failed to get block data for real-time block {}: {}", 
                                   block_event.hash, e);
                        }
                    }
                }
            }
            "transactions" => {
                if let Ok(tx_event) = serde_json::from_value::<TransactionEvent>(event.data.clone()) {
                    info!("Processing real-time transaction: {} with status {}", 
                          tx_event.hash, tx_event.status);
                    
                    // For now, we'll rely on block processing to handle transactions
                    // But we could implement direct transaction processing here if needed
                    if tx_event.status == "processed" {
                        // Transaction is already processed as part of a block
                        info!("Transaction {} already processed as part of block", tx_event.hash);
                    }
                }
            }
            "accounts" => {
                if let Ok(account_event) = serde_json::from_value::<AccountEvent>(event.data.clone()) {
                    info!("Account update: {} (program: {:?})", 
                          account_event.address, account_event.program_id);
                    
                    // Handle account updates if needed
                    // This could trigger additional indexing logic
                }
            }
            _ => {
                info!("Received unknown event topic: {}", event.topic);
            }
        }
        
        Ok(())
    }
}

impl Clone for RealtimeSync {
    fn clone(&self) -> Self {
        Self {
            processor: Arc::clone(&self.processor),
            websocket_client: self.websocket_client.clone(),
            arch_client: Arc::clone(&self.arch_client),
        }
    }
}


