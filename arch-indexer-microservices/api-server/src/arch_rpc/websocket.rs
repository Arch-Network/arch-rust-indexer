use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};
use url::Url;

use crate::config::settings::WebSocketSettings;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WebSocketEvent {
    pub topic: String,
    pub data: Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub struct WebSocketClient {
    settings: WebSocketSettings,
    url: String,
}

impl WebSocketClient {
    pub fn new(settings: WebSocketSettings, url: String) -> Self {
        Self { settings, url }
    }

    pub async fn start(&self, event_tx: mpsc::Sender<WebSocketEvent>) -> Result<()> {
        let url = Url::parse(&self.url)?;
        
        info!("Starting WebSocket client for: {}", self.url);
        
        let mut attempt_count = 0;
        loop {
            attempt_count += 1;
            info!("WebSocket connection attempt #{}", attempt_count);
            
            match self.connect_and_subscribe(&url, &event_tx).await {
                Ok(_) => {
                    info!("WebSocket connection closed, attempting to reconnect...");
                }
                Err(e) => {
                    error!("WebSocket connection error (attempt #{}): {}", attempt_count, e);
                    
                    // Check if we've exceeded max reconnection attempts
                    if attempt_count >= self.settings.max_reconnect_attempts {
                        error!("Exceeded maximum reconnection attempts ({}), stopping WebSocket client", self.settings.max_reconnect_attempts);
                        break;
                    }
                }
            }

            if !self.settings.enabled {
                info!("WebSocket client disabled, stopping");
                break;
            }

            info!("Waiting {} seconds before reconnection attempt {}...", self.settings.reconnect_interval_seconds, attempt_count + 1);
            tokio::time::sleep(tokio::time::Duration::from_secs(self.settings.reconnect_interval_seconds)).await;
        }

        info!("WebSocket client stopped");
        Ok(())
    }

    async fn connect_and_subscribe(
        &self,
        url: &Url,
        event_tx: &mpsc::Sender<WebSocketEvent>,
    ) -> Result<()> {
        info!("Connecting to WebSocket: {}", url);
        
        // Add timeout to prevent hanging
        let connect_future = connect_async(url);
        let timeout_duration = tokio::time::Duration::from_secs(30); // 30 second timeout
        
        let (ws_stream, _) = tokio::time::timeout(timeout_duration, connect_future).await
            .map_err(|_| anyhow::anyhow!("WebSocket connection timeout after {} seconds", timeout_duration.as_secs()))??;
            
        info!("✅ WebSocket connection established successfully!");

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to ALL available topics
        let topics = vec![
            "block",
            "transaction", 
            "account_update",
            "rolledback_transactions",
            "reapplied_transactions",
            "dkg",
        ];

        info!("Subscribing to {} topics: {:?}", topics.len(), topics);

        for topic in topics {
            let subscribe_msg = json!({
                "method": "subscribe",
                "params": {
                    "topic": topic,
                    "filter": {},
                    "request_id": format!("sub_{}", topic)
                }
            });

            info!("📤 Subscribing to topic: {}", topic);
            if let Err(e) = write.send(Message::Text(subscribe_msg.to_string())).await {
                error!("Failed to send subscription for {}: {}", topic, e);
                continue;
            }

            // Small delay between subscriptions
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        info!("✅ All subscriptions sent, now listening for events...");

        // Listen for incoming events
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(event) = self.parse_event(&text) {
                        if let Err(e) = event_tx.send(event).await {
                            error!("Failed to send event to processor: {}", e);
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket connection closed by server");
                    break;
                }
                Ok(Message::Ping(data)) => {
                    if let Err(e) = write.send(Message::Pong(data)).await {
                        error!("Failed to send pong: {}", e);
                        break;
                    }
                }
                Ok(Message::Pong(_)) => {
                    // Handle pong if needed
                }
                Ok(Message::Binary(_)) => {
                    info!("Received binary message (ignoring)");
                }
                Ok(Message::Frame(_)) => {
                    info!("Received raw frame (ignoring)");
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
            }
            
            // Add a small delay to prevent tight loops
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        Ok(())
    }

    fn parse_event(&self, text: &str) -> Result<WebSocketEvent> {
        let json_value: Value = serde_json::from_str(text)?;
        
        // Handle subscription confirmation messages
        if let Some(status) = json_value.get("status") {
            if status == "Subscribed" {
                info!("✅ Successfully subscribed to topic: {}", 
                    json_value.get("topic").unwrap_or(&serde_json::Value::Null));
                return Err(anyhow::anyhow!("Subscription confirmation, not an event"));
            }
        }

        // Handle error messages
        if let Some(status) = json_value.get("status") {
            if status == "Error" {
                if let Some(error_msg) = json_value.get("error") {
                    warn!("WebSocket error: {}", error_msg);
                }
                return Err(anyhow::anyhow!("Error message, not an event"));
            }
        }

        // Parse actual event data
        let topic = json_value
            .get("topic")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing topic"))?;

        let data = json_value
            .get("data")
            .cloned()
            .unwrap_or(Value::Null);

        let timestamp = chrono::Utc::now();

        let event = WebSocketEvent {
            topic: topic.to_string(),
            data,
            timestamp,
        };

        info!("📨 Received {} event: {:?}", topic, event);
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_event() {
        let client = WebSocketClient::new(WebSocketSettings::default(), "ws://localhost:8081".to_string());
        
        let event_json = r#"{
            "result": {
                "topic": "blocks",
                "data": {"hash": "test123", "height": 100},
                "timestamp": 1234567890
            }
        }"#;
        
        let event = client.parse_event(event_json).unwrap();
        assert_eq!(event.topic, "blocks");
        assert_eq!(event.timestamp, chrono::Utc::now());
    }
}
