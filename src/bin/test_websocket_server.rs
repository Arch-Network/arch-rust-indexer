use anyhow::Result;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use arch_indexer::arch_rpc::websocket::WebSocketEvent;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("🚀 Starting WebSocket Server Test...");

    // Create a broadcast channel for events
    let (event_tx, _event_rx) = broadcast::channel::<WebSocketEvent>(100);
    let event_tx = Arc::new(event_tx);

    // Create the router
    let app = Router::new()
        .route("/ws", get(handle_websocket))
        .route("/ws/status", get(get_status))
        .with_state(event_tx.clone());

    // Start the server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    info!("🌐 WebSocket server listening on ws://127.0.0.1:3000/ws");

    // Spawn a task to send test events
    let event_tx_clone = event_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
        
        loop {
            interval.tick().await;
            
            let test_event = WebSocketEvent {
                topic: "block".to_string(),
                data: json!({
                    "hash": format!("test_block_{}", chrono::Utc::now().timestamp()),
                    "timestamp": chrono::Utc::now().timestamp()
                }),
                timestamp: chrono::Utc::now(),
            };
            
            if let Err(e) = event_tx_clone.send(test_event) {
                error!("Failed to send test event: {}", e);
            } else {
                info!("📤 Sent test event");
            }
        }
    });

    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_websocket(
    ws: WebSocketUpgrade,
    axum::extract::State(event_tx): axum::extract::State<Arc<broadcast::Sender<WebSocketEvent>>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, event_tx))
}

async fn handle_socket(socket: WebSocket, event_tx: Arc<broadcast::Sender<WebSocketEvent>>) {
    let (mut sender, mut receiver) = socket.split();
    
    let client_id = format!("client_{}", uuid::Uuid::new_v4());
    info!("🔌 New WebSocket connection: {}", client_id);
    
    // Subscribe to events
    let mut event_rx = event_tx.subscribe();
    
    // Create a channel for sending messages back to the client
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<Message>(100);
    
    // Spawn task to forward events to this client
    let client_id_clone = client_id.clone();
    let response_tx_clone = response_tx.clone();
    
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            let event_json = serde_json::to_string(&event).unwrap();
            if let Err(e) = response_tx_clone.send(Message::Text(event_json)).await {
                error!("Failed to queue event for client {}: {}", client_id_clone, e);
                break;
            }
        }
        info!("🔌 WebSocket disconnected: {}", client_id_clone);
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

async fn get_status() -> axum::Json<serde_json::Value> {
    axum::Json(json!({
        "status": "running",
        "endpoints": {
            "websocket": "/ws",
            "status": "/ws/status"
        },
        "supported_methods": ["subscribe", "ping"],
        "event_types": ["block", "transaction", "account_update", "rolledback_transactions", "reapplied_transactions", "dkg"]
    }))
}
