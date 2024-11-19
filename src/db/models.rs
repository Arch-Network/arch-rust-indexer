use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;

#[derive(Debug, Serialize, Deserialize)]
pub struct Block {
    pub height: i64,
    pub hash: String,
    pub timestamp: chrono::NaiveDateTime,
    pub bitcoin_block_height: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub txid: String,
    pub block_height: i64,
    pub data: JsonValue,
    pub status: i32,
    pub bitcoin_txids: Vec<String>,
    pub created_at: chrono::NaiveDateTime,
}