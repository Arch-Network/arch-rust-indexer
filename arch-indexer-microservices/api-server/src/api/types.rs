use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Not found")]
    NotFound,
    #[error("Bad request: {0}")]
    BadRequest(String),
    
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "Not found"),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, &*Box::leak(msg.into_boxed_str())),
            ApiError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
            ApiError::Serialization(_) => (StatusCode::BAD_REQUEST, "Serialization error"),
        };

        let body = json!({
            "error": message.to_string(),
        });

        (status, axum::Json(body)).into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkStats {
    pub total_transactions: i64,
    pub total_blocks: i64,
    // Height of the highest indexed block in our DB
    pub indexed_height: i64,
    // Count form for indexed height (assumes genesis at height 0)
    pub indexed_blocks: i64,
    // Network total blocks based on node tip (height + 1)
    pub network_total_blocks: i64,
    pub latest_block_height: i64,
    pub block_height: i64,
    pub slot_height: i64,
    pub current_tps: f64,
    pub average_tps: f64,
    pub peak_tps: f64,
    pub daily_transactions: i64,
    // new optional field: how many blocks behind we are
    #[serde(default)]
    pub missing_blocks: i64,
}

#[derive(Serialize)]
pub struct SyncStatus {
    pub current_block_height: i64,
    pub latest_block_height: i64,
    pub percentage_complete: String,
    pub is_synced: bool,
    pub estimated_time_to_completion: String,
    pub elapsed_time: String,
    pub average_block_time: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProgramStats {
    pub program_id: String,
    pub transaction_count: i64,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenBalance {
    pub mint_address: String,
    pub mint_address_hex: String,
    pub balance: String,
    pub decimals: i32,
    pub owner_address: Option<String>,
    pub program_id: String,
    pub program_name: Option<String>,
    pub supply: Option<String>,
    pub is_frozen: Option<bool>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenMint {
    pub mint_address: String,
    pub mint_address_hex: String,
    pub program_id: String,
    pub decimals: i32,
    pub supply: String,
    pub is_frozen: bool,
    pub mint_authority: Option<String>,
    pub freeze_authority: Option<String>,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
}