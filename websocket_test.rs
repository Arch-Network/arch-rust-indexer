use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};
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

    let url = "wss://rpc-beta.test.arch.network";
    
    info!("Testing WebSocket connection to: {}", url);
    info!("Will listen for 60 seconds");
    
    // Parse the URL
    let url = Url::parse(url)?;
    
    // Attempt to connect
    info!("Attempting to connect to WebSocket...");
    match connect_async(url).await {
        Ok((ws_stream, _)) => {
            info!("✅ WebSocket connection established successfully!");
            
            let (mut write, mut read) = ws_stream.split();
            
            // Subscribe to events
            let subscribe_msg = json!({
                "jsonrpc": "2.0",
                "method": "subscribe",
                "params": ["blocks", "transactions", "accounts"],
                "id": 1
            });
            
            info!("Sending subscription message: {}", subscribe_msg);
            if let Err(e) = write.send(Message::Text(subscribe_msg.to_string())).await {
                error!("Failed to send subscription message: {}", e);
                return Err(anyhow::anyhow!("Subscription failed: {}", e));
            }
            
            info!("✅ Subscription message sent successfully");
            info!("Listening for events...");
            
            // Listen for incoming messages
            let duration = std::time::Duration::from_secs(60);
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
                                info!("📨 Event {}: {}", event_count, text);
                                
                                // Try to parse as JSON for better formatting
                                if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&text) {
                                    if let Some(result) = json_value.get("result") {
                                        if let Some(topic) = result.get("topic").and_then(|t| t.as_str()) {
                                            info!("  📋 Topic: {}", topic);
                                            if let Some(data) = result.get("data") {
                                                info!("  📊 Data: {}", serde_json::to_string_pretty(data).unwrap_or_else(|_| "Invalid JSON".to_string()));
                                            }
                                        }
                                    } else if let Some(error) = json_value.get("error") {
                                        warn!("  ❌ Error: {}", serde_json::to_string_pretty(error).unwrap_or_else(|_| "Invalid JSON".to_string()));
                                    } else if let Some(id) = json_value.get("id") {
                                        info!("  🆔 Response ID: {}", id);
                                    }
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


