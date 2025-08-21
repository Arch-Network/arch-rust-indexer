use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info, warn};

use crate::arch_rpc::websocket::WebSocketEvent;

#[derive(Debug, Clone)]
pub struct WebSocketServer {
    event_tx: broadcast::Sender<WebSocketEvent>,
    connections: Arc<RwLock<HashMap<String, broadcast::Sender<String>>>>,
}

impl WebSocketServer {
    pub fn new(event_tx: broadcast::Sender<WebSocketEvent>) -> Self {
        Self {
            event_tx,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn broadcast_event(&self, event: WebSocketEvent) -> Result<()> {
        let event_json = serde_json::to_string(&event)?;
        
        // Broadcast to all connected clients
        let connections = self.connections.read().await;
        for (client_id, tx) in connections.iter() {
            if let Err(e) = tx.send(event_json.clone()) {
                warn!("Failed to send event to client {}: {}", client_id, e);
            }
        }
        
        Ok(())
    }

    pub async fn handle_websocket(
        ws: WebSocketUpgrade,
        State(server): State<Arc<Self>>,
    ) -> impl IntoResponse {
        ws.on_upgrade(|socket| handle_socket(socket, server))
    }
}

async fn handle_socket(socket: WebSocket, self_: Arc<WebSocketServer>) {
    let (mut sender, mut receiver) = socket.split();
    
    let client_id = format!("client_id_{}", uuid::Uuid::new_v4());
    info!("🔌 New WebSocket connection: {}", client_id);
    
    // Create a channel for this client
    let (tx, mut rx) = broadcast::channel::<String>(100);
    
    // Store the connection
    {
        let mut connections = self_.connections.write().await;
        connections.insert(client_id.clone(), tx);
        info!("📊 Total connections: {}", connections.len());
    }
    
    // Create a channel for sending messages back to the client
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<Message>(100);
    
    // Spawn task to forward events to this client
    let client_id_clone = client_id.clone();
    let self_clone = self_.clone();
    let response_tx_clone = response_tx.clone();
    
    tokio::spawn(async move {
        while let Ok(event_json) = rx.recv().await {
            if let Err(e) = response_tx_clone.send(Message::Text(event_json)).await {
                error!("Failed to queue event for client {}: {}", client_id_clone, e);
                break;
            }
        }
        
        // Remove connection when done
        let mut connections = self_clone.connections.write().await;
        connections.remove(&client_id_clone);
        info!("🔌 WebSocket disconnected: {} ({} remaining)", client_id_clone, connections.len());
    });
    
    // Spawn task to send messages to the client
    let mut sender_clone = sender;
    tokio::spawn(async move {
        while let Some(msg) = response_rx.recv().await {
            if let Err(e) = sender_clone.send(msg).await {
                error!("Failed to send to client: {}", e);
                break;
            }
        }
    });
    
    // Handle incoming messages from client
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                info!("📨 Message from {}: {}", client_id, text);
                
                // Handle subscription requests
                if let Ok(subscription) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(method) = subscription.get("method").and_then(|m| m.as_str()) {
                        let response = match method {
                            "subscribe" => {
                                json!({
                                    "status": "Subscribed",
                                    "client_id": client_id,
                                    "message": "Successfully subscribed to real-time events"
                                })
                            }
                            "ping" => {
                                json!({
                                    "status": "pong",
                                    "timestamp": chrono::Utc::now().timestamp()
                                })
                            }
                            _ => {
                                json!({
                                    "status": "error",
                                    "error": format!("Unknown method: {}", method)
                                })
                            }
                        };
                        
                        if let Err(e) = response_tx.send(Message::Text(response.to_string())).await {
                            error!("Failed to queue response: {}", e);
                        }
                    }
                }
            }
            Message::Close(_) => {
                info!("🔌 Client {} requested close", client_id);
                break;
            }
            Message::Ping(data) => {
                if let Err(e) = response_tx.send(Message::Pong(data)).await {
                    error!("Failed to queue pong: {}", e);
                }
            }
            _ => {}
        }
    }
}

// Add this to your routes
pub fn websocket_routes() -> axum::Router<Arc<WebSocketServer>> {
    axum::Router::new()
        .route("/ws", get(WebSocketServer::handle_websocket))
        .route("/ws/status", get(get_websocket_status))
}

async fn get_websocket_status(
    State(server): State<Arc<WebSocketServer>>,
) -> axum::Json<serde_json::Value> {
    let connections = server.connections.read().await;
    
    axum::Json(json!({
        "status": "running",
        "connections": connections.len(),
        "endpoints": {
            "websocket": "/ws",
            "status": "/ws/status"
        },
        "supported_methods": ["subscribe", "ping"],
        "event_types": ["block", "transaction", "account_update", "rolledback_transactions", "reapplied_transactions", "dkg"]
    }))
}
