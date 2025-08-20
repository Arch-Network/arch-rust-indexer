use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let url = "ws://44.196.173.35:10081";
    
    info!("Testing WebSocket connection to: {}", url);
    info!("Trying different message formats...");
    
    // Parse the URL
    let url = Url::parse(url)?;
    
    // Attempt to connect
    info!("Attempting to connect to WebSocket...");
    match connect_async(url).await {
        Ok((ws_stream, _)) => {
            info!("✅ WebSocket connection established successfully!");
            
            let (mut write, mut read) = ws_stream.split();
            
            // Try different message formats
            let formats_to_try = vec![
                // Format 1: Correct Arch Network format (this should work!)
                ("Correct Arch Format", json!({
                    "method": "subscribe",
                    "params": {
                        "topic": "block",
                        "filter": {},
                        "request_id": "test1"
                    }
                })),
                
                // Format 2: Alternative topic
                ("Transaction Topic", json!({
                    "method": "subscribe",
                    "params": {
                        "topic": "transaction",
                        "filter": {},
                        "request_id": "test2"
                    }
                })),
                
                // Format 3: With filter
                ("With Filter", json!({
                    "method": "subscribe",
                    "params": {
                        "topic": "block",
                        "filter": {"height": "latest"},
                        "request_id": "test3"
                    }
                })),
                
                // Format 4: Minimal format
                ("Minimal", json!({
                    "method": "subscribe",
                    "params": {
                        "topic": "block"
                    }
                })),
                
                // Format 5: Different topic
                ("Account Update", json!({
                    "method": "subscribe",
                    "params": {
                        "topic": "account_update",
                        "filter": {},
                        "request_id": "test5"
                    }
                })),
                
                // Format 6: Legacy JSON-RPC format (for comparison)
                ("Legacy JSON-RPC", json!({
                    "jsonrpc": "2.0",
                    "method": "subscribe",
                    "params": ["blocks"],
                    "id": 6
                })),
                
                // Format 7: Simple format (for comparison)
                ("Simple", json!({
                    "method": "subscribe",
                    "topic": "blocks"
                })),
            ];
            
            // Try each format
            for (format_name, message) in formats_to_try {
                info!("🔍 Trying format: {}", format_name);
                
                let message_text = if message.is_string() {
                    message.as_str().unwrap().to_string()
                } else {
                    serde_json::to_string(&message).unwrap()
                };
                
                info!("📤 Sending: {}", message_text);
                
                if let Err(e) = write.send(Message::Text(message_text)).await {
                    error!("Failed to send {} message: {}", format_name, e);
                    continue;
                }
                
                // Wait a bit for response
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            }
            
            info!("✅ All formats tried, now listening for responses...");
            
            // Listen for incoming messages
            let duration = std::time::Duration::from_secs(30);
            let timeout = tokio::time::sleep(duration);
            let mut event_count = 0;
            
            tokio::select! {
                _ = timeout => {
                    info!("⏰ Test duration completed, received {} events", event_count);
                }
                _ = async {
                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(Message::Text(text)) => {
                                event_count += 1;
                                info!("📨 Response {}: {}", event_count, text);
                                
                                // Try to parse as JSON for better formatting
                                if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&text) {
                                    info!("  📊 Parsed JSON: {}", serde_json::to_string_pretty(&json_value).unwrap_or_else(|_| "Invalid JSON".to_string()));
                                }
                            }
                            Ok(Message::Close(_)) => {
                                info!("🔒 WebSocket connection closed by server");
                                break;
                            }
                            Ok(Message::Ping(data)) => {
                                info!("🏓 Received ping, sending pong");
                                if let Err(e) = write.send(Message::Pong(data)).await {
                                    error!("Failed to send pong: {}", e);
                                    break;
                                }
                            }
                            Ok(Message::Pong(_)) => {
                                info!("🏓 Received pong");
                            }
                            Ok(Message::Binary(_)) => {
                                info!("📦 Received binary message");
                            }
                            Ok(Message::Frame(_)) => {
                                info!("🖼️ Received raw frame");
                            }
                            Err(e) => {
                                error!("❌ WebSocket error: {}", e);
                                break;
                            }
                        }
                    }
                } => {
                    info!("WebSocket stream ended");
                }
            }
            
            info!("WebSocket test completed successfully");
        }
        Err(e) => {
            error!("❌ Failed to connect to WebSocket: {}", e);
            return Err(anyhow::anyhow!("Connection failed: {}", e));
        }
    }
    
    Ok(())
}
