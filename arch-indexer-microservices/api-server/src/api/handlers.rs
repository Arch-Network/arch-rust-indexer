use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use sqlx::{PgPool, Row};
use axum::response::IntoResponse;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, NaiveDateTime, Utc};
use hex;
use tracing::{info, debug, error};

use super::types::{ApiError, NetworkStats, SyncStatus, ProgramStats};
use crate::{db::models::{Block, Transaction, BlockWithTransactions}, indexer::BlockProcessor};
use crate::arch_rpc::ArchRpcClient;
use axum::http::StatusCode;

fn shortvec_len(len: usize) -> usize {
    // Solana short_vec length prefix (LEB128-like, 7 bits per byte)
    let mut n = 0usize;
    let mut v = len;
    loop {
        n += 1;
        if v < 0x80 { break; }
        v >>= 7;
    }
    n
}

fn estimate_tx_size(tx: &serde_json::Value) -> i64 {
    // signatures
    let sigs_len = tx.get("signatures")
        .and_then(|a| a.as_array())
        .map(|a| a.len()).unwrap_or(0);
    let sigs_size = shortvec_len(sigs_len) + sigs_len * 64;

    // message
    let message = tx.get("message").unwrap_or(&serde_json::Value::Null);

    // header is 3 bytes
    let header_size = 3usize;

    // account keys
    let keys_len = message.get("account_keys").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0);
    let keys_size = shortvec_len(keys_len) + keys_len * 32;

    // recent_blockhash (32 bytes)
    let rb_size = 32usize;

    // instructions
    let mut instr_size = 0usize;
    if let Some(instrs) = message.get("instructions").and_then(|a| a.as_array()) {
        instr_size += shortvec_len(instrs.len());
        for ins in instrs {
            let accounts_len = ins.get("accounts").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0);
            let data_len = ins.get("data").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0);
            instr_size += 1; // program_id_index
            instr_size += shortvec_len(accounts_len) + accounts_len; // account indices (u8 each)
            instr_size += shortvec_len(data_len) + data_len; // instruction data bytes
        }
    } else {
        instr_size += shortvec_len(0);
    }

    // approximate message size
    let message_size = header_size + keys_size + rb_size + instr_size;
    (sigs_size + message_size) as i64
}

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
    let rows = sqlx::query(
        r#"
        SELECT 
            b.height,
            b.hash,
            b.timestamp,
            b.bitcoin_block_height,
            COUNT(t.txid) as transaction_count,
            b.previous_block_hash
            , NULL::bigint as block_size_bytes
        FROM blocks b 
        LEFT JOIN transactions t ON b.height = t.block_height
        GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height, b.previous_block_hash
        HAVING COUNT(t.txid) > 0 OR NOT $3
        ORDER BY b.height DESC 
        LIMIT $1 OFFSET $2
        "#
    )
    .bind(limit)
    .bind(offset)
    .bind(filter_no_transactions)
    .fetch_all(&*pool)
    .await?;

    let blocks: Vec<Block> = rows
        .into_iter()
        .map(|r| {
            let ts_naive = r.get::<chrono::NaiveDateTime, _>("timestamp");
            let ts_utc = chrono::DateTime::<Utc>::from_naive_utc_and_offset(ts_naive, Utc);
            Block {
                height: r.get::<i64, _>("height"),
                hash: r.get::<String, _>("hash"),
                timestamp: ts_utc,
                bitcoin_block_height: r.try_get::<Option<i64>, _>("bitcoin_block_height").ok().flatten(),
                transaction_count: r.get::<i64, _>("transaction_count"),
                block_size_bytes: r.try_get::<Option<i64>, _>("block_size_bytes").ok().flatten(),
                previous_block_hash: r.try_get::<Option<String>, _>("previous_block_hash").ok().flatten(),
            }
        })
        .collect();

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
    let row = sqlx::query(
        r#"
        SELECT 
            b.height,
            b.hash,
            b.timestamp,
            b.bitcoin_block_height,
            COUNT(t.txid) as transaction_count,
            b.previous_block_hash
            , NULL::bigint as block_size_bytes
        FROM blocks b
        LEFT JOIN transactions t ON b.height = t.block_height
        WHERE b.hash = $1
        GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height, b.previous_block_hash
        "#
    )
    .bind(&blockhash)
    .fetch_optional(&*pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    let ts_naive = row.get::<chrono::NaiveDateTime, _>("timestamp");
    let ts_utc = chrono::DateTime::<Utc>::from_naive_utc_and_offset(ts_naive, Utc);
    let mut block = Block {
        height: row.get::<i64, _>("height"),
        hash: row.get::<String, _>("hash"),
        timestamp: ts_utc,
        bitcoin_block_height: row.try_get::<Option<i64>, _>("bitcoin_block_height").ok().flatten(),
        transaction_count: row.get::<i64, _>("transaction_count"),
        block_size_bytes: row.try_get::<Option<i64>, _>("block_size_bytes").ok().flatten(),
        previous_block_hash: row.try_get::<Option<String>, _>("previous_block_hash").ok().flatten(),
    };

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

    // Compute approximate block size from transactions
    let approx_bytes: i64 = transactions.iter().map(|t| estimate_tx_size(&t.data)).sum();
    if block.block_size_bytes.is_none() { block.block_size_bytes = Some(approx_bytes); }

    // Ensure previous_block_hash is set: DB fallback then RPC
    if block.previous_block_hash.is_none() && block.height > 0 {
        if let Ok(prev_row) = sqlx::query(
            r#"SELECT hash FROM blocks WHERE height = $1"#
        )
        .bind(block.height - 1)
        .fetch_optional(&*pool)
        .await {
            if let Some(r) = prev_row {
                let prev_hash: String = r.get::<String, _>("hash");
                block.previous_block_hash = Some(prev_hash);
            }
        }
        if block.previous_block_hash.is_none() {
            let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
            let arch_client = ArchRpcClient::new(rpc_url);
            if let Ok(rb) = arch_client.get_block(&block.hash, block.height).await {
                if rb.previous_block_hash.is_some() { block.previous_block_hash = rb.previous_block_hash; }
            }
        }
    }

    Ok(Json(BlockWithTransactions {
        height: block.height,
        hash: block.hash,
        timestamp: block.timestamp,
        bitcoin_block_height: block.bitcoin_block_height.unwrap(),
        transaction_count: block.transaction_count,
        previous_block_hash: block.previous_block_hash,
        block_size_bytes: block.block_size_bytes,
        transactions: Some(transactions),
    }))
}


