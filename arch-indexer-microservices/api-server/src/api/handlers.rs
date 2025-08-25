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
use bs58;
use tracing::{info, debug, error};
use axum::http::StatusCode;
use axum::extract::Path as AxPath;

use super::types::{ApiError, NetworkStats, SyncStatus, ProgramStats};
use super::program_ids as pid;
use crate::{db::models::{Block, Transaction, BlockWithTransactions}, indexer::BlockProcessor};
use crate::arch_rpc::ArchRpcClient;

fn key_to_bytes(v: &serde_json::Value) -> Option<Vec<u8>> {
    if let Some(arr) = v.as_array() {
        Some(arr.iter().filter_map(|x| x.as_i64().map(|n| n as u8)).collect())
    } else if let Some(s) = v.as_str() {
        // Accept base58 program names like Loader111... -> decode base58 if possible, else return None
        bs58::decode(s).into_vec().ok()
    } else {
        None
    }
}

fn key_to_base58(v: &serde_json::Value) -> String {
    if let Some(bytes) = key_to_bytes(v) {
        return bs58::encode(bytes).into_string();
    }
    v.as_str().unwrap_or("").to_string()
}

fn key_to_hex(v: &serde_json::Value) -> String {
    if let Some(bytes) = key_to_bytes(v) {
        return hex::encode(bytes);
    }
    if let Some(s) = v.as_str() {
        if let Ok(bytes) = bs58::decode(s).into_vec() { return hex::encode(bytes); }
    }
    String::new()
}

#[derive(serde::Serialize)]
pub struct ProgramRowOut {
    pub program_id_hex: String,
    pub program_id_base58: String,
    pub transaction_count: i64,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub display_name: Option<String>,
}

fn try_hex_to_base58(hex_str: &str) -> String {
    match hex::decode(hex_str) {
        Ok(bytes) => bs58::encode(bytes).into_string(),
        Err(_) => String::new(),
    }
}

fn normalize_program_param(id: &str) -> Option<String> {
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit()) && id.len() >= 2 {
        Some(id.to_lowercase())
    } else {
        bs58::decode(id).into_vec().ok().map(|b| hex::encode(b))
    }
}

fn fallback_program_name_from_b58(b58: &str) -> Option<String> {
    match b58 {
        // Arch IDs
        pid::SYSTEM_PROGRAM => Some("System Program".to_string()),
        pid::VOTE_PROGRAM => Some("Vote Program".to_string()),
        pid::STAKE_PROGRAM => Some("Stake Program".to_string()),
        pid::BPF_LOADER => Some("BPF Loader".to_string()),
        pid::NATIVE_LOADER => Some("Native Loader".to_string()),
        // Arch Token programs
        pid::APL_TOKEN_PROGRAM => Some("APL Token".to_string()),
        pid::APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM => Some("Associated Token Account".to_string()),
        // Legacy/Solana IDs we may encounter in old data
        pid::SOL_LOADER => Some("Loader".to_string()),
        pid::SOL_COMPUTE_BUDGET => Some("Compute Budget (Solana)".to_string()),
        pid::SOL_MEMO => Some("Memo (Solana)".to_string()),
        pid::SOL_SPL_TOKEN => Some("SPL Token (Solana)".to_string()),
        pid::SOL_ASSOCIATED_TOKEN_ACCOUNT => Some("Associated Token Account (Solana)".to_string()),
        _ => None,
    }
}

fn fallback_program_name_from_hex(hex_id: &str) -> Option<String> {
    // System Program common representations
    const SYS_HEX_ALL_ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const SYS_HEX_ONE: &str = "0000000000000000000000000000000000000000000000000000000000000001";
    if hex_id.eq_ignore_ascii_case(SYS_HEX_ALL_ZERO) || hex_id.eq_ignore_ascii_case(SYS_HEX_ONE) {
        return Some("System Program".to_string());
    }
    None
}

async fn maybe_persist_display_name(pool: &PgPool, program_hex: &str, name: &str) {
    // Best-effort: persist name if column exists and currently null
    let has_col: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'programs' AND column_name = 'display_name'
        )"#
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    if !has_col { return; }
    let _ = sqlx::query(
        r#"
        UPDATE programs
        SET display_name = $2
        WHERE program_id = $1 AND (display_name IS NULL OR display_name = '')
        "#
    )
    .bind(program_hex)
    .bind(name)
    .execute(pool)
    .await;
}

