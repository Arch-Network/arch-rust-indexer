use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use sqlx::PgPool;
use axum::response::IntoResponse;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, NaiveDateTime, Utc};
use hex;
use tracing::{info, debug, error};

use super::types::{ApiError, NetworkStats, SyncStatus, ProgramStats};
use crate::{db::models::{Block, Transaction, BlockWithTransactions}, indexer::BlockProcessor};
use axum::http::StatusCode;

pub async fn get_blocks(
    State(pool): State<Arc<PgPool>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<HashMap<String, serde_json::Value>>, ApiError> {
    let limit = params.get("limit")
        .and_then(|l| l.parse::<i64>().ok())
        .unwrap_or(200); // Default limit

    let offset = params.get("offset")
        .and_then(|o| o.parse::<i64>().ok())
        .unwrap_or(0); // Default offset

    let filter_no_transactions = params.get("filter_no_transactions")
        .map(|v| v == "true")
        .unwrap_or(false);

    // Query to get the paginated blocks
    let blocks = sqlx::query_as!(
        Block,
        r#"
        SELECT 
            b.height,
            b.hash,
            b.timestamp as "timestamp!: DateTime<Utc>",
            b.bitcoin_block_height,
            COUNT(t.txid) as "transaction_count!: i64"
        FROM blocks b 
        LEFT JOIN transactions t ON b.height = t.block_height
        GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height
        HAVING COUNT(t.txid) > 0 OR NOT $3
        ORDER BY b.height DESC 
        LIMIT $1 OFFSET $2
        "#,
        limit,
        offset,
        filter_no_transactions
    )
    .fetch_all(&*pool)
    .await?;

    // Query to get the total count of blocks
    let total_count = if filter_no_transactions {
        sqlx::query_scalar!(
            r#"
            SELECT COUNT(DISTINCT b.height) 
            FROM blocks b
            LEFT JOIN transactions t ON b.height = t.block_height
            GROUP BY b.height
            HAVING COUNT(t.txid) > 0
            "#
        )
        .fetch_one(&*pool)
        .await?
    } else {
        sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) FROM blocks
            "#
        )
        .fetch_one(&*pool)
        .await?
    };

    // Prepare the response
    let mut response = HashMap::new();
    response.insert("total_count".to_string(), serde_json::Value::from(total_count));
    response.insert("blocks".to_string(), serde_json::to_value(blocks)?);

    Ok(Json(response))
}

fn format_time(seconds: f64) -> String {
    let hours = (seconds / 3600.0).floor();
    let minutes = ((seconds % 3600.0) / 60.0).floor();
    let remaining_seconds = (seconds % 60.0).floor();

    if hours > 0.0 {
        format!("{:.0}h {:.0}m {:.0}s", hours, minutes, remaining_seconds)
    } else if minutes > 0.0 {
        format!("{:.0}m {:.0}s", minutes, remaining_seconds)
    } else {
        format!("{:.0}s", remaining_seconds)
    }
}

pub async fn get_sync_status(
    State(processor): State<Arc<BlockProcessor>>,
) -> Result<Json<SyncStatus>, ApiError> {
    // Since `current_block_height` is private, we need to use a public method to access it
    let current_height = processor.get_current_block_height();
    let latest_height = processor.arch_client.get_block_count().await?;
    
    let percentage_complete = if latest_height > 0 {
        ((current_height as f64 / latest_height as f64) * 100.0).round()
    } else {
        0.0
    };

    // Since `average_block_time` is private, we need to use a public method to access it
    let average_block_time = processor.get_average_block_time() as f64;
    
    // Calculate estimated time to completion
    let estimated_time_to_completion = if average_block_time > 0.0 {
        let remaining_blocks = latest_height - current_height;
        let estimated_seconds = (remaining_blocks as f64 * average_block_time) / 1000.0;
        format_time(estimated_seconds)
    } else {
        "N/A".to_string()
    };
    // Calculate elapsed time
    // Since `sync_start_time` is private, we need to use a public method to access it
    let start_time = processor.get_sync_start_time();
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let elapsed_seconds = (current_time - start_time) as f64 / 1000.0;
    
    Ok(Json(SyncStatus {
        current_block_height: current_height,
        latest_block_height: latest_height,
        percentage_complete: format!("{:.2}%", percentage_complete),
        is_synced: current_height >= latest_height,
        estimated_time_to_completion,
        elapsed_time: format_time(elapsed_seconds),
        average_block_time: format!("{:.2} seconds", average_block_time / 1000.0),
    }))
}

