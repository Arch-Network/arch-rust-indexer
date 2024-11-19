use axum::{
    response::{IntoResponse, Response},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Not found")]
    NotFound,
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "Resource not found"),
            ApiError::Database(ref e) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
            ApiError::Internal(ref e) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
        };

        let body = Json(ErrorResponse {
            error: message.to_string(),
        });

        (status, body).into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
pub struct NetworkStats {
    pub total_transactions: i64,
    pub block_height: i64,
    pub slot_height: i64,
    pub tps: f64,
    pub true_tps: f64,
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