use axum::{
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::collections::HashMap;

pub fn create_static_router() -> Router {
    Router::new()
        .route("/static/css/main.css", get(serve_main_css))
        .route("/static/js/main.js", get(serve_main_js))
        .route("/static/js/websocket.js", get(serve_websocket_js))
}

async fn serve_main_css() -> impl IntoResponse {
    let css = r#"
        /* Additional CSS can be added here */
        .custom-styles {
            /* Custom styles for future enhancements */
        }
    "#;
    
    Response::builder()
        .header("Content-Type", "text/css")
        .body(css.to_string())
        .unwrap()
}

async fn serve_main_js() -> impl IntoResponse {
    let js = r#"
        // Additional JavaScript functionality can be added here
        console.log('Main JS loaded');
    "#;
    
    Response::builder()
        .header("Content-Type", "application/javascript")
        .body(js.to_string())
        .unwrap()
}

async fn serve_websocket_js() -> impl IntoResponse {
    let js = r#"
        // WebSocket utility functions
        class WebSocketManager {
            constructor(url) {
                this.url = url;
                this.ws = null;
                this.reconnectAttempts = 0;
                this.maxReconnectAttempts = 5;
            }
            
            connect() {
                this.ws = new WebSocket(this.url);
                this.setupEventHandlers();
            }
            
            setupEventHandlers() {
                this.ws.onopen = () => {
                    console.log('WebSocket connected');
                    this.reconnectAttempts = 0;
                };
                
                this.ws.onclose = () => {
                    console.log('WebSocket disconnected');
                    this.attemptReconnect();
                };
                
                this.ws.onerror = (error) => {
                    console.error('WebSocket error:', error);
                };
            }
            
            attemptReconnect() {
                if (this.reconnectAttempts < this.maxReconnectAttempts) {
                    this.reconnectAttempts++;
                    setTimeout(() => {
                        console.log(`Attempting to reconnect... (${this.reconnectAttempts}/${this.maxReconnectAttempts})`);
                        this.connect();
                    }, 5000 * this.reconnectAttempts);
                }
            }
            
            send(message) {
                if (this.ws && this.ws.readyState === WebSocket.OPEN) {
                    this.ws.send(JSON.stringify(message));
                }
            }
            
            close() {
                if (this.ws) {
                    this.ws.close();
                }
            }
        }
        
        // Export for use in other scripts
        window.WebSocketManager = WebSocketManager;
    "#;
    
    Response::builder()
        .header("Content-Type", "application/javascript")
        .body(js.to_string())
        .unwrap()
}
