use anyhow::Result;
use clap::Parser;
use tokio::sync::mpsc;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use arch_indexer::arch_rpc::websocket::{WebSocketClient, WebSocketEvent};
use arch_indexer::config::settings::WebSocketSettings;

#[derive(Parser)]
struct Args {
    /// WebSocket URL to connect to
    #[arg(long, default_value = "ws://localhost:8081")]
    url: String,
    
    /// Duration to listen for events (in seconds)
    #[arg(long, default_value = "60")]
    duration: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();
    
    info!("Testing WebSocket connection to: {}", args.url);
    info!("Will listen for {} seconds", args.duration);
    
    let (event_tx, mut event_rx) = mpsc::channel::<WebSocketEvent>(100);
    
    // Create WebSocket client with default settings
    let settings = WebSocketSettings::default();
    let client = WebSocketClient::new(settings, args.url.clone());
    
    // Start connection in background
    let connection_handle = tokio::spawn(async move {
        if let Err(e) = client.start(event_tx).await {
            error!("WebSocket connection failed: {}", e);
        }
    });
    
    // Listen for events
    let duration = std::time::Duration::from_secs(args.duration);
    let timeout = tokio::time::sleep(duration);
    
    let mut event_count = 0;
    
    tokio::select! {
        _ = timeout => {
            info!("Test duration completed, received {} events", event_count);
        }
        _ = connection_handle => {
            error!("WebSocket connection ended prematurely");
        }
    }
    
    // Process any remaining events
    while let Ok(event) = event_rx.try_recv() {
        event_count += 1;
        info!("Event {}: topic={}, timestamp={}", 
              event_count, event.topic, event.timestamp);
        
        if event.topic == "blocks" {
            info!("  Block event: {:?}", event.data);
        } else if event.topic == "transactions" {
            info!("  Transaction event: {:?}", event.data);
        } else if event.topic == "accounts" {
            info!("  Account event: {:?}", event.data);
        }
    }
    
    info!("WebSocket test completed");
    Ok(())
} 
