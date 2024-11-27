use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use serde_json::Value;
#[derive(Debug, Serialize, Deserialize)]
pub struct Block {
    pub height: i64,
    pub hash: String,
    pub timestamp: DateTime<Utc>,
    pub bitcoin_block_height: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub txid: String,
    pub block_height: i64,
    pub data: JsonValue,
    pub status: Value,
    pub bitcoin_txids: Option<Vec<String>>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockWithTransactions {
    pub height: i64,
    pub hash: String,
    pub timestamp: DateTime<Utc>,
    pub bitcoin_block_height: Option<i64>,
    pub transaction_count: i64,
}