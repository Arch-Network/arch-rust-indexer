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
    pub block_height: i64,
    pub slot_height: i64,
    pub current_tps: f64,
    pub average_tps: f64,
    pub peak_tps: f64,
    pub daily_transactions: i64
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

#[derive(Debug, Serialize, Deserialize)]
pub struct ProgramStats {
    pub program_id: String,
    pub transaction_count: i64,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
}