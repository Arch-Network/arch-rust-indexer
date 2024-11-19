use axum::{
    routing::get,
    Router,
};
use serde_json::json;
use axum::Json;
use sqlx::PgPool;
use std::sync::Arc;
use crate::indexer::BlockProcessor;

use super::handlers;

pub fn create_router(pool: Arc<PgPool>, processor: Arc<BlockProcessor>) -> Router {
    Router::new()
    .route("/", get(|| async { 
        Json(json!({
            "message": "Arch Indexer API is running"
        }))
    }))
        .route("/api/blocks", get(handlers::get_blocks))
        .route("/api/blocks/:blockhash", get(handlers::get_block_by_hash))
        .route("/api/blocks/height/:height", get(handlers::get_block_by_height))
        .route("/api/transactions", get(handlers::get_transactions))
        .route("/api/transactions/:txid", get(handlers::get_transaction))
        .route("/api/network-stats", get(handlers::get_network_stats))
        .with_state(pool)
        .nest("/api", Router::new()
            .route("/sync-status", get(handlers::get_sync_status))
            .with_state(processor)
        )
}