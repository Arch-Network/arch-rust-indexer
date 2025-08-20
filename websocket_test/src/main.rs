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

    // Allow testing against different endpoints
    let url = std::env::var("WEBSOCKET_URL").unwrap_or_else(|_| {
        "ws://44.196.173.35:10081".to_string()
    });
    
    info!("Testing WebSocket connection to: {}", url);
    info!("Set WEBSOCKET_URL env var to test different endpoints");
    info!("");
    info!("📋 Testing Options:");
    info!("  1. Local validator: ws://localhost:8081 (if running with --websocket)");
    info!("  2. New server: ws://44.196.173.35:10081 (WebSocket port)");
    info!("  3. Beta server: wss://rpc-beta.test.arch.network/ws (limited API)");
    info!("  4. Custom endpoint: Set WEBSOCKET_URL env var");
    info!("");
    
    // Parse the URL
    let url = Url::parse(&url)?;
    
    // Attempt to connect
    info!("Attempting to connect to WebSocket...");
    match connect_async(url).await {
        Ok((ws_stream, _)) => {
            info!("✅ WebSocket connection established successfully!");
            
            let (mut write, mut read) = ws_stream.split();
            
            // Try different methods to discover what the server supports
            let methods_to_try = vec![
                ("subscribe", json!({
                    "method": "subscribe",
                    "params": {
                        "topic": "block",
                        "filter": {},
                        "request_id": "test1"
                    }
                })),
                ("ping", json!({
                    "method": "ping",
                    "params": {},
                    "request_id": "test2"
                })),
                ("getInfo", json!({
                    "method": "getInfo",
                    "params": {},
                    "request_id": "test3"
                })),
                ("getVersion", json!({
                    "method": "getVersion",
                    "params": {},
                    "request_id": "test4"
                })),
                ("getBlockHeight", json!({
                    "method": "getBlockHeight",
                    "params": {},
                    "request_id": "test5"
                })),
                // Try some common RPC methods
                ("getBlockCount", json!({
                    "method": "getBlockCount",
                    "params": {},
                    "request_id": "test6"
                })),
                ("getBestBlockHash", json!({
                    "method": "getBestBlockHash",
                    "params": {},
                    "request_id": "test7"
                })),
            ];
            
            // Try each method
            for (method_name, message) in methods_to_try {
                info!("🔍 Trying method: {}", method_name);
                info!("📤 Sending: {}", serde_json::to_string_pretty(&message).unwrap());
                
                if let Err(e) = write.send(Message::Text(message.to_string())).await {
                    error!("Failed to send {} message: {}", method_name, e);
                    continue;
                }
                
                // Wait a bit for response
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            }
            
            info!("✅ All methods tried, now listening for responses...");
            
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