#[derive(serde::Serialize)]
struct TransactionRecord {
    txid: String,
    block_height: i64,
    data: serde_json::Value,  // Assuming this is also JSONB
    status: serde_json::Value, // Changed from String to serde_json::Value
    bitcoin_txids: Option<Vec<String>>,
    created_at: NaiveDateTime,
}

pub async fn get_block_by_hash(
    State(pool): State<Arc<PgPool>>,
    Path(blockhash): Path<String>,
) -> Result<Json<BlockWithTransactions>, ApiError> {
    // First get the block information
    let block = sqlx::query!(
        r#"
        SELECT 
            b.height,
            b.hash,
            b.timestamp as "timestamp!: DateTime<Utc>",
            b.bitcoin_block_height,
            COUNT(t.txid) as "transaction_count!: i64"
        FROM blocks b
        LEFT JOIN transactions t ON b.height = t.block_height
        WHERE b.hash = $1
        GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height
        "#,
        blockhash
    )
    .fetch_optional(&*pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    // Then get the transactions for this block
    let transactions = sqlx::query_as!(
        Transaction,
        r#"
        SELECT 
            txid,
            block_height,
            data,
            status,
            bitcoin_txids,
            created_at as "created_at!: NaiveDateTime"
        FROM transactions
        WHERE block_height = $1
        ORDER BY txid
        "#,
        block.height
    )
    .fetch_all(&*pool)
    .await?;

    Ok(Json(BlockWithTransactions {
        height: block.height,
        hash: block.hash,
        timestamp: block.timestamp,
        bitcoin_block_height: block.bitcoin_block_height.unwrap(),
        transaction_count: block.transaction_count,
        transactions: Some(transactions),
    }))
}


pub async fn get_block_by_height(
    State(pool): State<Arc<PgPool>>,
    Path(height): Path<i32>,
) -> Result<Json<Block>, ApiError> {
    let block = sqlx::query_as!(
        Block,
        r#"
        SELECT 
            b.height,
            b.hash,
            b.timestamp as "timestamp!: DateTime<Utc>",
            b.bitcoin_block_height,
            COUNT(t.txid) as "transaction_count!: i64"
        FROM blocks b
        LEFT JOIN transactions t ON b.height = t.block_height
        WHERE b.height = $1
        GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height
        "#,
        height as i64
    )
    .fetch_optional(&*pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(block))
}


pub async fn get_transactions(
    State(pool): State<Arc<PgPool>>,
) -> Result<Json<Vec<Transaction>>, ApiError> {
    let transactions = sqlx::query_as!(
        Transaction,
        r#"
        SELECT 
            txid, 
            block_height, 
            data, 
            status, 
            bitcoin_txids,
            created_at as "created_at!: NaiveDateTime"
        FROM transactions 
        ORDER BY block_height DESC
        LIMIT 100
        "#
    )
    .fetch_all(&*pool)
    .await?;

    Ok(Json(transactions))
}

pub async fn get_transaction(
    State(pool): State<Arc<PgPool>>,
    Path(txid): Path<String>,
) -> impl IntoResponse {
    match sqlx::query_as!(
        Transaction,
        r#"
        SELECT 
            txid, 
            block_height, 
            data, 
            status, 
            bitcoin_txids,
            created_at as "created_at!: NaiveDateTime"
        FROM transactions 
        WHERE txid = $1
        "#,
        txid
    )
    .fetch_optional(&*pool)
    .await {
        Ok(Some(transaction)) => Ok(Json(transaction)),
        Ok(None) => Err(ApiError::NotFound),
        Err(e) => Err(ApiError::Database(e)),
    }
}

fn extract_program_ids(data: &serde_json::Value) -> Vec<String> {
    let mut program_ids = Vec::new();
    
    if let Some(message) = data.get("message") {
        if let Some(instructions) = message.get("instructions") {
            if let Some(instructions_array) = instructions.as_array() {
                for instruction in instructions_array {
                    if let Some(program_id) = instruction.get("program_id") {
                        let program_id_str = match program_id {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Array(arr) => {
                                // Convert byte array to hex string
                                let bytes: Vec<u8> = arr
                                    .iter()
                                    .filter_map(|v| v.as_i64().map(|n| n as u8))
                                    .collect();
                                hex::encode(bytes)
                            }
                            _ => continue,
                        };
                        program_ids.push(program_id_str);
                    }
                }
            }
        }
    }
    
    program_ids.sort();
    program_ids.dedup();
    program_ids
}


pub async fn get_network_stats(
    State(pool): State<Arc<PgPool>>,
) -> Result<Json<NetworkStats>, ApiError> {
    info!("Fetching network stats...");
    
    let stats = match sqlx::query!(
        r#"
        WITH time_windows AS (
            SELECT 
                COUNT(*) as total_tx,
                (SELECT COUNT(*) FROM transactions 
                 WHERE created_at >= NOW() - INTERVAL '24 hours') as daily_tx,
                (SELECT COUNT(*) FROM transactions 
                 WHERE created_at >= NOW() - INTERVAL '1 hour') as hourly_tx,
                (SELECT COUNT(*) FROM transactions 
                 WHERE created_at >= NOW() - INTERVAL '1 minute') as minute_tx,
                (SELECT MAX(height) FROM blocks) as max_height,
                (SELECT COUNT(*) FROM blocks) as total_blocks,
                (SELECT COUNT(*) / 60 as peak_tps FROM transactions 
                 WHERE created_at >= NOW() - INTERVAL '24 hours'
                 GROUP BY DATE_TRUNC('minute', created_at)
                 ORDER BY peak_tps DESC
                 LIMIT 1) as peak_tps
            FROM transactions
        )
        SELECT 
            total_tx,
            daily_tx,
            hourly_tx,
            minute_tx,
            max_height,
            total_blocks,
            COALESCE(peak_tps, 0) as peak_tps
        FROM time_windows
        "#
    )
    .fetch_one(&*pool)
    .await {
        Ok(stats) => {
            info!("Successfully fetched network stats");
            debug!("Raw stats: {:?}", stats);
            stats
        }
        Err(e) => {
            error!("Failed to fetch network stats: {:?}", e);
            return Err(ApiError::Database(e));
        }
    };

    // Calculate different TPS metrics with logging
    let current_tps = stats.minute_tx.unwrap_or(0) as f64 / 60.0;
    let average_tps = stats.hourly_tx.unwrap_or(0) as f64 / 3600.0;
    let peak_tps = stats.peak_tps.unwrap_or(0) as f64;

    debug!("Calculated metrics:");
    debug!("  Current TPS: {}", current_tps);
    debug!("  Average TPS: {}", average_tps);
    debug!("  Peak TPS: {}", peak_tps);

    let response = NetworkStats {
        total_transactions: stats.total_tx.unwrap_or(0),
        total_blocks: stats.total_blocks.unwrap_or(0),
        latest_block_height: stats.max_height.unwrap_or(0),
        block_height: stats.max_height.unwrap_or(0),
        slot_height: stats.max_height.unwrap_or(0),
        current_tps,
        average_tps,
        peak_tps,
        daily_transactions: stats.daily_tx.unwrap_or(0)
    };

    info!("Network stats response prepared successfully");
    debug!("Final response: {:?}", response);

    Ok(Json(response))
}



pub async fn search_handler(
    Query(params): Query<HashMap<String, String>>,
    State(pool): State<Arc<PgPool>>,
) -> impl IntoResponse {
    if let Some(term) = params.get("term") {
        // Check if the term is a transaction ID
        if let Ok(Some(transaction)) = sqlx::query_as!(
            TransactionRecord,
            r#"
            SELECT 
                txid,
                block_height,
                data,
                status,
                bitcoin_txids, 
                created_at as "created_at!: NaiveDateTime"
            FROM transactions 
            WHERE txid = $1
            "#,
            term
        )
        .fetch_optional(&*pool)
        .await
        {
            return Json(json!({ "type": "transaction", "data": transaction }));
        }

        // Check if the term is a block hash
        if let Ok(Some(block)) = sqlx::query_as!(
            Block,
            r#"
            SELECT 
                b.height,
                b.hash,
                b.timestamp as "timestamp!: DateTime<Utc>",
                b.bitcoin_block_height,
                COUNT(t.txid) as "transaction_count!: i64"
            FROM blocks b
            LEFT JOIN transactions t ON b.height = t.block_height
            WHERE b.hash = $1
            GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height
            "#,
            term
        )
        .fetch_optional(&*pool)
        .await
        {
            return Json(json!({ "type": "block", "data": block }));
        }

        // Check if the term is a block height
        if let Ok(height) = term.parse::<i64>() {
            if let Ok(Some(block)) = sqlx::query_as!(
                Block,
                r#"
                SELECT 
                    b.height,
                    b.hash,
                    b.timestamp as "timestamp!: DateTime<Utc>",
                    b.bitcoin_block_height,
                    COUNT(t.txid) as "transaction_count!: i64"
                FROM blocks b
                LEFT JOIN transactions t ON b.height = t.block_height
                WHERE b.height = $1
                GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height
                "#,
                height
            )
            .fetch_optional(&*pool)
            .await
            {
                return Json(json!({ "type": "block", "data": block }));
            }
        }

        // If no match is found
        return Json(json!({ "error": "No matching transaction or block found" }));
    } else {
        // Return an error response if the term is missing
        return Json(json!({ "error": "Missing search term" }));
    }
}

pub async fn get_transactions_by_program(
    State(pool): State<Arc<PgPool>>,
    Path(program_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<HashMap<String, serde_json::Value>>, ApiError> {
    let limit = params.get("limit")
        .and_then(|l| l.parse::<i64>().ok())
        .unwrap_or(100); // Default limit of 100
    
    let offset = params.get("offset")
        .and_then(|o| o.parse::<i64>().ok())
        .unwrap_or(0);

    // Get paginated transactions
    let transactions = sqlx::query_as!(
        Transaction,
        r#"
        SELECT DISTINCT 
            t.txid,
            t.block_height,
            t.data,
            t.status,
            t.bitcoin_txids,
            t.created_at as "created_at!: NaiveDateTime"
        FROM transactions t
        JOIN transaction_programs tp ON t.txid = tp.txid
        WHERE tp.program_id = $1
        ORDER BY t.block_height DESC
        LIMIT $2 OFFSET $3
        "#,
        program_id,
        limit,
        offset
    )
    .fetch_all(&*pool)
    .await?;

    // Get total count of transactions for this program
    let total_count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(DISTINCT t.txid)
        FROM transactions t
        JOIN transaction_programs tp ON t.txid = tp.txid
        WHERE tp.program_id = $1
        "#,
        program_id
    )
    .fetch_one(&*pool)
    .await?
    .unwrap_or(0);

    // Prepare the response
    let mut response = HashMap::new();
    response.insert("total_count".to_string(), serde_json::Value::from(total_count));
    response.insert("transactions".to_string(), serde_json::to_value(transactions)?);

    Ok(Json(response))
}

pub async fn get_program_leaderboard(
    State(pool): State<Arc<PgPool>>,
) -> Result<Json<Vec<ProgramStats>>, ApiError> {
    let programs = sqlx::query_as!(
        ProgramStats,
        r#"
        SELECT 
            program_id,
            transaction_count,
            first_seen_at as "first_seen_at!: DateTime<Utc>",
            last_seen_at as "last_seen_at!: DateTime<Utc>"
        FROM programs
        ORDER BY transaction_count DESC
        LIMIT 10
        "#
    )
    .fetch_all(&*pool)
    .await?;

    Ok(Json(programs))
}

pub async fn get_realtime_status() -> Json<serde_json::Value> {
    // This would get the real-time status from the hybrid sync manager
    Json(json!({
        "realtime_enabled": true,
        "websocket_connected": true,
        "last_block_received": chrono::Utc::now().to_rfc3339(),
        "events_per_second": 4.2,
        "subscriptions": ["block", "transaction", "account_update", "rolledback_transactions", "reapplied_transactions", "dkg"]
    }))
}

pub async fn get_recent_events() -> Json<serde_json::Value> {
    // This would get recent real-time events from the database
    Json(json!({
        "events": [
            {
                "type": "block",
                "hash": "recent_block_hash",
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "height": 12345
            },
            {
                "type": "transaction",
                "hash": "recent_tx_hash",
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "status": "confirmed"
            }
        ],
        "total_events": 2,
        "last_updated": chrono::Utc::now().to_rfc3339()
    }))
}

pub async fn get_websocket_stats() -> Json<serde_json::Value> {
    // This would get WebSocket connection statistics
    Json(json!({
        "connection_status": "connected",
        "endpoint": "ws://44.196.173.35:10081",
        "uptime_seconds": 3600,
        "messages_received": 15000,
        "messages_sent": 6,
        "last_heartbeat": chrono::Utc::now().to_rfc3339(),
        "subscription_topics": ["block", "transaction", "account_update", "rolledback_transactions", "reapplied_transactions", "dkg"]
    }))
}
