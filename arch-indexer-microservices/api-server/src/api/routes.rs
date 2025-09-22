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
        .route("/health", get(handlers::health_check))
        .route("/api/blocks", get(handlers::get_blocks))
        .route("/api/blocks/gaps", get(handlers::get_block_gaps))
        .route("/api/blocks/missing", get(handlers::get_missing_block_heights))
        .route("/api/blocks/backfill-missing", get(handlers::backfill_missing_blocks))
        .route("/api/blocks/backfill-range", get(handlers::backfill_block_range))
        .route("/api/blocks/height/:height", get(handlers::get_block_by_height))
        .route("/api/blocks/:blockhash", get(handlers::get_block_by_hash))
        .route("/api/transactions", get(handlers::get_transactions))
        .route("/api/transactions/:txid", get(handlers::get_transaction))
        .route("/api/transactions/:txid/execution", get(handlers::get_transaction_execution))
        .route("/api/transactions/:txid/participants", get(handlers::get_transaction_participants))
        .route("/api/transactions/:txid/instructions", get(handlers::get_transaction_instructions))
        .route("/api/search", get(handlers::search_handler))
        .route("/api/network/stats", get(handlers::get_network_stats))
        .route("/api/programs", get(handlers::list_programs))
        .route("/api/programs/leaderboard", get(handlers::get_program_leaderboard))
        .route("/api/programs/:program_id", get(handlers::get_program_details))
        .route("/api/programs/:program_id/transactions", get(handlers::get_transactions_by_program))
        .route("/api/programs/backfill", get(handlers::backfill_programs))
        .route("/api/tokens/leaderboard", get(handlers::get_token_leaderboard))
        // Accounts
        .route("/api/accounts/:address", get(handlers::get_account_summary))
        .route("/api/accounts/:address/transactions", get(handlers::get_account_transactions))
        .route("/api/accounts/:address/transactions/v2", get(handlers::get_account_transactions_v2))
        .route("/api/accounts/:address/programs", get(handlers::get_account_programs))
        .route("/api/accounts/:address/token-balances", get(handlers::get_account_token_balances))
        .route("/api/realtime/status", get(handlers::get_realtime_status))
        .route("/api/realtime/events", get(handlers::get_recent_events))
        .route("/api/websocket/stats", get(handlers::get_websocket_stats))
        .route("/api/mempool/stats", get(handlers::get_mempool_stats))
        .route("/api/mempool/recent", get(handlers::get_recent_mempool_transactions))
        .route("/api/transactions/:txid/metrics", get(handlers::get_transaction_metrics))
        .with_state(pool)
}
