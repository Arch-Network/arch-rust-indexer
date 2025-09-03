use chrono::{DateTime, Utc, Datelike};
use serde::{Deserialize, Serialize, ser};
use sqlx::types::JsonValue;
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct Block {
    pub height: i64,
    pub hash: String,
    #[serde(serialize_with = "serialize_timestamp_safe")]
    pub timestamp: DateTime<Utc>,
    pub bitcoin_block_height: Option<i64>,
    pub transaction_count: i64,
    pub previous_block_hash: Option<String>,
    pub block_size_bytes: Option<i64>,
}

fn serialize_timestamp_safe<S>(timestamp: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: ser::Serializer,
{
    // Check if timestamp is in valid range (1900-2100)
    if timestamp.year() < 1900 || timestamp.year() > 2100 {
        // Return a fallback timestamp for invalid dates
        let fallback = DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z").unwrap();
        fallback.serialize(serializer)
    } else {
        timestamp.serialize(serializer)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub txid: String,
    pub block_height: i64,
    pub data: JsonValue,
    pub status: Value,
    pub bitcoin_txids: Option<Vec<String>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockWithTransactions {
    pub height: i64,
    pub hash: String,
    pub timestamp: DateTime<Utc>,
    pub bitcoin_block_height: i64,
    pub transaction_count: i64,
    pub previous_block_hash: Option<String>,
    pub block_size_bytes: Option<i64>,
    pub transactions: Option<Vec<Transaction>>,
}