pub async fn get_block_by_height(
    State(pool): State<Arc<PgPool>>,
    Path(height): Path<i32>,
) -> Result<Json<Block>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT 
            b.height,
            b.hash,
            b.timestamp,
            b.bitcoin_block_height,
            COUNT(t.txid) as transaction_count,
            b.previous_block_hash
            , NULL::bigint as block_size_bytes
        FROM blocks b
        LEFT JOIN transactions t ON b.height = t.block_height
        WHERE b.height = $1
        GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height, b.previous_block_hash
        "#
    )
    .bind(height as i64)
    .fetch_optional(&*pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    let ts_naive = row.get::<chrono::NaiveDateTime, _>("timestamp");
    let ts_utc = chrono::DateTime::<Utc>::from_naive_utc_and_offset(ts_naive, Utc);
    let mut block = Block {
        height: row.get::<i64, _>("height"),
        hash: row.get::<String, _>("hash"),
        timestamp: ts_utc,
        bitcoin_block_height: row.try_get::<Option<i64>, _>("bitcoin_block_height").ok().flatten(),
        transaction_count: row.get::<i64, _>("transaction_count"),
        block_size_bytes: row.try_get::<Option<i64>, _>("block_size_bytes").ok().flatten(),
        previous_block_hash: row.try_get::<Option<String>, _>("previous_block_hash").ok().flatten(),
    };

    // Compute approximate size by summing tx sizes for this block
    if block.block_size_bytes.is_none() {
        if let Ok(tx_rows) = sqlx::query(
            r#"
            SELECT data FROM transactions WHERE block_height = $1
            "#
        )
        .bind(block.height)
        .fetch_all(&*pool)
        .await
        {
            let approx: i64 = tx_rows
                .into_iter()
                .map(|r| estimate_tx_size(&r.get::<sqlx::types::JsonValue, _>("data")))
                .sum();
            if approx > 0 {
                block.block_size_bytes = Some(approx);
            }
        }
    }

    // Ensure previous_block_hash is set: DB fallback then RPC
    if block.previous_block_hash.is_none() && block.height > 0 {
        if let Ok(prev_row) = sqlx::query(
            r#"SELECT hash FROM blocks WHERE height = $1"#
        )
        .bind(block.height - 1)
        .fetch_optional(&*pool)
        .await
        {
            if let Some(r) = prev_row {
                let prev_hash: String = r.get::<String, _>("hash");
                block.previous_block_hash = Some(prev_hash);
            }
        }
        if block.previous_block_hash.is_none() {
            let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
            let arch_client = ArchRpcClient::new(rpc_url);
            if let Ok(rb) = arch_client.get_block(&block.hash, block.height).await {
                if rb.previous_block_hash.is_some() {
                    block.previous_block_hash = rb.previous_block_hash;
                }
            }
        }
    }

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

    // Fetch chain head from RPC to report accurate network tip
    let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
    let arch_client = ArchRpcClient::new(rpc_url);
    let node_tip = match arch_client.get_block_count().await {
        Ok(h) => h,
        Err(e) => {
            error!("Failed to fetch block count from RPC: {:?}", e);
            stats.max_height.unwrap_or(0)
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
        latest_block_height: node_tip,
        block_height: node_tip,
        slot_height: node_tip,
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
        if let Ok(Some(r)) = sqlx::query(
            r#"
            SELECT 
                b.height,
                b.hash,
                b.timestamp,
                b.bitcoin_block_height,
                COUNT(t.txid) as transaction_count,
                b.previous_block_hash
                , NULL::bigint as block_size_bytes
            FROM blocks b
            LEFT JOIN transactions t ON b.height = t.block_height
            WHERE b.hash = $1
            GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height, b.previous_block_hash
            "#
        )
        .bind(term)
        .fetch_optional(&*pool)
        .await
        {
            let ts_naive = r.get::<chrono::NaiveDateTime, _>("timestamp");
            let ts_utc = chrono::DateTime::<Utc>::from_naive_utc_and_offset(ts_naive, Utc);
            let mut block = Block {
                    height: r.get::<i64, _>("height"),
                    hash: r.get::<String, _>("hash"),
                    timestamp: ts_utc,
                    bitcoin_block_height: r.try_get::<Option<i64>, _>("bitcoin_block_height").ok().flatten(),
                    transaction_count: r.get::<i64, _>("transaction_count"),
                    block_size_bytes: r.try_get::<Option<i64>, _>("block_size_bytes").ok().flatten(),
                    previous_block_hash: r.try_get::<Option<String>, _>("previous_block_hash").ok().flatten(),
            };

            // Fallback: if previous_block_hash is missing, fetch from RPC
            if block.previous_block_hash.is_none() {
                let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
                let arch_client = crate::arch_rpc::ArchRpcClient::new(rpc_url);
                if let Ok(rb) = arch_client.get_block(&block.hash, block.height).await {
                    block.previous_block_hash = rb.previous_block_hash;
                }
            }

            // Attach transactions list and compute approximate block size
            let tx_rows = sqlx::query(
                r#"
                SELECT 
                    txid,
                    block_height,
                    data,
                    status,
                    bitcoin_txids,
                    created_at
                FROM transactions
                WHERE block_height = $1
                ORDER BY txid
                LIMIT 500
                "#
            )
            .bind(block.height)
            .fetch_all(&*pool)
            .await
            .unwrap_or_default();

            let txs: Vec<Transaction> = tx_rows
                .into_iter()
                .map(|row| Transaction {
                    txid: row.get::<String, _>("txid"),
                    block_height: row.get::<i64, _>("block_height"),
                    data: row.get::<sqlx::types::JsonValue, _>("data"),
                    status: row.get::<serde_json::Value, _>("status"),
                    bitcoin_txids: row.try_get::<Option<Vec<String>>, _>("bitcoin_txids").unwrap_or(None),
                    created_at: row.get::<chrono::NaiveDateTime, _>("created_at"),
                })
                .collect();

            // Compute approximate bytes from serialized tx structure
            let approx_bytes: i64 = txs.iter()
                .map(|t| estimate_tx_size(&t.data))
                .sum();
            if block.block_size_bytes.is_none() {
                block.block_size_bytes = Some(approx_bytes);
            }

            return Json(json!({ "type": "block", "data": {
                "height": block.height,
                "hash": block.hash,
                "timestamp": block.timestamp,
                "bitcoin_block_height": block.bitcoin_block_height,
                "transaction_count": block.transaction_count,
                "block_size_bytes": block.block_size_bytes,
                "previous_block_hash": block.previous_block_hash,
                "transactions": txs,
            }}));
        }

        // Check if the term is a block height
        if let Ok(height) = term.parse::<i64>() {
            if let Ok(Some(row)) = sqlx::query(
                r#"
                SELECT 
                    b.height,
                    b.hash,
                    b.timestamp,
                    b.bitcoin_block_height,
                    COUNT(t.txid) as transaction_count,
                    b.previous_block_hash,
                    NULL::bigint as block_size_bytes
                FROM blocks b
                LEFT JOIN transactions t ON b.height = t.block_height
                WHERE b.height = $1
                GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height, b.previous_block_hash
                "#
            )
            .bind(height)
            .fetch_optional(&*pool)
            .await
            {
                let r = row;
                let ts_naive = r.get::<chrono::NaiveDateTime, _>("timestamp");
                let ts_utc = chrono::DateTime::<Utc>::from_naive_utc_and_offset(ts_naive, Utc);
                let mut block = Block {
                    height: r.get::<i64, _>("height"),
                    hash: r.get::<String, _>("hash"),
                    timestamp: ts_utc,
                    bitcoin_block_height: r.try_get::<Option<i64>, _>("bitcoin_block_height").ok().flatten(),
                    transaction_count: r.get::<i64, _>("transaction_count"),
                    block_size_bytes: r.try_get::<Option<i64>, _>("block_size_bytes").ok().flatten(),
                    previous_block_hash: r.try_get::<Option<String>, _>("previous_block_hash").ok().flatten(),
                };

                // Attach transactions for richer search result UX (no sqlx macros)
                let tx_rows = sqlx::query(
                    r#"
                    SELECT 
                        txid,
                        block_height,
                        data,
                        status,
                        bitcoin_txids,
                        created_at
                    FROM transactions
                    WHERE block_height = $1
                    ORDER BY txid
                    LIMIT 500
                    "#
                )
                .bind(block.height)
                .fetch_all(&*pool)
                .await
                .unwrap_or_default();

                let txs: Vec<Transaction> = tx_rows
                    .into_iter()
                    .map(|row| Transaction {
                        txid: row.get::<String, _>("txid"),
                        block_height: row.get::<i64, _>("block_height"),
                        data: row.get::<sqlx::types::JsonValue, _>("data"),
                        status: row.get::<serde_json::Value, _>("status"),
                        bitcoin_txids: row.try_get::<Option<Vec<String>>, _>("bitcoin_txids").unwrap_or(None),
                        created_at: row.get::<chrono::NaiveDateTime, _>("created_at"),
                    })
                    .collect();

                // Compute approximate bytes from serialized tx structure
                if block.block_size_bytes.is_none() {
                    let approx_bytes: i64 = txs.iter()
                        .map(|t| estimate_tx_size(&t.data))
                        .sum();
                    block.block_size_bytes = Some(approx_bytes);
                }

                // Fallback: if previous_block_hash is missing, fetch from DB then RPC
                if block.previous_block_hash.is_none() && block.height > 0 {
                    if let Ok(prev_row) = sqlx::query(
                        r#"SELECT hash FROM blocks WHERE height = $1"#
                    )
                    .bind(block.height - 1)
                    .fetch_optional(&*pool)
                    .await {
                        if let Some(r) = prev_row {
                            let prev_hash: String = r.get::<String, _>("hash");
                            block.previous_block_hash = Some(prev_hash);
                        }
                    }
                    if block.previous_block_hash.is_none() {
                        let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
                        let arch_client = crate::arch_rpc::ArchRpcClient::new(rpc_url);
                        if let Ok(rb) = arch_client.get_block(&block.hash, block.height).await {
                            if rb.previous_block_hash.is_some() { block.previous_block_hash = rb.previous_block_hash; }
                        }
                    }
                }

                return Json(json!({ "type": "block", "data": {
                    "height": block.height,
                    "hash": block.hash,
                    "timestamp": block.timestamp,
                    "bitcoin_block_height": block.bitcoin_block_height,
                    "transaction_count": block.transaction_count,
                    "block_size_bytes": block.block_size_bytes,
                    "previous_block_hash": block.previous_block_hash,
                    "transactions": txs,
                }}));
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

#[derive(serde::Serialize)]
pub struct MempoolStatsResponse {
    pub total_transactions: i64,
    pub pending_count: i64,
    pub confirmed_count: i64,
    pub avg_fee_priority: Option<f64>,
    pub avg_size_bytes: Option<f64>,
    pub total_size_bytes: Option<i64>,
    pub oldest_transaction: Option<DateTime<Utc>>,
    pub newest_transaction: Option<DateTime<Utc>>,
}

pub async fn get_mempool_stats(
    State(pool): State<Arc<PgPool>>,
) -> Result<Json<MempoolStatsResponse>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT 
            total_transactions,
            pending_count,
            confirmed_count,
            avg_fee_priority,
            avg_size_bytes,
            total_size_bytes,
            oldest_transaction,
            newest_transaction
        FROM mempool_stats
        "#
    )
    .fetch_one(&*pool)
    .await?;

    let total_transactions: i64 = row.try_get::<i64, _>("total_transactions").unwrap_or(0);
    let pending_count: i64 = row.try_get::<i64, _>("pending_count").unwrap_or(0);
    let confirmed_count: i64 = row.try_get::<i64, _>("confirmed_count").unwrap_or(0);
    let avg_fee_priority: Option<f64> = row.try_get::<Option<f64>, _>("avg_fee_priority").unwrap_or(None);
    let avg_size_bytes: Option<f64> = row.try_get::<Option<f64>, _>("avg_size_bytes").unwrap_or(None);
    let total_size_bytes: Option<i64> = row.try_get::<Option<i64>, _>("total_size_bytes").unwrap_or(None);
    let oldest_transaction: Option<DateTime<Utc>> = row.try_get::<Option<DateTime<Utc>>, _>("oldest_transaction").unwrap_or(None);
    let newest_transaction: Option<DateTime<Utc>> = row.try_get::<Option<DateTime<Utc>>, _>("newest_transaction").unwrap_or(None);

    Ok(Json(MempoolStatsResponse {
        total_transactions,
        pending_count,
        confirmed_count,
        avg_fee_priority,
        avg_size_bytes,
        total_size_bytes,
        oldest_transaction,
        newest_transaction,
    }))
}

#[derive(serde::Serialize)]
pub struct MempoolTxBrief {
    pub txid: String,
    pub fee_priority: Option<i32>,
    pub size_bytes: Option<i32>,
    pub added_at: DateTime<Utc>,
}

pub async fn get_recent_mempool_transactions(
    State(pool): State<Arc<PgPool>>,
) -> Result<Json<Vec<MempoolTxBrief>>, ApiError> {
    let records = sqlx::query(
        r#"
        SELECT txid,
               fee_priority,
               size_bytes,
               added_at
        FROM mempool_transactions
        ORDER BY added_at DESC
        LIMIT 50
        "#
    )
    .fetch_all(&*pool)
    .await?;

    let items = records
        .into_iter()
        .map(|r| MempoolTxBrief {
            txid: r.get::<String, _>("txid"),
            fee_priority: r.try_get::<Option<i32>, _>("fee_priority").unwrap_or(None),
            size_bytes: r.try_get::<Option<i32>, _>("size_bytes").unwrap_or(None),
            added_at: r.get::<DateTime<Utc>, _>("added_at"),
        })
        .collect();

    Ok(Json(items))
}

#[derive(serde::Serialize)]
pub struct TransactionMetricsResponse {
    pub txid: String,
    pub compute_units_consumed: Option<i32>,
    pub fee_priority: Option<i32>,
    pub size_bytes: Option<i32>,
    pub in_mempool: bool,
    pub created_at: Option<NaiveDateTime>,
}

pub async fn get_transaction_metrics(
    State(pool): State<Arc<PgPool>>,
    Path(txid): Path<String>,
) -> Result<Json<TransactionMetricsResponse>, ApiError> {
    // Fetch compute units and created_at from confirmed transactions table (if present)
    let base = sqlx::query(
        r#"
        SELECT 
            compute_units_consumed,
            created_at
        FROM transactions
        WHERE txid = $1
        "#
    )
    .bind(&txid)
    .fetch_optional(&*pool)
    .await?;

    // Fetch fee priority and size from mempool (if present)
    let mem = sqlx::query(
        r#"
        SELECT 
            fee_priority,
            size_bytes
        FROM mempool_transactions
        WHERE txid = $1
        "#
    )
    .bind(&txid)
    .fetch_optional(&*pool)
    .await?;

    let response = TransactionMetricsResponse {
        txid: txid.clone(),
        compute_units_consumed: base.as_ref().and_then(|r| r.try_get::<Option<i32>, _>("compute_units_consumed").ok()).flatten(),
        fee_priority: mem.as_ref().and_then(|m| m.try_get::<Option<i32>, _>("fee_priority").ok()).flatten(),
        size_bytes: mem.as_ref().and_then(|m| m.try_get::<Option<i32>, _>("size_bytes").ok()).flatten(),
        in_mempool: mem.is_some(),
        created_at: base.and_then(|r| r.try_get::<Option<NaiveDateTime>, _>("created_at").ok()).flatten(),
    };

    Ok(Json(response))
}

pub async fn get_program_details(
    State(pool): State<Arc<PgPool>>,
    Path(program_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Basic program stats (runtime query to avoid sqlx prepare at build time)
    let program = sqlx::query(
        r#"
        SELECT 
            program_id,
            transaction_count,
            first_seen_at,
            last_seen_at
        FROM programs
        WHERE program_id = $1
        "#
    )
    .bind(&program_id)
    .fetch_optional(&*pool)
    .await?;

    if let Some(p) = program {
        let pid: String = p.get::<String, _>("program_id");
        let tx_count: i64 = p.get::<i64, _>("transaction_count");
        let first_seen: DateTime<Utc> = p.get::<DateTime<Utc>, _>("first_seen_at");
        let last_seen: DateTime<Utc> = p.get::<DateTime<Utc>, _>("last_seen_at");

        // Recent transactions for this program
        let recent_rows = sqlx::query(
            r#"
            SELECT DISTINCT t.txid, t.block_height, t.created_at
            FROM transactions t
            JOIN transaction_programs tp ON t.txid = tp.txid
            WHERE tp.program_id = $1
            ORDER BY t.block_height DESC
            LIMIT 25
            "#
        )
        .bind(&pid)
        .fetch_all(&*pool)
        .await?;

        let recent: Vec<serde_json::Value> = recent_rows
            .into_iter()
            .map(|r| {
                json!({
                    "txid": r.get::<String, _>("txid"),
                    "block_height": r.get::<i64, _>("block_height"),
                    "created_at": r.get::<chrono::NaiveDateTime, _>("created_at"),
                })
            })
            .collect();

        let payload = json!({
            "program": {
                "program_id": pid,
                "transaction_count": tx_count,
                "first_seen_at": first_seen,
                "last_seen_at": last_seen
            },
            "recent_transactions": recent
        });

        Ok(Json(payload))
    } else {
        Err(ApiError::NotFound)
    }
}
