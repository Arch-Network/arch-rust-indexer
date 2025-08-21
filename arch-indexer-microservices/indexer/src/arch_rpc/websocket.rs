use anyhow::Result;
use futures_util::StreamExt;
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

                    while let Some(msg) = ws_stream.next().await {
                        match msg {
                            Ok(Message::Text(text)) => {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                    let topic = json.get("topic").and_then(|t| t.as_str()).unwrap_or("").to_string();
                                    let data = json.get("data").cloned().unwrap_or(serde_json::Value::Null);
                                    let _ = tx.send(WebSocketEvent {
                                        topic,
                                        data,
                                        timestamp: chrono::Utc::now(),
                                    });
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
