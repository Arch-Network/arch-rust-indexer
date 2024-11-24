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

use super::types::{ApiError, NetworkStats, SyncStatus};
use crate::{db::models::{Block, Transaction}, indexer::BlockProcessor};

pub async fn get_blocks(
    State(pool): State<Arc<PgPool>>,
) -> Result<Json<Vec<Block>>, ApiError> {
    let blocks = sqlx::query_as!(
        Block,
        r#"
        SELECT 
            height,
            hash,
            timestamp as "timestamp!: DateTime<Utc>",
            bitcoin_block_height
        FROM blocks 
        ORDER BY height DESC 
        LIMIT 200
        "#
    )
    .fetch_all(&*pool)
    .await?;

    Ok(Json(blocks))
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
) -> Result<Json<Block>, ApiError> {
    let block = sqlx::query_as!(
        Block,
        r#"
        SELECT 
            b.height,
            b.hash,
            b.timestamp as "timestamp!: DateTime<Utc>",
            b.bitcoin_block_height
        FROM blocks b 
        WHERE b.hash = $1
        "#,
        blockhash
    )
    .fetch_optional(&*pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    // Fetching transactions for the block
    let _transactions = sqlx::query_as!(
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
        "#,
        block.height
    )
    .fetch_all(&*pool)
    .await?;

    // Returning the block with its transactions
    Ok(Json(block))
}


pub async fn get_block_by_height(
    State(pool): State<Arc<PgPool>>,
    Path(height): Path<i32>,
) -> Result<Json<Block>, ApiError> {
    let block = sqlx::query_as!(
        Block,
        r#"
        SELECT 
            height,
            hash,
            timestamp as "timestamp!: DateTime<Utc>",
            bitcoin_block_height
        FROM blocks
        WHERE height = $1
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


pub async fn get_network_stats(
    State(pool): State<Arc<PgPool>>,
) -> Result<Json<NetworkStats>, ApiError> {
    let stats = sqlx::query!(
        r#"
        WITH recent_blocks AS (
            SELECT height, timestamp
            FROM blocks
            WHERE height > (SELECT MAX(height) - 100 FROM blocks)
        ),
        time_range AS (
            SELECT 
                COUNT(*) as block_count,
                MAX(height) as max_height,
                EXTRACT(EPOCH FROM (MAX(timestamp) - MIN(timestamp))) as time_span
            FROM recent_blocks
        ),
        tx_counts AS (
            SELECT COUNT(*) as total_tx
            FROM transactions
        )
        SELECT 
            tr.max_height,
            tr.time_span::float8 as time_span,
            tc.total_tx
        FROM time_range tr, tx_counts tc
        "#
    )
    .fetch_one(&*pool)
    .await?;

    // Calculate transactions per second (TPS)
    let tps = if let Some(time_span) = stats.time_span {
        if time_span > 0.0 {
            stats.total_tx.unwrap_or(0) as f64 / time_span
        } else {
            0.0
        }
    } else {
        0.0
    };

    Ok(Json(NetworkStats {
        total_transactions: stats.total_tx.unwrap_or(0),
        block_height: stats.max_height.unwrap_or(0),
        slot_height: stats.max_height.unwrap_or(0),
        tps,
        true_tps: tps,
    }))
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
                height,
                hash,
                timestamp as "timestamp!: DateTime<Utc>",
                bitcoin_block_height
            FROM blocks 
            WHERE hash = $1
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
                    height,
                    hash,
                    timestamp as "timestamp!: DateTime<Utc>",
                    bitcoin_block_height
                FROM blocks 
                WHERE height = $1
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