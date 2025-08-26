use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};
use url::Url;

use crate::config::settings::WebSocketSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketEvent {
    pub topic: String,
    pub data: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct WebSocketClient {
    settings: WebSocketSettings,
    url: String,
}

impl WebSocketClient {
    pub fn new(settings: WebSocketSettings, url: String) -> Self { Self { settings, url } }

    pub async fn connect_and_listen(&self, tx: mpsc::UnboundedSender<WebSocketEvent>) -> Result<()> {
        let url = Url::parse(&self.url)?;
        let mut attempts = 0usize;

        loop {
            info!("WebSocket connecting to {} (attempt {} of {})", self.url, attempts + 1, self.settings.max_reconnect_attempts);
            match connect_async(url.clone()).await {
                Ok((mut ws_stream, _)) => {
                    info!("WebSocket connected to {}", self.url);
                    attempts = 0; // reset on success

                    // Try to subscribe to standard topics if server expects a subscription
                    // Support JSON-RPC style subscribe used by Arch nodes
                    let subscribe_msg = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "subscribe",
                        "params": ["blocks", "transactions", "accounts", "rolledback_transactions"],
                        "id": 1
                    })
                    .to_string();
                    if let Err(e) = ws_stream.send(Message::Text(subscribe_msg)).await {
                        warn!("Failed to send WS subscribe message: {}", e);
                    } else {
                        info!("WS subscription message sent");
                    }

                    while let Some(msg) = ws_stream.next().await {
                        match msg {
                            Ok(Message::Text(text)) => {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                    // Accept one of:
                                    // 1) {"topic":"block","data":{...}}
                                    // 2) {"result":{"topic":"block","data":{...}}}
                                    // 3) {"method":"subscription","params":{"result":{"topic":"block","data":{...}}}}
                                    let mut topic: Option<String> = None;
                                    let mut data: Option<serde_json::Value> = None;

                                    if let Some(result) = json.get("result") {
                                        topic = result.get("topic").and_then(|t| t.as_str()).map(|s| s.to_string());
                                        data = result.get("data").cloned();
                                    }
                                    if topic.is_none() && json.get("topic").is_some() {
                                        topic = json.get("topic").and_then(|t| t.as_str()).map(|s| s.to_string());
                                        data = json.get("data").cloned();
                                    }
                                    if topic.is_none() {
                                        if let Some(method) = json.get("method").and_then(|m| m.as_str()) {
                                            if method == "subscription" {
                                                if let Some(params) = json.get("params") {
                                                    if let Some(result) = params.get("result") {
                                                        topic = result.get("topic").and_then(|t| t.as_str()).map(|s| s.to_string());
                                                        data = result.get("data").cloned();
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if let Some(t) = topic {
                                        let _ = tx.send(WebSocketEvent { topic: t, data: data.unwrap_or(serde_json::Value::Null), timestamp: chrono::Utc::now() });
                                    } else {
                                        // unrecognized message shape; ignore
                                    }
                                } else {
                                    warn!("Failed to parse WS text as JSON: {}", text);
                                }
                            }
                            Ok(Message::Binary(_bin)) => {
                                // ignore binary for now
                            }
                            Ok(Message::Ping(p)) => {
                                // let _ = ws_stream.send(Message::Pong(p)).await;
                                drop(p);
                            }
                            Ok(Message::Close(frame)) => {
                                warn!("WebSocket closed: {:?}", frame);
                                break;
                            }
                            Err(e) => {
                                warn!("WebSocket error: {}", e);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    warn!("WebSocket connect error: {}", e);
                }
            }

            attempts += 1;
            if attempts >= self.settings.max_reconnect_attempts {
                error!("Max WebSocket reconnect attempts reached; giving up");
                return Err(anyhow::anyhow!("WebSocket reconnect attempts exhausted"));
            }

            let delay = self.settings.reconnect_interval_seconds.max(1);
            info!("Reconnecting WebSocket in {}s...", delay);
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        }
    }
}
