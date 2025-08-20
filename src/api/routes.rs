use axum::{
    extract::State,
    routing::get,
    Router,
};
use std::sync::Arc;

use sqlx::PgPool;
use crate::api::handlers;

pub fn create_router(pool: Arc<PgPool>) -> Router {
    Router::new()
        .route("/api/blocks", get(handlers::get_blocks))
        .route("/api/blocks/height/:height", get(handlers::get_block_by_height))
        .route("/api/blocks/:blockhash", get(handlers::get_block_by_hash))
        .route("/api/transactions", get(handlers::get_transactions))
        .route("/api/transactions/:txid", get(handlers::get_transaction))
        .route("/api/search", get(handlers::search_handler))
        .route("/api/network-stats", get(handlers::get_network_stats))
        .route("/api/programs/leaderboard", get(handlers::get_program_leaderboard))
        .route("/api/programs/:program_id/transactions", get(handlers::get_transactions_by_program))
        .route("/api/realtime/status", get(handlers::get_realtime_status))
        .route("/api/realtime/events", get(handlers::get_recent_events))
        .route("/api/websocket/stats", get(handlers::get_websocket_stats))
        .with_state(pool)
}