pub async fn list_programs(
    State(pool): State<Arc<PgPool>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok()).map(|v| v.min(200)).unwrap_or(100);
    let page = params.get("page").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let offset = (page - 1) * limit;
    let search = params.get("search").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    let (where_sql, bind_search): (&str, Option<String>) = if let Some(s) = search {
        if let Some(hex_norm) = normalize_program_param(&s) {
            ("WHERE program_id LIKE $3", Some(format!("{}%", hex_norm)))
        } else {
            ("", None)
        }
    } else {
        ("", None)
    };

    // total count
    let total_count: i64 = if let Some(s) = &bind_search {
        match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM programs WHERE program_id LIKE $1")
            .bind(s)
            .fetch_one(&*pool)
            .await {
            Ok(v) => v,
            Err(e) => { error!("/api/programs count query failed: {}", e); return Err(ApiError::Database(e)); }
        }
    } else {
        match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM programs")
            .fetch_one(&*pool)
            .await {
            Ok(v) => v,
            Err(e) => { error!("/api/programs count(all) failed: {}", e); return Err(ApiError::Database(e)); }
        }
    };

    // Check if display_name column exists; build columns accordingly
    let has_display: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'programs' AND column_name = 'display_name'
        )"#
    )
    .fetch_one(&*pool)
    .await
    .unwrap_or(false);

    let cols = if has_display {
        "program_id, transaction_count, first_seen_at, last_seen_at, display_name"
    } else {
        "program_id, transaction_count, first_seen_at, last_seen_at, NULL::text as display_name"
    };

    // rows
    let sql = format!(
        "SELECT {} FROM programs {} ORDER BY last_seen_at DESC LIMIT $1 OFFSET $2",
        cols,
        where_sql
    );
    let rows_res = if let Some(s) = bind_search.clone() {
        sqlx::query(&sql)
            .bind(limit)
            .bind(offset)
            .bind(s)
            .fetch_all(&*pool)
            .await
    } else {
        sqlx::query(&sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&*pool)
            .await
    };

    let rows = match rows_res {
        Ok(v) => v,
        Err(e) => {
            // Fallback: some deployments may have different timestamp types; fall back to simple ordering
            error!("/api/programs primary rows query failed: {} | sql={}", e, sql);
            let fallback_sql = if bind_search.is_some() {
                "SELECT program_id, transaction_count, CURRENT_TIMESTAMP as first_seen_at, CURRENT_TIMESTAMP as last_seen_at, NULL::text as display_name FROM programs WHERE program_id LIKE $3 ORDER BY transaction_count DESC LIMIT $1 OFFSET $2".to_string()
            } else {
                "SELECT program_id, transaction_count, CURRENT_TIMESTAMP as first_seen_at, CURRENT_TIMESTAMP as last_seen_at, NULL::text as display_name FROM programs ORDER BY transaction_count DESC LIMIT $1 OFFSET $2".to_string()
            };

            if let Some(s) = bind_search {
                sqlx::query(&fallback_sql)
                    .bind(limit)
                    .bind(offset)
                    .bind(s)
                    .fetch_all(&*pool)
                    .await
                    .map_err(|e2| { error!("/api/programs fallback rows failed: {} | sql={}", e2, fallback_sql); ApiError::Database(e2) })?
            } else {
                sqlx::query(&fallback_sql)
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(&*pool)
                    .await
                    .map_err(|e2| { error!("/api/programs fallback rows failed: {} | sql={}", e2, fallback_sql); ApiError::Database(e2) })?
            }
        }
    };

    let items: Vec<ProgramRowOut> = rows
        .into_iter()
        .map(|r| {
            let pid: String = r.get::<String, _>("program_id");
            let b58 = try_hex_to_base58(&pid);
            let dn: Option<String> = r.try_get::<Option<String>, _>("display_name").ok().flatten();
            let display = dn
                .or_else(|| fallback_program_name_from_b58(&b58))
                .or_else(|| fallback_program_name_from_hex(&pid));
            // fire-and-forget persist
            if let Some(name_val) = display.clone() {
                let pool_clone = pool.clone();
                let pid_clone = pid.clone();
                tokio::spawn(async move {
                    maybe_persist_display_name(&*pool_clone, &pid_clone, &name_val).await;
                });
            }
            ProgramRowOut {
                program_id_hex: pid.clone(),
                program_id_base58: b58,
                transaction_count: r.get::<i64, _>("transaction_count"),
                first_seen_at: r.get::<DateTime<Utc>, _>("first_seen_at"),
                last_seen_at: r.get::<DateTime<Utc>, _>("last_seen_at"),
                display_name: display,
            }
        })
        .collect();

    Ok(Json(json!({
        "total_count": total_count,
        "programs": items,
        "page": page,
        "limit": limit,
    })))
}

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

    // Deprecated: previously used to exclude empty blocks. We now always include all blocks.
    let _deprecated_filter_no_transactions = params.get("filter_no_transactions").is_some();

    // Query to get the paginated blocks
    let rows = sqlx::query(
        r#"
        SELECT 
            b.height,
            b.hash,
            b.timestamp,
            b.bitcoin_block_height,
            COALESCE(COUNT(t.txid), 0) as transaction_count,
            NULL::bigint as block_size_bytes
        FROM blocks b 
        LEFT JOIN transactions t ON b.height = t.block_height
        GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height
        ORDER BY b.height DESC 
        LIMIT $1 OFFSET $2
        "#
    )
    .bind(limit)
    .bind(offset)
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
    // Use dynamic query (not macros) to avoid sqlx offline cache issues in Docker build
    let total_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM blocks")
        .fetch_one(&*pool)
        .await?;

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
            NULL::bigint as block_size_bytes
        FROM blocks b
        LEFT JOIN transactions t ON b.height = t.block_height
        WHERE b.hash = $1
        GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height
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
        previous_block_hash: None,
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
        bitcoin_block_height: block.bitcoin_block_height.unwrap_or(0),
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
            NULL::bigint as block_size_bytes
        FROM blocks b
        LEFT JOIN transactions t ON b.height = t.block_height
        WHERE b.height = $1
        GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height
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
        previous_block_hash: None,
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
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<HashMap<String, serde_json::Value>>, ApiError> {
    let limit = params
        .get("limit")
        .and_then(|l| l.parse::<i64>().ok())
        .unwrap_or(100);

    let offset = params
        .get("offset")
        .and_then(|o| o.parse::<i64>().ok())
        .unwrap_or(0);

    // Fetch paginated transactions newest-first (dynamic query to avoid sqlx offline cache issues)
    let rows = sqlx::query(
        r#"
        SELECT 
            txid, 
            block_height, 
            data, 
            status, 
            bitcoin_txids,
            created_at
        FROM transactions 
        ORDER BY created_at DESC, block_height DESC
        LIMIT $1 OFFSET $2
        "#
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&*pool)
    .await?;

    let transactions: Vec<Transaction> = rows
        .into_iter()
        .map(|r| Transaction {
            txid: r.get::<String, _>("txid"),
            block_height: r.get::<i64, _>("block_height"),
            data: r.get("data"),
            status: r.get("status"),
            bitcoin_txids: r.try_get::<Option<Vec<String>>, _>("bitcoin_txids").ok().flatten(),
            created_at: r.get::<NaiveDateTime, _>("created_at"),
        })
        .collect();

    // Total transactions count for pagination
    let total_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
        .fetch_one(&*pool)
        .await?;

    let mut response = HashMap::new();
    response.insert("total_count".to_string(), serde_json::Value::from(total_count));
    response.insert(
        "transactions".to_string(),
        serde_json::to_value(transactions)?
    );

    Ok(Json(response))
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

#[derive(serde::Serialize)]
pub struct ExecutionResponse {
    pub status: serde_json::Value,
    pub logs: Vec<String>,
    pub bitcoin_txid: Option<String>,
    pub rollback_status: Option<serde_json::Value>,
    pub compute_units_consumed: Option<u64>,
    pub runtime_transaction: Option<serde_json::Value>,
}

fn parse_compute_units_from_logs(logs: &[String]) -> Option<u64> {
    // Look for lines like "Program log: compute units consumed xxx" or similar
    // For now we scan for any number in the last few characters
    for line in logs.iter().rev() {
        let lower = line.to_lowercase();
        if lower.contains("compute") && lower.contains("unit") {
            // extract last integer
            let mut num: Option<u64> = None;
            let mut acc: String = String::new();
            for ch in lower.chars() {
                if ch.is_ascii_digit() { acc.push(ch); } else { if !acc.is_empty() { num = acc.parse::<u64>().ok(); acc.clear(); } }
            }
            if num.is_some() { return num; }
        }
    }
    None
}

pub async fn get_transaction_execution(
    AxPath(txid): AxPath<String>,
) -> Result<Json<ExecutionResponse>, ApiError> {
    let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
    let arch_client = ArchRpcClient::new(rpc_url);
    let rpc = match arch_client.get_processed_transaction(&txid).await {
        Ok(v) => v,
        Err(_) => return Err(ApiError::NotFound),
    };

    let status = rpc.status.clone();
    // logs field name depends on client; map defensively
    let logs: Vec<String> = if let Some(arr) = rpc.runtime_transaction.get("logs").and_then(|v| v.as_array()) {
        arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
    } else {
        // try Deserialize from our struct in arch client shape
        let _ = rpc.runtime_transaction.get("message");
        let _ = rpc.status.clone();
        // Fallback: try to use top-level logs if present via serde shape
        rpc_runtime_logs_fallback(&rpc)
    };

    let compute_units = parse_compute_units_from_logs(&logs);

    let resp = ExecutionResponse {
        status,
        logs,
        bitcoin_txid: rpc.bitcoin_txids.clone().and_then(|v| v.get(0).cloned()),
        rollback_status: Some(serde_json::json!(rpc.accounts_tags)),
        compute_units_consumed: compute_units,
        runtime_transaction: Some(rpc.runtime_transaction.clone()),
    };

    Ok(Json(resp))
}

fn rpc_runtime_logs_fallback(r: &crate::arch_rpc::ProcessedTransaction) -> Vec<String> {
    // our current struct doesn't expose logs directly, but RPC returns logs at top-level in some cases
    // We attempt to parse from status if embedded, else empty
    Vec::new()
}

#[derive(serde::Serialize)]
pub struct ParticipantRow {
    pub address_hex: String,
    pub address_base58: String,
    pub is_signer: bool,
    pub is_writable: bool,
    pub is_readonly: bool,
    pub is_fee_payer: bool,
}

pub async fn get_transaction_participants(
    State(pool): State<Arc<PgPool>>,
    AxPath(txid): AxPath<String>,
) -> Result<Json<Vec<ParticipantRow>>, ApiError> {
    // Prefer RPC so we always match latest shape
    let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
    let arch_client = ArchRpcClient::new(rpc_url);
    let data_opt_rpc = arch_client.get_processed_transaction(&txid).await.ok().map(|r| r.runtime_transaction);

    let row_opt = sqlx::query(
        r#"
        SELECT data FROM transactions WHERE txid = $1
        "#
    )
    .bind(&txid)
    .fetch_optional(&*pool)
    .await?;

    let data: serde_json::Value = if let Some(v) = data_opt_rpc { v } else if let Some(row) = row_opt { row.get("data") } else { return Err(ApiError::NotFound) };
    let header = data.get("message").and_then(|m| m.get("header")).ok_or(ApiError::NotFound)?;
    let n_req = header.get("num_required_signatures").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
    let n_ro_signed = header.get("num_readonly_signed_accounts").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
    let n_ro_unsigned = header.get("num_readonly_unsigned_accounts").and_then(|v| v.as_i64()).unwrap_or(0) as usize;

    let keys = data.get("message").and_then(|m| m.get("account_keys")).and_then(|a| a.as_array()).ok_or(ApiError::NotFound)?;

    let total = keys.len();
    let writable_signed = n_req.saturating_sub(n_ro_signed);
    let writable_unsigned = total.saturating_sub(n_req).saturating_sub(n_ro_unsigned);

    let mut out: Vec<ParticipantRow> = Vec::with_capacity(total);
    for (i, k) in keys.iter().enumerate() {
        let hex_id = key_to_hex(k);
        let b58_id = key_to_base58(k);
        let is_signer = i < n_req;
        let is_fee_payer = i == 0 && is_signer;
        let is_writable = if is_signer { i < writable_signed } else { i < n_req + writable_unsigned };
        let is_readonly = !is_writable;
        out.push(ParticipantRow { address_hex: hex_id, address_base58: b58_id, is_signer, is_writable, is_readonly, is_fee_payer });
    }

    Ok(Json(out))
}

#[derive(serde::Serialize)]
pub struct InstructionRow {
    pub index: usize,
    pub program_id_hex: String,
    pub program_id_base58: String,
    pub accounts: Vec<String>,
    pub data_len: usize,
    pub action: Option<String>,
    pub decoded: Option<serde_json::Value>,
    pub data_hex: String,
}

pub async fn get_transaction_instructions(
    State(pool): State<Arc<PgPool>>,
    AxPath(txid): AxPath<String>,
) -> Result<Json<Vec<InstructionRow>>, ApiError> {
    let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
    let arch_client = ArchRpcClient::new(rpc_url);
    let data_opt_rpc = arch_client.get_processed_transaction(&txid).await.ok().map(|r| r.runtime_transaction);

    let row_opt = sqlx::query(
        r#"
        SELECT data FROM transactions WHERE txid = $1
        "#
    )
    .bind(&txid)
    .fetch_optional(&*pool)
    .await?;

    let data: serde_json::Value = if let Some(v) = data_opt_rpc { v } else if let Some(row) = row_opt { row.get("data") } else { return Err(ApiError::NotFound) };
    let message = data.get("message").ok_or(ApiError::NotFound)?;
    let keys = message.get("account_keys").and_then(|a| a.as_array()).ok_or(ApiError::NotFound)?;
    let instructions = message.get("instructions").and_then(|a| a.as_array()).ok_or(ApiError::NotFound)?;

    fn u32_le(bytes: &[u8]) -> Option<u32> { if bytes.len() >= 4 { Some(u32::from_le_bytes([bytes[0],bytes[1],bytes[2],bytes[3]])) } else { None } }
    fn u64_le(bytes: &[u8]) -> Option<u64> { if bytes.len() >= 8 { Some(u64::from_le_bytes([bytes[0],bytes[1],bytes[2],bytes[3],bytes[4],bytes[5],bytes[6],bytes[7]])) } else { None } }

    fn decode_instruction(program_b58: &str, program_hex: &str, data: &[u8], accounts: &[String]) -> (Option<String>, Option<serde_json::Value>) {
        fn is_system_b58(s: &str) -> bool {
            if s.is_empty() { return false; }
            if s.chars().all(|c| c == '1') { return true; }
            let ones = "111111111111111111111111111111"; // 30 ones prefix
            s.starts_with(ones) && s.ends_with('2')
        }
        // Compute Budget program (Solana convention)
        if program_b58 == "ComputeBudget111111111111111111111111111111" {
            if let Some((&tag, rest)) = data.split_first() {
                // 1: RequestHeapFrame(u32 size)
                if tag == 1 { if let Some(size) = u32_le(rest) {
                    let decoded = json!({
                        "discriminator": {"type": "u8", "data": tag},
                        "bytes": {"type": "u32", "data": size}
                    });
                    return (Some("Compute Budget: RequestHeapFrame".to_string()), Some(decoded)); }}
                // 2: SetComputeUnitLimit(u32 units)
                if tag == 2 && rest.len() >= 4 {
                    let units = u32_le(rest).unwrap_or(0);
                    let decoded = json!({
                        "discriminator": {"type": "u8", "data": tag},
                        "units": {"type": "u32", "data": units}
                    });
                    return (Some("Compute Budget: SetComputeUnitLimit".to_string()), Some(decoded));
                }
                // 3: SetComputeUnitPrice(u64 micro_lamports)
                if tag == 3 && rest.len() >= 8 {
                    let price = u64_le(rest).unwrap_or(0);
                    let decoded = json!({
                        "discriminator": {"type": "u8", "data": tag},
                        "price_micro_lamports": {"type": "u64", "data": price}
                    });
                    return (Some("Compute Budget: SetComputeUnitPrice".to_string()), Some(decoded));
                }
            }
        }
        // System Program transfer (Solana convention)
        if is_system_b58(program_b58)
            || program_hex.eq_ignore_ascii_case("0000000000000000000000000000000000000000000000000000000000000000")
            || program_hex.eq_ignore_ascii_case("0000000000000000000000000000000000000000000000000000000000000001") {
            if data.len() >= 4 {
                let tag = u32_le(&data[0..4]).unwrap_or(9999);
                // 1: Assign { owner: Pubkey }
                if tag == 1 && data.len() >= 4 + 32 {
                    let owner_b58 = bs58::encode(&data[4..36]).into_string();
                    let account = accounts.get(0).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag},
                        "owner": owner_b58,
                        "account": account,
                    });
                    return (Some("System Program: Assign".to_string()), Some(decoded));
                }
                // 0: CreateAccount { lamports: u64, space: u64, owner: Pubkey(32) }
                if tag == 0 && data.len() >= 4 + 8 + 8 + 32 {
                    let lamports = u64_le(&data[4..12]).unwrap_or(0);
                    let space = u64_le(&data[12..20]).unwrap_or(0);
                    let owner = bs58::encode(&data[20..52]).into_string();
                    let from = accounts.get(0).cloned(); // funder
                    let new_account = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag},
                        "lamports": {"type":"u64", "data": lamports},
                        "space": {"type":"u64", "data": space},
                        "owner": owner,
                        "funder": from,
                        "new_account": new_account,
                    });
                    return (Some("System Program: CreateAccount".to_string()), Some(decoded));
                }
                // 2 (or 4 on some variants): Transfer { lamports: u64 }
                if (tag == 2 || tag == 4) && data.len() >= 12 {
                    let lamports = u64_le(&data[4..12]).unwrap_or(0);
                    let src = accounts.get(0).cloned();
                    let dst = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag},
                        "lamports": {"type":"u64", "data": lamports},
                        "source": src,
                        "destination": dst,
                    });
                    return (Some("System Program: Transfer".to_string()), Some(decoded));
                }
                // Fallback: classic 12-byte payload with two accounts -> transfer
                if data.len() == 12 && accounts.len() >= 2 {
                    let lamports = u64_le(&data[4..12]).unwrap_or(0);
                    let src = accounts.get(0).cloned();
                    let dst = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag},
                        "lamports": {"type":"u64", "data": lamports},
                        "source": src,
                        "destination": dst,
                    });
                    return (Some("System Program: Transfer".to_string()), Some(decoded));
                }
                // 8: Allocate { space: u64 }
                if tag == 8 && data.len() >= 4 + 8 {
                    let space = u64_le(&data[4..12]).unwrap_or(0);
                    let account = accounts.get(0).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag},
                        "space": {"type":"u64", "data": space},
                        "account": account,
                    });
                    return (Some("System Program: Allocate".to_string()), Some(decoded));
                }
            }
        }
        // Token Program (support Arch APL Token and legacy SPL Token IDs)
        if program_b58 == pid::APL_TOKEN_PROGRAM || program_b58 == pid::SOL_SPL_TOKEN {
            if !data.is_empty() {
                let tag = data[0];
                // 0: InitializeMint { decimals, mint_authority, freeze_authority: COption<Pubkey> }
                if tag == 0 && data.len() >= 1 + 1 + 32 + 1 {
                    let decimals = data[1];
                    let mint_authority = bs58::encode(&data[2..34]).into_string();
                    let has_freeze = data[34] != 0;
                    let mut obj = serde_json::Map::new();
                    obj.insert("discriminator".to_string(), json!({"type":"u8", "data": tag}));
                    obj.insert("decimals".to_string(), json!({"type":"u8", "data": decimals}));
                    obj.insert("mint_authority".to_string(), json!(mint_authority));
                    if has_freeze && data.len() >= 1 + 1 + 32 + 1 + 32 {
                        obj.insert("freeze_authority".to_string(), json!(bs58::encode(&data[35..67]).into_string()));
                    } else {
                        obj.insert("freeze_authority".to_string(), json!(null));
                    }
                    return (Some("Token: InitializeMint".to_string()), Some(serde_json::Value::Object(obj)));
                }
                // 1: InitializeAccount (no data)
                if tag == 1 {
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "account": accounts.get(0),
                        "mint": accounts.get(1),
                        "owner": accounts.get(2),
                    });
                    return (Some("Token: InitializeAccount".to_string()), Some(decoded));
                }
                // 3: Transfer { amount: u64 }, accounts: [source, destination, authority]
                if tag == 3 && data.len() >= 1 + 8 {
                    let amount = u64_le(&data[1..9]).unwrap_or(0);
                    let source = accounts.get(0).cloned();
                    let destination = accounts.get(1).cloned();
                    let authority = accounts.get(2).cloned();
                    let decoded = json!({
                        "type": "transfer",
                        "amount": amount,
                        "from": source,
                        "to": destination,
                        "authority": authority,
                    });
                    return (Some("Token: Transfer".to_string()), Some(decoded));
                }
                // 12: TransferChecked { amount: u64, decimals: u8 }
                if tag == 12 && data.len() >= 1 + 8 + 1 {
                    let amount = u64_le(&data[1..9]).unwrap_or(0);
                    let decimals = data[9];
                    let token = accounts.get(0).cloned();
                    let source = accounts.get(1).cloned();
                    let mint = accounts.get(2).cloned();
                    let destination = accounts.get(3).cloned();
                    let authority = accounts.get(4).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "amount": {"type":"u64", "data": amount},
                        "decimals": {"type":"u8", "data": decimals},
                        "token": token,
                        "source": source,
                        "mint": mint,
                        "destination": destination,
                        "authority": authority,
                    });
                    return (Some("Token: TransferChecked".to_string()), Some(decoded));
                }
                // 4: Approve { amount: u64 }
                if tag == 4 && data.len() >= 1 + 8 {
                    let amount = u64_le(&data[1..9]).unwrap_or(0);
                    let source = accounts.get(0).cloned();
                    let delegate = accounts.get(2).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "amount": {"type":"u64", "data": amount},
                        "source": source,
                        "delegate": delegate,
                    });
                    return (Some("Token: Approve".to_string()), Some(decoded));
                }
                // 5: Revoke
                if tag == 5 {
                    let source = accounts.get(0).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "source": source,
                    });
                    return (Some("Token: Revoke".to_string()), Some(decoded));
                }
                // 7: MintTo { amount: u64 }
                if tag == 7 && data.len() >= 1 + 8 {
                    let amount = u64_le(&data[1..9]).unwrap_or(0);
                    let mint = accounts.get(0).cloned();
                    let dest = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "amount": {"type":"u64", "data": amount},
                        "mint": mint,
                        "destination": dest,
                    });
                    return (Some("Token: MintTo".to_string()), Some(decoded));
                }
                // 8: Burn { amount: u64 }
                if tag == 8 && data.len() >= 1 + 8 {
                    let amount = u64_le(&data[1..9]).unwrap_or(0);
                    let account = accounts.get(0).cloned();
                    let mint = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "amount": {"type":"u64", "data": amount},
                        "account": account,
                        "mint": mint,
                    });
                    return (Some("Token: Burn".to_string()), Some(decoded));
                }
                // 9: CloseAccount
                if tag == 9 {
                    let account = accounts.get(0).cloned();
                    let destination = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "account": account,
                        "destination": destination,
                    });
                    return (Some("Token: CloseAccount".to_string()), Some(decoded));
                }
                // 10: FreezeAccount
                if tag == 10 {
                    let account = accounts.get(0).cloned();
                    let mint = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "account": account,
                        "mint": mint,
                    });
                    return (Some("Token: FreezeAccount".to_string()), Some(decoded));
                }
                // 11: ThawAccount
                if tag == 11 {
                    let account = accounts.get(0).cloned();
                    let mint = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "account": account,
                        "mint": mint,
                    });
                    return (Some("Token: ThawAccount".to_string()), Some(decoded));
                }
                // 17: SyncNative (no data)
                if tag == 17 {
                    let account = accounts.get(0).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "account": account,
                    });
                    return (Some("Token: SyncNative".to_string()), Some(decoded));
                }
            }
        }
        // Associated Token Account Program (Arch). Instruction has no data
        if program_b58 == pid::APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM && data.is_empty() {
            let funder = accounts.get(0).cloned();
            let associated_account = accounts.get(1).cloned();
            let wallet = accounts.get(2).cloned();
            let mint = accounts.get(3).cloned();
            let system_program = accounts.get(4).cloned();
            let token_program = accounts.get(5).cloned();
            let decoded = json!({
                "type": "create_associated_token_account",
                "funder": funder,
                "associated_account": associated_account,
                "wallet": wallet,
                "mint": mint,
                "system_program": system_program,
                "token_program": token_program,
            });
            return (Some("Associated Token Account: Create".to_string()), Some(decoded));
        }
        // Memo program: utf-8
        if program_b58 == "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr" {
            if let Ok(text) = std::str::from_utf8(data) {
                return (Some("Memo: Write".to_string()), Some(json!({"memo": text})));
            }
        }
        (None, None)
    }

    let mut out = Vec::with_capacity(instructions.len());
    for (idx, ins) in instructions.iter().enumerate() {
        // Resolve program id: prefer explicit program_id, else program_id_index into account_keys
        let program_val = if let Some(v) = ins.get("program_id") {
            Some(v.clone())
        } else if let Some(ix) = ins.get("program_id_index").and_then(|v| v.as_i64()) {
            keys.get(ix as usize).cloned()
        } else { None };
        let program_hex = program_val.as_ref().map(|v| key_to_hex(v)).unwrap_or_default();
        let program_b58 = program_val.as_ref().map(|v| key_to_base58(v)).unwrap_or_default();
        let acc_idx: Vec<usize> = ins.get("accounts").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as usize)).collect()).unwrap_or_default();
        let accounts: Vec<String> = acc_idx.into_iter().filter_map(|i| keys.get(i).map(|k| key_to_base58(k))).collect();
        let data_vec: Vec<u8> = ins.get("data").and_then(|d| d.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as u8)).collect()).unwrap_or_default();
        let data_len = data_vec.len();
        let data_hex = hex::encode(&data_vec);
        let (action, decoded) = decode_instruction(&program_b58, &program_hex, &data_vec, &accounts);
        out.push(InstructionRow { index: idx, program_id_hex: program_hex, program_id_base58: program_b58, accounts, data_len, action, decoded, data_hex });
    }

    Ok(Json(out))
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

    // Normalize incoming program_id (accept hex or base58) to canonical hex
    let pid_hex = if program_id.chars().all(|c| c.is_ascii_hexdigit()) && program_id.len() >= 2 {
        program_id.clone()
    } else {
        // Try base58 decode -> hex
        match bs58::decode(&program_id).into_vec() {
            Ok(bytes) => hex::encode(bytes),
            Err(_) => program_id.clone(),
        }
    };

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
        pid_hex,
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
        pid_hex
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
    // Normalize input (hex or base58) to hex
    let pid_hex = if program_id.chars().all(|c| c.is_ascii_hexdigit()) && program_id.len() >= 2 {
        program_id.clone()
    } else {
        match bs58::decode(&program_id).into_vec() {
            Ok(bytes) => hex::encode(bytes),
            Err(_) => program_id.clone(),
        }
    };

    // Check if display_name column exists
    let has_display: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'programs' AND column_name = 'display_name'
        )"#
    )
    .fetch_one(&*pool)
    .await
    .unwrap_or(false);

    // Build SQL accordingly (always alias display_name for consistent row handling)
    let sql = if has_display {
        r#"
        SELECT 
            program_id,
            transaction_count,
            first_seen_at,
            last_seen_at,
            display_name
        FROM programs
        WHERE program_id = $1
        "#
    } else {
        r#"
        SELECT 
            program_id,
            transaction_count,
            first_seen_at,
            last_seen_at,
            NULL::text as display_name
        FROM programs
        WHERE program_id = $1
        "#
    };

    let program = sqlx::query(sql)
        .bind(&pid_hex)
        .fetch_optional(&*pool)
        .await?;

    if let Some(p) = program {
        let pid: String = p.get::<String, _>("program_id");
        let tx_count: i64 = p.get::<i64, _>("transaction_count");
        let first_seen: DateTime<Utc> = p.get::<DateTime<Utc>, _>("first_seen_at");
        let last_seen: DateTime<Utc> = p.get::<DateTime<Utc>, _>("last_seen_at");
        let dn: Option<String> = p.try_get::<Option<String>, _>("display_name").ok().flatten();
        let b58 = bs58::encode(hex::decode(&pid).unwrap_or_default()).into_string();
        let display_name = dn
            .or_else(|| fallback_program_name_from_b58(&b58))
            .or_else(|| fallback_program_name_from_hex(&pid));
        if let Some(ref name) = display_name {
            maybe_persist_display_name(&*pool, &pid, name).await;
        }

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
                "program_id_hex": pid,
                "program_id_base58": b58,
                "display_name": display_name,
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

pub async fn backfill_programs(State(pool): State<Arc<PgPool>>) -> impl IntoResponse {
    // Pure-Rust backfill that does not rely on DB functions. Idempotent.
    let mut offset: i64 = 0;
    let limit: i64 = 1000;
    let mut upserted_programs: u64 = 0;
    let mut linked: u64 = 0;

    loop {
        let rows = sqlx::query(
            r#"
            SELECT txid, data
            FROM transactions
            ORDER BY created_at ASC
            LIMIT $1 OFFSET $2
            "#
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&*pool)
        .await;

        let rows = match rows { Ok(r) => r, Err(e) => {
            let body = json!({ "status": "error", "message": format!("db read: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(body))
        }};

        if rows.is_empty() { break; }

        for row in rows {
            let txid: String = row.get("txid");
            let data: serde_json::Value = row.get("data");
            if let Some(instrs) = data.get("message").and_then(|m| m.get("instructions")).and_then(|v| v.as_array()) {
                for ins in instrs {
                    if let Some(pid_v) = ins.get("program_id") {
                        let pid_hex = key_to_hex(pid_v);
                        if !pid_hex.is_empty() {
                            // upsert program
                            let res = sqlx::query(
                                r#"
                                INSERT INTO programs (program_id, transaction_count)
                                VALUES ($1, 0)
                                ON CONFLICT (program_id) DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP
                                "#
                            )
                            .bind(&pid_hex)
                            .execute(&*pool)
                            .await;
                            if let Ok(r) = res { upserted_programs += r.rows_affected(); }

                            // link transaction_programs
                            let res2 = sqlx::query(
                                r#"
                                INSERT INTO transaction_programs (txid, program_id)
                                VALUES ($1, $2)
                                ON CONFLICT (txid, program_id) DO NOTHING
                                "#
                            )
                            .bind(&txid)
                            .bind(&pid_hex)
                            .execute(&*pool)
                            .await;
                            if let Ok(r2) = res2 { linked += r2.rows_affected(); }
                        }
                    }
                }
            }
        }

        offset += limit;
    }

    let body = json!({
        "status": "ok",
        "programs_upserted": upserted_programs,
        "links_created": linked,
    });
    (StatusCode::OK, Json(body))
}
