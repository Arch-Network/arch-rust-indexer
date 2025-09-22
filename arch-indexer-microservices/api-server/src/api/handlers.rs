use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use sqlx::{PgPool, Row};
use axum::response::IntoResponse;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Utc};
use hex;
use bs58;
use tracing::{info, debug, error};
use axum::http::StatusCode;
use axum::extract::Path as AxPath;

use super::types::{ApiError, NetworkStats, SyncStatus, ProgramStats};
use super::program_ids as pid;
use crate::{db::models::{Block, Transaction, BlockWithTransactions}, indexer::BlockProcessor};
use crate::arch_rpc::ArchRpcClient;
use std::collections::HashSet;
use axum::http::StatusCode as AxStatusCode;

fn key_to_bytes(v: &serde_json::Value) -> Option<Vec<u8>> {
    if let Some(arr) = v.as_array() {
        return Some(arr.iter().filter_map(|x| x.as_i64().map(|n| n as u8)).collect());
    }
    if let Some(s) = v.as_str() {
        // Try base58 first
        if let Ok(bytes) = bs58::decode(s).into_vec() {
            return Some(bytes);
        }
        // Then try hex if it looks like hex
        let maybe_hex = s.len() >= 2 && s.len() % 2 == 0 && s.chars().all(|c| c.is_ascii_hexdigit());
        if maybe_hex {
            if let Ok(bytes) = hex::decode(s) {
                return Some(bytes);
            }
        }
    }
    None
}

fn key_to_base58(v: &serde_json::Value) -> String {
    if let Some(bytes) = key_to_bytes(v) {
        return bs58::encode(bytes).into_string();
    }
    // If it's a hex string, attempt to decode and re-encode as base58
    if let Some(s) = v.as_str() {
        let maybe_hex = s.len() >= 2 && s.len() % 2 == 0 && s.chars().all(|c| c.is_ascii_hexdigit());
        if maybe_hex {
            if let Ok(bytes) = hex::decode(s) {
                return bs58::encode(bytes).into_string();
            }
        }
        return s.to_string();
    }
    String::new()
}

fn key_to_hex(v: &serde_json::Value) -> String {
    if let Some(bytes) = key_to_bytes(v) {
        return hex::encode(bytes);
    }
    if let Some(s) = v.as_str() {
        // If already hex-looking, return normalized lowercase
        if s.len() >= 2 && s.len() % 2 == 0 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            return s.to_lowercase();
        }
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
    if let Ok(bytes) = hex::decode(hex_str) {
        // First, check if this hex represents one of our known program string constants
        if let Ok(s) = std::str::from_utf8(&bytes) {
            // Check if this is one of our known program string constants
            match s {
                "VoteProgram111111111111111111111" |
                "StakeProgram11111111111111111111" |
                "BpfLoader11111111111111111111111" |
                "NativeLoader11111111111111111111" |
                "ComputeBudget111111111111111111111111111111" |
                "AplToken111111111111111111111111" |
                "AssociatedTokenAccount1111111111" => {
                    return s.to_string(); // Always return the friendly string constant
                }
                _ => {
                    // For other string constants, check if they look like base58 labels
                    let is_b58_label = !s.is_empty() && s.chars().all(|c| {
                        matches!(c,
                            '1'|'2'|'3'|'4'|'5'|'6'|'7'|'8'|'9'|
                            'A'..='H'|'J'..='N'|'P'..='Z'|
                            'a'..='k'|'m'..='z'
                        )
                    });
                    if is_b58_label { 
                        return s.to_string();
                    }
                }
            }
        }
        
        // If not a known program or string constant, convert to base58
        // This will handle cases like 01de36762ac00d066bfc0a96641499bb850aebfde3b2f400 -> 5QVc8gaXMdjnfS8JS1K8NbQQVPhVHfVPY2asS8b1xY8g
        return bs58::encode(bytes).into_string();
    }
    String::new()
}

fn normalize_program_param(id: &str) -> Option<String> {
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit()) && id.len() >= 2 {
        Some(id.to_lowercase())
    } else {
        bs58::decode(id).into_vec().ok().map(|b| hex::encode(b))
    }
}

fn fallback_program_name_from_b58(b58: &str) -> Option<String> {
    // First try our new comprehensive mapping
    if let Some(name) = crate::api::program_ids::get_program_name(b58) {
        return Some(name.to_string());
    }
    
    // Check if the base58 field contains a string constant that we can map
    match b58 {
        // Arch canonical program IDs (string constants)
        "VoteProgram111111111111111111111" => Some("Vote Program".to_string()),
        "StakeProgram11111111111111111111" => Some("Stake Program".to_string()),
        "BpfLoader11111111111111111111111" => Some("BPF Loader".to_string()),
        "NativeLoader11111111111111111111" => Some("Native Loader".to_string()),
        "ComputeBudget111111111111111111111111111111" => Some("Compute Budget Program".to_string()),
        "AplToken111111111111111111111111" => Some("APL Token Program".to_string()),
        "AssociatedTokenAccount1111111111" => Some("APL Associated Token Account Program".to_string()),
        
        // Accept legacy/base58 forms for friendly label
        "7ZMyUmgbNckx7G5BCrdmX2XUasjDAk5uhcMpDbUDxHQ3" => Some("APL Token".to_string()),
        "7ZQepXoDxPadjZnu618jicRj89uLzM3ucvyEEt72G1fm" => Some("Associated Token Account".to_string()),
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

    // Only map hex IDs that decode to our known string constants
    if let Ok(bytes) = hex::decode(hex_id) {
        if let Ok(label) = std::str::from_utf8(&bytes) {
            // Only map exact matches to our known program string constants
            match label {
                "VoteProgram111111111111111111111" => return Some("Vote Program".to_string()),
                "StakeProgram11111111111111111111" => return Some("Stake Program".to_string()),
                "BpfLoader11111111111111111111111" => return Some("BPF Loader".to_string()),
                "NativeLoader11111111111111111111" => return Some("Native Loader".to_string()),
                "ComputeBudget111111111111111111111111111111" => return Some("Compute Budget Program".to_string()),
                "AplToken111111111111111111111111" => return Some("APL Token Program".to_string()),
                "AssociatedTokenAccount1111111111" => return Some("APL Associated Token Account Program".to_string()),
                _ => {}
            }
        }
        
        // For other hex IDs, try the base58 mapping as a fallback
        let b58 = bs58::encode(&bytes).into_string();
        if let Some(name) = fallback_program_name_from_b58(&b58) {
            return Some(name);
        }
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

#[derive(serde::Serialize)]
pub struct AccountSummary {
    pub address: String,
    pub address_hex: String,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub transaction_count: i64,
    pub lamports_balance: Option<i128>,
}

pub async fn get_account_summary(
    State(pool): State<Arc<PgPool>>,
    AxPath(address): AxPath<String>,
) -> Result<Json<AccountSummary>, ApiError> {
    let address_hex = normalize_program_param(&address).ok_or(ApiError::BadRequest("Invalid address".into()))?;
    // Prefer account_participation if present; otherwise fall back to scanning transactions JSON
    let has_participation: bool = sqlx::query_scalar(
        r#"SELECT to_regclass('public.account_participation') IS NOT NULL"#
    )
    .fetch_one(&*pool)
    .await
    .unwrap_or(false);

    let row = if has_participation {
        sqlx::query(
            r#"
            SELECT MIN(created_at) AS first_seen, MAX(created_at) AS last_seen, COUNT(*) AS tx_count
            FROM account_participation ap
            WHERE ap.address_hex ILIKE $1
            "#)
            .bind(address_hex.clone())
            .fetch_optional(&*pool)
            .await?
    } else {
        sqlx::query(
            r#"
            WITH accs AS (
                SELECT t.txid, t.created_at,
                    CASE 
                        WHEN jsonb_typeof(acc.value) = 'string' THEN normalize_program_id(trim(both '"' from (acc.value)::text))
                        ELSE normalize_program_id((acc.value)::text)
                    END AS acc_hex
                FROM transactions t
                CROSS JOIN LATERAL jsonb_array_elements(t.data->'message'->'account_keys') AS acc(value)
            )
            SELECT MIN(created_at) AS first_seen,
                   MAX(created_at) AS last_seen,
                   COUNT(*) AS tx_count
            FROM accs
            WHERE acc_hex ILIKE $1
            "#)
            .bind(address_hex.clone())
            .fetch_optional(&*pool)
            .await
            .map_err(|e| { error!("get_account_summary fallback query error: {:?}", e); ApiError::Database(e) })?
    };

    let (first_seen, last_seen, tx_count) = if let Some(row) = row {
        (
            row.try_get::<Option<DateTime<Utc>>, _>("first_seen").ok().flatten(),
            row.try_get::<Option<DateTime<Utc>>, _>("last_seen").ok().flatten(),
            row.try_get::<i64, _>("tx_count").unwrap_or(0),
        )
    } else { (None, None, 0) };

    // Opportunistically compute lamports balance by scanning system transfers involving this account
    let lamports_balance = compute_account_lamports_balance(&*pool, &address, &address_hex).await.ok();

    Ok(Json(AccountSummary {
        address,
        address_hex: address_hex.clone(),
        first_seen,
        last_seen,
        transaction_count: tx_count,
        lamports_balance: {
            // Prefer persisted native balance if present; read as text and parse to i128
            let nb_text: Option<String> = sqlx::query_scalar("SELECT balance::text FROM native_balances WHERE address_hex ILIKE $1")
                .bind(&address_hex)
                .fetch_optional(&*pool)
                .await
                .unwrap_or(None);
            let nb_parsed: Option<i128> = nb_text.and_then(|s| s.parse::<i128>().ok());
            nb_parsed.or(lamports_balance).or(Some(0))
        },
    }))
}

async fn compute_account_lamports_balance(pool: &PgPool, address_b58: &str, address_hex: &str) -> Result<i128, ApiError> {
    // Prefer account_participation if available; it's robust to encoding differences
    let has_participation: bool = sqlx::query_scalar(
        r#"SELECT to_regclass('public.account_participation') IS NOT NULL"#
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    let use_participation: bool = if has_participation {
        sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM account_participation WHERE address_hex ILIKE $1)"#
        )
        .bind(address_hex)
        .fetch_one(pool)
        .await
        .unwrap_or(false)
    } else { false };

    let rows = if use_participation {
        sqlx::query(
            r#"
            SELECT t.data
            FROM account_participation ap
            JOIN transactions t ON t.txid = ap.txid
            WHERE ap.address_hex ILIKE $1
            ORDER BY t.created_at ASC
            LIMIT 10000
            "#
        )
        .bind(address_hex)
        .fetch_all(pool)
        .await
        .map_err(ApiError::Database)?
    } else {
        // Fallback: scan account_keys for this address (handles hex and jsonb arrays)
        sqlx::query(
            r#"
            WITH accs AS (
                SELECT t.txid, t.created_at, t.data
                FROM transactions t
                CROSS JOIN LATERAL jsonb_array_elements(COALESCE(t.data#>'{message,account_keys}', t.data#>'{message,keys}', '[]'::jsonb)) AS acc(value)
                WHERE (
                    CASE 
                        WHEN jsonb_typeof(acc.value) = 'string' THEN normalize_program_id(trim(both '"' from (acc.value)::text))
                        WHEN jsonb_typeof(acc.value) = 'array' THEN normalize_program_id(acc.value)
                        WHEN jsonb_typeof(acc.value) = 'object' THEN normalize_program_id(acc.value->'pubkey')
                        ELSE NULL
                    END ILIKE $1
                    OR (
                        jsonb_typeof(acc.value) = 'string' AND (acc.value #>> '{}') = $2
                    )
                )
            )
            SELECT data FROM accs
            ORDER BY created_at ASC
            LIMIT 10000
            "#
        )
        .bind(address_hex)
        .bind(address_b58)
        .fetch_all(pool)
        .await
        .map_err(ApiError::Database)?
    };

    fn u32_le(bytes: &[u8]) -> Option<u32> { if bytes.len() >= 4 { Some(u32::from_le_bytes([bytes[0],bytes[1],bytes[2],bytes[3]])) } else { None } }
    fn u64_le(bytes: &[u8]) -> Option<u64> { if bytes.len() >= 8 { Some(u64::from_le_bytes([bytes[0],bytes[1],bytes[2],bytes[3],bytes[4],bytes[5],bytes[6],bytes[7]])) } else { None } }

    fn is_system_b58(s: &str) -> bool {
        if s.is_empty() { return false; }
        if s.chars().all(|c| c == '1') { return true; }
        let ones = "111111111111111111111111111111"; // 30 ones prefix
        s.starts_with(ones) && s.ends_with('2')
    }

    let mut balance_delta: i128 = 0;
    for row in rows {
        let data: serde_json::Value = row.get("data");
        let message = if let Some(m) = data.get("message") { m } else { continue };
        let keys = if let Some(a) = message.get("account_keys").and_then(|a| a.as_array()) { a } else { continue };
        let instructions = if let Some(a) = message.get("instructions").and_then(|a| a.as_array()) { a } else { continue };

        // Resolve base58 accounts for quick comparisons
        let keys_b58: Vec<String> = keys.iter().map(|k| key_to_base58(k)).collect();

        for ins in instructions {
            // Determine program id for instruction
            let program_val = if let Some(v) = ins.get("program_id") {
                Some(v.clone())
            } else if let Some(ix) = ins.get("program_id_index").and_then(|v| v.as_i64()) {
                keys.get(ix as usize).cloned()
            } else { None };
            let program_hex = program_val.as_ref().map(|v| key_to_hex(v)).unwrap_or_default();
            let program_b58 = program_val.as_ref().map(|v| key_to_base58(v)).unwrap_or_default();

            let acc_idx: Vec<usize> = ins.get("accounts").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as usize)).collect()).unwrap_or_default();
            let accounts_b58: Vec<String> = acc_idx.iter().filter_map(|i| keys_b58.get(*i)).cloned().collect();
            let data_vec: Vec<u8> = ins.get("data").and_then(|d| d.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as u8)).collect()).unwrap_or_default();

            // Only consider System Program transfers
            let is_system = is_system_b58(&program_b58)
                || program_hex.eq_ignore_ascii_case("0000000000000000000000000000000000000000000000000000000000000000")
                || program_hex.eq_ignore_ascii_case("0000000000000000000000000000000000000000000000000000000000000001");
            if !is_system || data_vec.len() < 4 { continue; }

            let tag = u32_le(&data_vec[0..4]).unwrap_or(9999);
            // 0: CreateAccount { lamports: u64, space: u64, owner: Pubkey }
            if tag == 0 && data_vec.len() >= 12 && accounts_b58.len() >= 2 {
                let lamports = u64_le(&data_vec[4..12]).unwrap_or(0) as i128;
                let src = accounts_b58.get(0);
                let dst = accounts_b58.get(1);
                if let Some(d) = dst { if d == address_b58 { balance_delta += lamports; }
                }
                if let Some(s) = src { if s == address_b58 { balance_delta -= lamports; }
                }
                continue;
            }
            if (tag == 2 || tag == 4) && data_vec.len() >= 12 && accounts_b58.len() >= 2 {
                let lamports = u64_le(&data_vec[4..12]).unwrap_or(0) as i128;
                let src = accounts_b58.get(0);
                let dst = accounts_b58.get(1);
                if let Some(d) = dst { if d == address_b58 { balance_delta += lamports; continue; } }
                if let Some(s) = src { if s == address_b58 { balance_delta -= lamports; continue; } }
            }
            // Fallback: exactly 12 bytes payload treated as transfer
            if data_vec.len() == 12 && accounts_b58.len() >= 2 {
                let lamports = u64_le(&data_vec[4..12]).unwrap_or(0) as i128;
                let src = accounts_b58.get(0);
                let dst = accounts_b58.get(1);
                if let Some(d) = dst { if d == address_b58 { balance_delta += lamports; continue; } }
                if let Some(s) = src { if s == address_b58 { balance_delta -= lamports; continue; } }
            }
        }
    }

    Ok(balance_delta)
}

pub async fn get_account_transactions(
    State(pool): State<Arc<PgPool>>,
    AxPath(address): AxPath<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok()).map(|v| v.min(200)).unwrap_or(50);
    let page = params.get("page").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let offset = (page - 1) * limit;
    let address_hex = normalize_program_param(&address).ok_or(ApiError::BadRequest("Invalid address".into()))?;

    let has_participation: bool = sqlx::query_scalar(
        r#"SELECT to_regclass('public.account_participation') IS NOT NULL"#
    )
    .fetch_one(&*pool)
    .await
    .unwrap_or(false);

    // Only use participation if it actually has rows for this address; otherwise fallback to scanning transactions
    let use_participation: bool = if has_participation {
        sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM account_participation WHERE address_hex ILIKE $1)"#
        )
        .bind(&address_hex)
        .fetch_one(&*pool)
        .await
        .unwrap_or(false)
    } else { false };

    let rows = if use_participation {
        sqlx::query(
            r#"
            SELECT ap.txid, ap.block_height, ap.created_at
            FROM account_participation ap
            WHERE ap.address_hex ILIKE $1
            ORDER BY ap.created_at DESC
            LIMIT $2 OFFSET $3
            "#)
            .bind(&address_hex)
            .bind(limit)
            .bind(offset)
            .fetch_all(&*pool)
            .await?
    } else {
        sqlx::query(
            r#"
            WITH accs AS (
                SELECT t.txid, t.block_height, t.created_at,
                    CASE 
                        WHEN jsonb_typeof(acc.value) = 'string' THEN normalize_program_id(trim(both '"' from (acc.value)::text))
                        ELSE normalize_program_id((acc.value)::text)
                    END AS acc_hex
                FROM transactions t
                CROSS JOIN LATERAL jsonb_array_elements(COALESCE(t.data#>'{message,account_keys}', t.data#>'{message,keys}', '[]'::jsonb)) AS acc(value)
            )
            SELECT txid, block_height, created_at
            FROM accs
            WHERE acc_hex ILIKE $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#)
            .bind(&address_hex)
            .bind(limit)
            .bind(offset)
            .fetch_all(&*pool)
            .await
            .map_err(|e| { error!("get_account_transactions fallback query error: {:?}", e); ApiError::Database(e) })?
    };

    let list: Vec<serde_json::Value> = rows.into_iter().map(|r| json!({
        "txid": r.get::<String,_>("txid"),
        "block_height": r.get::<i64,_>("block_height"),
        "created_at": r.get::<DateTime<Utc>,_>("created_at"),
    })).collect();

    Ok(Json(json!({
        "page": page,
        "limit": limit,
        "transactions": list
    })))
}

pub async fn get_account_transactions_v2(
    State(pool): State<Arc<PgPool>>,
    AxPath(address): AxPath<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok()).map(|v| v.min(200)).unwrap_or(50);
    let page = params.get("page").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let offset = (page - 1) * limit;
    let address_hex = normalize_program_param(&address).ok_or(ApiError::BadRequest("Invalid address".into()))?;

    // We prefer participation if available
    let use_participation: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_name = 'account_participation')"#
    )
    .fetch_one(&*pool)
    .await
    .unwrap_or(false)
    && sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM account_participation WHERE address_hex ILIKE $1)"#
    )
    .bind(&address_hex)
    .fetch_one(&*pool)
    .await
    .unwrap_or(false);

    // Fetch candidate txids
    let tx_rows = if use_participation {
        sqlx::query(
            r#"
            SELECT ap.txid, ap.block_height, ap.created_at
            FROM account_participation ap
            WHERE ap.address_hex ILIKE $1
            ORDER BY ap.created_at DESC
            LIMIT $2 OFFSET $3
            "#
        )
        .bind(&address_hex)
        .bind(limit)
        .bind(offset)
        .fetch_all(&*pool)
        .await?
    } else {
        sqlx::query(
            r#"
            WITH accs AS (
                SELECT t.txid, t.block_height, t.created_at,
                       CASE 
                         WHEN jsonb_typeof(acc.value) = 'string' THEN normalize_program_id(trim(both '"' from (acc.value)::text))
                         ELSE normalize_program_id((acc.value)::text)
                       END AS acc_hex
                FROM transactions t
                CROSS JOIN LATERAL jsonb_array_elements(COALESCE(t.data#>'{message,account_keys}', t.data#>'{message,keys}', '[]'::jsonb)) AS acc(value)
            )
            SELECT txid, block_height, created_at
            FROM accs
            WHERE acc_hex ILIKE $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#
        )
        .bind(&address_hex)
        .bind(limit)
        .bind(offset)
        .fetch_all(&*pool)
        .await?
    };

    // Hydrate each transaction with enriched fields
    let mut out: Vec<serde_json::Value> = Vec::with_capacity(tx_rows.len());
    for r in tx_rows {
        let txid: String = r.get("txid");
        let block_height: i64 = r.get("block_height");
        let created_at: DateTime<Utc> = r.get("created_at");

        // Load transaction JSON and logs for compute units, programs, and instruction chips
        let row = sqlx::query(
            r#"
            SELECT data, status, logs, compute_units_consumed
            FROM transactions
            WHERE txid = $1
            "#
        )
        .bind(&txid)
        .fetch_optional(&*pool)
        .await?;

        let (data, status, logs, compute_units): (serde_json::Value, String, Vec<String>, Option<i64>) = if let Some(rr) = row {
            let d: serde_json::Value = rr.get("data");
            let s: serde_json::Value = rr.get("status");
            let st = if s.is_string() { s.as_str().unwrap_or("").to_string() } else { s.to_string() };
            let l: serde_json::Value = rr.get("logs");
            let logs_vec = l.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_else(Vec::new);
            let cu: Option<i64> = rr.try_get::<Option<i32>, _>("compute_units_consumed").ok().flatten().map(|v| v as i64);
            (d, st, logs_vec, cu)
        } else { (serde_json::json!({}), "unknown".to_string(), Vec::new(), None) };

        // Fee payer (first signer in message header; or participants row order) best-effort
        let fee_payer = data.get("message")
            .and_then(|m| m.get("account_keys").or_else(|| m.get("keys")))
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(0))
            .map(|k| key_to_base58(k))
            .unwrap_or_default();

        // Programs involved (from transaction_programs if present; else derive from instructions)
        let prows = sqlx::query(
            r#"SELECT program_id FROM transaction_programs WHERE txid = $1 LIMIT 20"#
        )
        .bind(&txid)
        .fetch_all(&*pool)
        .await
        .unwrap_or_default();
        let programs_hex: Vec<String> = prows.into_iter().map(|pr| pr.get::<String,_>("program_id")).collect();
        let programs_b58: Vec<String> = programs_hex.iter().map(|h| try_hex_to_base58(h)).collect();

        // Build instruction summaries directly from transaction JSON using our decoder
        let instrs = build_instruction_summaries_from_tx(&data);
        let chips: Vec<String> = instrs.iter().filter_map(|v| v.get("action").and_then(|a| a.as_str()).map(|s| s.to_string())).collect();

        // Fee estimation: unit price from any instruction tag (ComputeBudget SetComputeUnitPrice) × compute units
        let unit_price_micro: Option<u64> = instrs.iter().find_map(|ins| {
            let decoded = ins.get("decoded")?;
            let price = decoded.get("price_micro_lamports").and_then(|v| v.get("data")).and_then(|v| v.as_u64());
            price
        });
        let fee_estimated_arch: Option<f64> = match (unit_price_micro, compute_units) {
            (Some(price), Some(units)) => Some((price as f64 / 1_000_000.0) * units as f64 / 1_000_000_000.0),
            _ => None,
        };

        // Native value delta for this address
        let value_delta_lamports = compute_native_balance_delta_for_address_in_tx(&data, &address_hex).unwrap_or(0);
        let value_arch = (value_delta_lamports as f64) / 1_000_000_000.0;

        out.push(json!({
            "txid": txid,
            "block_height": block_height,
            "created_at": created_at,
            "status": status,
            "fee_payer": fee_payer,
            "value_arch": value_arch,
            "fee_estimated_arch": fee_estimated_arch,
            "programs": programs_b58,
            "instructions": chips,
        }));
    }

    Ok(Json(json!({
        "page": page,
        "limit": limit,
        "transactions": out
    })))
}

#[derive(serde::Serialize)]
pub struct TokenLeaderboardRow {
    pub mint_address: String,
    pub program_id: String,
    pub holders: i64,
    pub total_balance: String,
    pub decimals: i32,
    pub supply: Option<String>,
    pub mint_authority: Option<String>,
}

pub async fn get_token_leaderboard(
    State(pool): State<Arc<PgPool>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok()).map(|v| v.min(200)).unwrap_or(50);
    let page = params.get("page").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let offset = (page - 1) * limit;
    let authority = params.get("authority").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    // Guard: required tables must exist, otherwise return empty result gracefully
    let has_token_balances: bool = sqlx::query_scalar(
        r#"SELECT to_regclass('public.token_balances') IS NOT NULL"#
    )
    .fetch_one(&*pool)
    .await
    .unwrap_or(false);
    if !has_token_balances {
        return Ok(Json(json!({
            "page": page,
            "limit": limit,
            "total": 0i64,
            "tokens": []
        })));
    }
    let has_token_mints: bool = sqlx::query_scalar(
        r#"SELECT to_regclass('public.token_mints') IS NOT NULL"#
    )
    .fetch_one(&*pool)
    .await
    .unwrap_or(false);

    // Aggregate holders and balances per mint from indexed tables
    let (items, total): (Vec<TokenLeaderboardRow>, i64) = if has_token_mints {
        let base_sql = r#"
            WITH agg AS (
                SELECT 
                    tb.mint_address,
                    COALESCE(MAX(tb.program_id), '') AS program_id,
                    COUNT(DISTINCT tb.account_address) AS holders,
                    SUM(tb.balance)::text AS total_balance,
                    COALESCE(MAX(tb.decimals), 0) AS decimals
                FROM token_balances tb
                GROUP BY tb.mint_address
            )
            SELECT 
                a.mint_address,
                a.program_id,
                a.holders,
                a.total_balance,
                a.decimals,
                tm.supply,
                tm.mint_authority
            FROM agg a
            LEFT JOIN token_mints tm ON tm.mint_address = a.mint_address
            WHERE COALESCE(tm.program_id, a.program_id) IN (
                normalize_program_id('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'),
                normalize_program_id('AplToken111111111111111111111111')
            )
        "#;
        let sql = if authority.is_some() {
            format!("{} AND tm.mint_authority = $3 ORDER BY a.holders DESC, a.total_balance DESC LIMIT $1 OFFSET $2", base_sql)
        } else {
            format!("{} ORDER BY a.holders DESC, a.total_balance DESC LIMIT $1 OFFSET $2", base_sql)
        };

        let mut q = sqlx::query(&sql)
            .bind(limit)
            .bind(offset);
        if let Some(a) = &authority { q = q.bind(a); }
        let rows = q.fetch_all(&*pool)
        .await
        .map_err(ApiError::Database)?;

        let items: Vec<TokenLeaderboardRow> = rows.into_iter().map(|r| TokenLeaderboardRow {
            mint_address: r.get::<String, _>("mint_address"),
            program_id: r.get::<String, _>("program_id"),
            holders: r.get::<i64, _>("holders"),
            total_balance: { let v: String = r.get("total_balance"); v },
            decimals: r.get::<i32, _>("decimals"),
            supply: r.try_get::<Option<String>, _>("supply").ok().flatten(),
            mint_authority: r.try_get::<Option<String>, _>("mint_authority").ok().flatten(),
        }).collect();

        let total: i64 = if let Some(a) = &authority {
            sqlx::query_scalar("SELECT COUNT(*) FROM token_mints WHERE mint_authority = $1")
                .bind(a)
                .fetch_one(&*pool)
                .await
                .unwrap_or(0)
        } else {
            sqlx::query_scalar("SELECT COUNT(DISTINCT mint_address) FROM token_balances")
                .fetch_one(&*pool)
                .await
                .unwrap_or(0)
        };
        (items, total)
    } else {
        // token_mints doesn't exist yet; return aggregated balances without join, ignore authority filter
        let sql = r#"
            WITH agg AS (
                SELECT 
                    tb.mint_address,
                    COALESCE(MAX(tb.program_id), '') AS program_id,
                    COUNT(DISTINCT tb.account_address) AS holders,
                    SUM(tb.balance)::text AS total_balance,
                    COALESCE(MAX(tb.decimals), 0) AS decimals
                FROM token_balances tb
                GROUP BY tb.mint_address
            )
            SELECT * FROM agg a
            WHERE a.program_id IN (
                normalize_program_id('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'),
                normalize_program_id('AplToken111111111111111111111111')
            )
            ORDER BY a.holders DESC, a.total_balance DESC
            LIMIT $1 OFFSET $2
        "#;
        let rows = sqlx::query(sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&*pool)
            .await
            .map_err(ApiError::Database)?;
        let items: Vec<TokenLeaderboardRow> = rows.into_iter().map(|r| TokenLeaderboardRow {
            mint_address: r.get::<String, _>("mint_address"),
            program_id: r.get::<String, _>("program_id"),
            holders: r.get::<i64, _>("holders"),
            total_balance: { let v: String = r.get("total_balance"); v },
            decimals: r.get::<i32, _>("decimals"),
            supply: None,
            mint_authority: None,
        }).collect();
        let total: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT mint_address) FROM token_balances")
            .fetch_one(&*pool)
            .await
            .unwrap_or(0);
        (items, total)
    };

    Ok(Json(json!({
        "page": page,
        "limit": limit,
        "total": total,
        "tokens": items
    })))
}

#[derive(serde::Serialize)]
pub struct AccountProgramRow {
    pub program_id: String,
    pub program_id_base58: String,
    pub transaction_count: i64,
}

pub async fn get_account_programs(
    State(pool): State<Arc<PgPool>>,
    AxPath(address): AxPath<String>,
) -> Result<Json<Vec<AccountProgramRow>>, ApiError> {
    let address_hex = normalize_program_param(&address).ok_or(ApiError::BadRequest("Invalid address".into()))?;
    let has_participation: bool = sqlx::query_scalar(
        r#"SELECT to_regclass('public.account_participation') IS NOT NULL"#
    )
    .fetch_one(&*pool)
    .await
    .unwrap_or(false);

    // Only use account_participation if it has rows for this address; otherwise fallback
    let use_participation: bool = if has_participation {
        sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM account_participation WHERE address_hex ILIKE $1)"#
        )
        .bind(&address_hex)
        .fetch_one(&*pool)
        .await
        .unwrap_or(false)
    } else { false };

    let rows = if use_participation {
        sqlx::query(
            r#"
            SELECT tp.program_id, COUNT(*)::bigint as cnt
            FROM account_participation ap
            JOIN transactions t ON t.txid = ap.txid
            JOIN transaction_programs tp ON tp.txid = t.txid
            WHERE ap.address_hex ILIKE $1
            GROUP BY tp.program_id
            ORDER BY cnt DESC
            LIMIT 200
            "#)
            .bind(&address_hex)
            .fetch_all(&*pool)
            .await?
    } else {
        sqlx::query(
            r#"
            WITH acc_txs AS (
                SELECT 
                    t.txid,
                    t.data->'message'->'account_keys' AS keys,
                    jsonb_array_elements(COALESCE(t.data->'message'->'instructions', '[]'::jsonb)) AS inst
                FROM transactions t
                WHERE EXISTS (
                    SELECT 1 
                    FROM jsonb_array_elements(t.data->'message'->'account_keys') AS acc(value)
                    WHERE normalize_program_id(acc.value) ILIKE $1
                )
            ),
            progs AS (
                SELECT 
                    normalize_program_id(
                        (keys -> ((inst->>'program_id_index')::int))
                    ) AS program_id
                FROM acc_txs
            )
            SELECT program_id, COUNT(*)::bigint AS cnt
            FROM progs
            WHERE program_id IS NOT NULL
            GROUP BY program_id
            ORDER BY cnt DESC
            LIMIT 200
            "#)
            .bind(&address_hex)
            .fetch_all(&*pool)
            .await
            .map_err(|e| { error!("get_account_programs derived fallback error: {:?}", e); ApiError::Database(e) })?
    };
    let mut out: Vec<AccountProgramRow> = rows.into_iter().map(|r| {
        let pid_hex: String = r.get::<String,_>("program_id");
        let pid_b58: String = bs58::encode(hex::decode(&pid_hex).unwrap_or_default()).into_string();
        AccountProgramRow {
            program_id: pid_hex,
            program_id_base58: pid_b58,
            transaction_count: r.get::<i64,_>("cnt"),
        }
    }).collect();

    if out.is_empty() {
        // As a last resort, if the account participated but no programs were linked (e.g., only System),
        // derive total tx count for this account and attribute to System program.
        let total_for_acct: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)::bigint
            FROM transactions t
            WHERE EXISTS (
                SELECT 1 FROM jsonb_array_elements(t.data->'message'->'account_keys') AS acc(value)
                WHERE normalize_program_id(acc.value) ILIKE $1
            )
            "#
        )
        .bind(&address_hex)
        .fetch_one(&*pool)
        .await
        .unwrap_or(0);

        if total_for_acct > 0 {
            let sys_hex = "0000000000000000000000000000000000000000000000000000000000000001".to_string();
            let sys_b58 = bs58::encode(hex::decode(&sys_hex).unwrap_or_default()).into_string();
            out.push(AccountProgramRow {
                program_id: sys_hex,
                program_id_base58: sys_b58,
                transaction_count: total_for_acct,
            });
        }
    }

    Ok(Json(out))
}

pub async fn get_account_token_balances(
    State(pool): State<Arc<PgPool>>,
    AxPath(address): AxPath<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok()).map(|v| v.min(200)).unwrap_or(50);
    let page = params.get("page").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let offset = (page - 1) * limit;
    let address_hex = normalize_program_param(&address).ok_or(ApiError::BadRequest("Invalid address".into()))?;

    // Check if token_balances table exists
    let has_token_balances: bool = sqlx::query_scalar(
        r#"SELECT to_regclass('public.token_balances') IS NOT NULL"#
    )
    .fetch_one(&*pool)
    .await
    .unwrap_or(false);

    if !has_token_balances {
        // Return empty result if table doesn't exist yet
        return Ok(Json(json!({
            "page": page,
            "limit": limit,
            "balances": [],
            "total": 0
        })));
    }

    // Get total count
    let total_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM token_balances tb
        LEFT JOIN token_mints tm ON tb.mint_address = tm.mint_address
        WHERE tb.account_address ILIKE $1
        "#
    )
    .bind(address_hex.clone())
    .fetch_one(&*pool)
    .await
    .unwrap_or(0);

    // Get token balances with pagination
    let rows = sqlx::query(
        r#"
        SELECT 
            tb.mint_address,
            tb.balance::text AS balance,
            tb.decimals,
            tb.owner_address,
            tb.program_id,
            tm.supply::text AS supply,
            tm.is_frozen,
            tb.last_updated
        FROM token_balances tb
        LEFT JOIN token_mints tm ON tb.mint_address = tm.mint_address
        WHERE tb.account_address ILIKE $1
        ORDER BY tb.last_updated DESC
        LIMIT $2 OFFSET $3
        "#
    )
    .bind(address_hex.clone())
    .bind(limit)
    .bind(offset)
    .fetch_all(&*pool)
    .await?;

    // Build balances and compute total appropriately (fallback path computes its own total)
    let (balances, total): (Vec<serde_json::Value>, i64) = if !rows.is_empty() {
        let list = rows.into_iter().map(|r| {
            let mint_address = r.get::<String, _>("mint_address");
            let program_id = r.get::<String, _>("program_id");
            
            // Convert hex to base58 for display
            let mint_address_b58 = try_hex_to_base58(&mint_address);
            let program_id_b58 = try_hex_to_base58(&program_id);
            
            // Get program display name
            let program_name = fallback_program_name_from_hex(&program_id);
            
            json!({
                "mint_address": if mint_address_b58.is_empty() { mint_address.clone() } else { mint_address_b58 },
                "mint_address_hex": mint_address,
                "balance": r.get::<String, _>("balance"),
                "decimals": r.get::<i32, _>("decimals"),
                "owner_address": r.try_get::<Option<String>, _>("owner_address").ok().flatten(),
                "program_id": if program_id_b58.is_empty() { program_id.clone() } else { program_id_b58 },
                "program_name": program_name,
                "supply": r.try_get::<Option<String>, _>("supply").ok().flatten(),
                "is_frozen": r.try_get::<Option<bool>, _>("is_frozen").ok().flatten(),
                "last_updated": r.get::<DateTime<Utc>, _>("last_updated")
            })
        }).collect();
        (list, total_count)
    } else {
        // Fallback: compute balances on the fly from past transactions that include this account
        let target_hex = address_hex.clone();
        let target_b58 = try_hex_to_base58(&target_hex);

        // Determine if participation table is available
        let has_participation: bool = sqlx::query_scalar(
            r#"SELECT to_regclass('public.account_participation') IS NOT NULL"#
        )
        .fetch_one(&*pool)
        .await
        .unwrap_or(false);

        let mut tx_rows = if has_participation {
            sqlx::query(
                r#"
                SELECT t.data, t.created_at
                FROM transactions t
                JOIN account_participation ap ON ap.txid = t.txid
                WHERE ap.address_hex ILIKE $1
                ORDER BY t.created_at ASC
                LIMIT 2000
                "#
            )
            .bind(target_hex.clone())
            .fetch_all(&*pool)
            .await
            .unwrap_or_default()
        } else { Vec::new() };

        if tx_rows.is_empty() {
            // Fallback to scanning transactions JSON directly if participation table is missing or has no matches
            tx_rows = sqlx::query(
                r#"
                WITH accs AS (
                    SELECT t.txid, t.created_at, t.data,
                        CASE 
                            WHEN jsonb_typeof(acc.value) = 'string' THEN normalize_program_id(trim(both '"' from (acc.value)::text))
                            ELSE normalize_program_id((acc.value)::text)
                        END AS acc_hex
                    FROM transactions t
                    CROSS JOIN LATERAL jsonb_array_elements(t.data->'message'->'account_keys') AS acc(value)
                )
                SELECT data, created_at
                FROM accs
                WHERE acc_hex ILIKE $1
                ORDER BY created_at ASC
                LIMIT 2000
                "#
            )
            .bind(target_hex.clone())
            .fetch_all(&*pool)
            .await
            .unwrap_or_default();
        }

        if tx_rows.is_empty() {
            // Ultimate fallback: scan a window of recent transactions without pre-filter and compute balances
            tx_rows = sqlx::query(
                r#"
                SELECT data, created_at
                FROM transactions
                ORDER BY created_at ASC
                LIMIT 2000
                "#
            )
            .fetch_all(&*pool)
            .await
            .unwrap_or_default();
        }

        let mut by_mint: std::collections::HashMap<String, (i128, i32)> = std::collections::HashMap::new();

        for row in tx_rows {
            let data: serde_json::Value = row.get("data");
            let message = if let Some(m) = data.get("message") { m } else { continue };
            let keys = if let Some(a) = message.get("account_keys").and_then(|a| a.as_array()) { a } else { continue };
            let instructions = if let Some(a) = message.get("instructions").and_then(|a| a.as_array()) { a } else { continue };

            for ins in instructions {
                // Resolve program id: explicit or via program_id_index
                let program_val = if let Some(v) = ins.get("program_id") {
                    Some(v.clone())
                } else if let Some(ix) = ins.get("program_id_index").and_then(|v| v.as_i64()) {
                    keys.get(ix as usize).cloned()
                } else { None };
                let program_hex = program_val.as_ref().map(|v| key_to_hex(v)).unwrap_or_default();
                let program_b58 = program_val.as_ref().map(|v| key_to_base58(v)).unwrap_or_default();
                if program_b58 != pid::APL_TOKEN_PROGRAM && program_b58 != pid::SOL_SPL_TOKEN { continue; }

                let acc_idx: Vec<usize> = ins.get("accounts").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as usize)).collect()).unwrap_or_default();
                let accounts_b58: Vec<String> = acc_idx.iter().filter_map(|i| keys.get(*i).map(|k| key_to_base58(k))).collect();
                let data_vec: Vec<u8> = ins.get("data").and_then(|d| d.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as u8)).collect()).unwrap_or_default();

                if data_vec.is_empty() { continue; }
                let tag = data_vec[0];

                let mint_hex_from_idx = |idx: usize| -> Option<String> {
                    acc_idx.get(idx).and_then(|i| keys.get(*i)).map(|v| key_to_hex(v)).filter(|s| !s.is_empty())
                };

                let is_me = |addr: &Option<String>| -> bool {
                    if let Some(a) = addr { !target_b58.is_empty() && a == &target_b58 } else { false }
                };

                match tag {
                    3 => {
                        // Transfer { amount: u64 }, accounts: [source, destination, authority]
                        if data_vec.len() >= 1 + 8 && accounts_b58.len() >= 2 {
                            let amount = u64::from_le_bytes([data_vec[1],data_vec[2],data_vec[3],data_vec[4],data_vec[5],data_vec[6],data_vec[7],data_vec[8]]) as i128;
                            let src = accounts_b58.get(0).cloned();
                            let dst = accounts_b58.get(1).cloned();
                            let delta = if is_me(&dst) { amount } else if is_me(&src) { -amount } else { 0 };
                            if delta != 0 {
                                let mint = if !program_hex.is_empty() { program_hex.clone() } else { program_b58.clone() };
                                let e = by_mint.entry(mint).or_insert((0, 0));
                                e.0 += delta;
                            }
                        }
                    }
                    12 => {
                        // TransferChecked { amount: u64, decimals: u8 }
                        if data_vec.len() >= 1 + 8 + 1 && accounts_b58.len() >= 4 {
                            let amount = u64::from_le_bytes([data_vec[1],data_vec[2],data_vec[3],data_vec[4],data_vec[5],data_vec[6],data_vec[7],data_vec[8]]) as i128;
                            let decimals = data_vec[9] as i32;
                            let mint_hex = mint_hex_from_idx(2).or_else(|| mint_hex_from_idx(1));
                            if let Some(mint) = mint_hex {
                                let src = accounts_b58.get(1).cloned();
                                let dst = accounts_b58.get(3).cloned();
                                let delta = if is_me(&dst) { amount } else if is_me(&src) { -amount } else { 0 };
                                if delta != 0 {
                                    let e = by_mint.entry(mint).or_insert((0, decimals));
                                    e.0 += delta;
                                    if e.1 == 0 { e.1 = decimals; }
                                }
                            }
                        }
                    }
                    7 => {
                        // MintTo { amount: u64 }
                        if data_vec.len() >= 1 + 8 && accounts_b58.len() >= 2 {
                            let amount = u64::from_le_bytes([data_vec[1],data_vec[2],data_vec[3],data_vec[4],data_vec[5],data_vec[6],data_vec[7],data_vec[8]]) as i128;
                            let mint_hex = mint_hex_from_idx(0);
                            let dst = accounts_b58.get(1).cloned();
                            if let (Some(mint), Some(d)) = (mint_hex, dst) {
                                if !target_b58.is_empty() && d == target_b58 {
                                    let e = by_mint.entry(mint).or_insert((0, 0));
                                    e.0 += amount;
                                }
                            }
                        }
                    }
                    14 => {
                        // MintToChecked { amount: u64, decimals: u8 }
                        if data_vec.len() >= 1 + 8 + 1 && accounts_b58.len() >= 2 {
                            let amount = u64::from_le_bytes([data_vec[1],data_vec[2],data_vec[3],data_vec[4],data_vec[5],data_vec[6],data_vec[7],data_vec[8]]) as i128;
                            let decimals = data_vec[9] as i32;
                            let mint_hex = mint_hex_from_idx(0);
                            let dst = accounts_b58.get(1).cloned();
                            if let (Some(mint), Some(d)) = (mint_hex, dst) {
                                if !target_b58.is_empty() && d == target_b58 {
                                    let e = by_mint.entry(mint).or_insert((0, decimals));
                                    e.0 += amount;
                                    if e.1 == 0 { e.1 = decimals; }
                                }
                            }
                        }
                    }
                    8 => {
                        // Burn { amount: u64 }
                        if data_vec.len() >= 1 + 8 && accounts_b58.len() >= 2 {
                            let amount = u64::from_le_bytes([data_vec[1],data_vec[2],data_vec[3],data_vec[4],data_vec[5],data_vec[6],data_vec[7],data_vec[8]]) as i128;
                            let mint_hex = mint_hex_from_idx(1);
                            let acc = accounts_b58.get(0).cloned();
                            if let (Some(mint), Some(a)) = (mint_hex, acc) {
                                if !target_b58.is_empty() && a == target_b58 {
                                    let e = by_mint.entry(mint).or_insert((0, 0));
                                    e.0 -= amount;
                                }
                            }
                        }
                    }
                    15 => {
                        // BurnChecked { amount: u64, decimals: u8 }
                        if data_vec.len() >= 1 + 8 + 1 && accounts_b58.len() >= 2 {
                            let amount = u64::from_le_bytes([data_vec[1],data_vec[2],data_vec[3],data_vec[4],data_vec[5],data_vec[6],data_vec[7],data_vec[8]]) as i128;
                            let decimals = data_vec[9] as i32;
                            let mint_hex = mint_hex_from_idx(1);
                            let acc = accounts_b58.get(0).cloned();
                            if let (Some(mint), Some(a)) = (mint_hex, acc) {
                                if !target_b58.is_empty() && a == target_b58 {
                                    let e = by_mint.entry(mint).or_insert((0, decimals));
                                    e.0 -= amount;
                                    if e.1 == 0 { e.1 = decimals; }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let list: Vec<serde_json::Value> = by_mint.into_iter().map(|(mint_hex, (amount, decimals))| {
            let mint_b58 = try_hex_to_base58(&mint_hex);
            json!({
                "mint_address": if mint_b58.is_empty() { mint_hex.clone() } else { mint_b58 },
                "mint_address_hex": mint_hex,
                "balance": amount.to_string(),
                "decimals": decimals.max(0),
                "owner_address": null,
                // we don't reliably know the program here; leave hex empty for now
                "program_id": "",
                "program_name": null,
                "supply": null,
                "is_frozen": null,
                "last_updated": chrono::Utc::now(),
            })
        }).collect();
        let computed_total = list.len() as i64;
        (list, computed_total)
    };

    Ok(Json(json!({
        "page": page,
        "limit": limit,
        "balances": balances,
        "total": total
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
            b.timestamp::timestamptz as timestamp,
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
            let ts_utc = r.get::<chrono::DateTime<Utc>, _>("timestamp");
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

/// Simple health check. Verifies DB connectivity and returns 200 OK on success.
pub async fn health_check(State(pool): State<Arc<PgPool>>) -> axum::response::Response {
    let ok = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&*pool)
        .await
        .map(|v| v == 1)
        .unwrap_or(false);
    if ok {
        (AxStatusCode::OK, "OK").into_response()
    } else {
        (AxStatusCode::INTERNAL_SERVER_ERROR, "DB not ready").into_response()
    }
}

/// Return ranges of missing block heights within current indexed bounds and a total count
pub async fn get_block_gaps(State(pool): State<Arc<PgPool>>) -> Result<Json<serde_json::Value>, ApiError> {
    let bounds_row = sqlx::query(
        r#"SELECT MIN(height) AS min_height, MAX(height) AS max_height, COUNT(*) as total FROM blocks"#
    )
    .fetch_one(&*pool)
    .await?;

    let min_height: i64 = bounds_row.get::<Option<i64>, _>("min_height").unwrap_or(0);
    let max_height: i64 = bounds_row.get::<Option<i64>, _>("max_height").unwrap_or(0);
    if max_height <= min_height {
        return Ok(Json(json!({ "ranges": [], "missing_count": 0, "min": min_height, "max": max_height })));
    }

    let chunk_size: i64 = 100_000;
    let mut missing: Vec<(i64, i64)> = Vec::new();
    let mut missing_count: i64 = 0;
    let mut cursor = min_height;
    while cursor <= max_height {
        let end = (cursor + chunk_size - 1).min(max_height);
        let rows = sqlx::query(
            r#"SELECT height FROM blocks WHERE height >= $1 AND height <= $2 ORDER BY height"#
        )
        .bind(cursor)
        .bind(end)
        .fetch_all(&*pool)
        .await?;

        let set: HashSet<i64> = rows.iter().map(|r| r.get::<i64, _>("height")).collect();
        let mut run_start: Option<i64> = None;
        for h in cursor..=end {
            if !set.contains(&h) {
                missing_count += 1;
                if run_start.is_none() { run_start = Some(h); }
            } else if let Some(s) = run_start.take() {
                missing.push((s, h - 1));
            }
        }
        if let Some(s) = run_start.take() { missing.push((s, end)); }
        cursor = end + 1;
    }

    Ok(Json(json!({
        "ranges": missing.into_iter().map(|(s,e)| json!({"start": s, "end": e})).collect::<Vec<_>>(),
        "missing_count": missing_count,
        "min": min_height,
        "max": max_height,
    })))
}

/// Backfill missing blocks by fetching from RPC and inserting into DB.
/// WARNING: Long-running; use small limits in production.
pub async fn backfill_missing_blocks(
    State(pool): State<Arc<PgPool>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let max_to_process: i64 = params
        .get("limit")
        .and_then(|v| v.parse::<i64>().ok())
        .map(|v| v.max(1).min(20_000))
        .unwrap_or(5_000);

    // Compute missing heights (reuse logic from get_block_gaps but stop once we have enough)
    let bounds_row = sqlx::query(
        r#"SELECT MIN(height) AS min_height, MAX(height) AS max_height FROM blocks"#
    )
    .fetch_one(&*pool)
    .await?;
    let min_height: i64 = bounds_row.get::<Option<i64>, _>("min_height").unwrap_or(0);
    let max_height: i64 = bounds_row.get::<Option<i64>, _>("max_height").unwrap_or(0);

    if max_height <= min_height {
        return Ok(Json(json!({ "processed": 0, "message": "no gaps detected" })));
    }

    let chunk_size: i64 = 100_000;
    let mut to_fill: Vec<i64> = Vec::new();
    let mut cursor = min_height;
    'outer: while cursor <= max_height {
        let end = (cursor + chunk_size - 1).min(max_height);
        let rows = sqlx::query(
            r#"SELECT height FROM blocks WHERE height >= $1 AND height <= $2 ORDER BY height"#
        )
        .bind(cursor)
        .bind(end)
        .fetch_all(&*pool)
        .await?;
        let set: HashSet<i64> = rows.iter().map(|r| r.get::<i64, _>("height")).collect();
        for h in cursor..=end {
            if !set.contains(&h) {
                to_fill.push(h);
                if (to_fill.len() as i64) >= max_to_process { break 'outer; }
            }
        }
        cursor = end + 1;
    }

    if to_fill.is_empty() {
        return Ok(Json(json!({ "processed": 0, "message": "no gaps detected" })));
    }

    let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
    let arch_client = ArchRpcClient::new(rpc_url);

    let mut processed: i64 = 0;
    for &height in &to_fill {
        // Fetch block
        let hash = match arch_client.get_block_hash(height).await {
            Ok(h) => h,
            Err(e) => { error!("backfill: get_block_hash {} failed: {:?}", height, e); continue; }
        };
        let block = match arch_client.get_block(&hash, height).await {
            Ok(b) => b,
            Err(e) => { error!("backfill: get_block {} failed: {:?}", height, e); continue; }
        };

        // Insert block
        let timestamp = crate::utils::convert_arch_timestamp(block.timestamp);
        if let Err(e) = sqlx::query(
            r#"INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (height) DO UPDATE SET hash = EXCLUDED.hash, timestamp = EXCLUDED.timestamp, bitcoin_block_height = EXCLUDED.bitcoin_block_height"#
        )
        .bind(block.height)
        .bind(&block.hash)
        .bind(timestamp)
        .bind(block.bitcoin_block_height)
        .execute(&*pool)
        .await {
            error!("backfill: insert block {} failed: {:?}", height, e);
            continue;
        }

        // Insert transactions (if any) – fetch full details first
        for txid in &block.transactions {
            let ptx = match arch_client.get_processed_transaction(txid).await {
                Ok(t) => t,
                Err(e) => { error!("backfill: get_processed_transaction {} failed: {:?}", txid, e); continue; }
            };

            let compute_units: Option<i32> = if let Some(logs) = ptx.runtime_transaction.get("logs") {
                if let Some(arr) = logs.as_array() {
                    arr.iter().filter_map(|log| log.as_str()).find_map(|log| {
                        if log.contains("Consumed") { log.split_whitespace().filter_map(|w| w.parse::<i32>().ok()).next() } else { None }
                    })
                } else { None }
            } else { None };

            if let Err(e) = sqlx::query(
                r#"INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids, created_at, logs, rollback_status, accounts_tags, compute_units_consumed)
                    VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP, $6, $7, $8, $9)
                    ON CONFLICT (txid) DO UPDATE SET block_height = EXCLUDED.block_height, data = EXCLUDED.data, status = EXCLUDED.status, bitcoin_txids = EXCLUDED.bitcoin_txids, logs = EXCLUDED.logs, rollback_status = EXCLUDED.rollback_status, accounts_tags = EXCLUDED.accounts_tags, compute_units_consumed = EXCLUDED.compute_units_consumed"#
            )
            .bind(txid)
            .bind(block.height)
            .bind(serde_json::to_value(&ptx.runtime_transaction).unwrap_or(serde_json::Value::Null))
            .bind(serde_json::to_value(&ptx.status).unwrap_or(serde_json::Value::Null))
            .bind(ptx.bitcoin_txids.as_ref().map(|v| v.as_slice()))
            .bind(serde_json::to_value(ptx.runtime_transaction.get("logs").unwrap_or(&serde_json::Value::Array(vec![]))).unwrap_or(serde_json::Value::Null))
            .bind(serde_json::to_value("NotRolledback").unwrap_or(serde_json::Value::Null))
            .bind(serde_json::to_value(&ptx.accounts_tags).unwrap_or(serde_json::Value::Array(vec![])))
            .bind(compute_units)
            .execute(&*pool)
            .await {
                error!("backfill: upsert tx {}@{} failed: {:?}", txid, height, e);
            }
        }

        processed += 1;
    }

    Ok(Json(json!({ "requested": max_to_process, "processed": processed, "remaining_estimate": (to_fill.len() as i64 - processed).max(0) })))
}

/// Return explicit list of missing block heights, with optional bounds and limit
pub async fn get_missing_block_heights(
    State(pool): State<Arc<PgPool>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Determine current DB bounds
    let bounds_row = sqlx::query(
        r#"SELECT MIN(height) AS min_height, MAX(height) AS max_height FROM blocks"#
    )
    .fetch_one(&*pool)
    .await?;

    let db_min: i64 = bounds_row.get::<Option<i64>, _>("min_height").unwrap_or(0);
    let db_max: i64 = bounds_row.get::<Option<i64>, _>("max_height").unwrap_or(0);

    // Allow scanning from genesis (0) by default; callers can override
    let mut scan_start: i64 = params.get("start").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    let mut scan_end: i64 = params.get("end").and_then(|v| v.parse::<i64>().ok()).unwrap_or(db_max);

    if scan_end < scan_start { return Ok(Json(json!({ "missing": [], "count": 0, "min": scan_start, "max": scan_end, "complete": true })));
    }

    let limit: i64 = params.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(10_000).max(1).min(1_000_000);

    let chunk_size: i64 = 100_000;
    let mut missing: Vec<i64> = Vec::new();
    let mut cursor = scan_start;
    let mut complete = true;
    'outer: while cursor <= scan_end {
        let end = (cursor + chunk_size - 1).min(scan_end);
        let rows = sqlx::query(
            r#"SELECT height FROM blocks WHERE height >= $1 AND height <= $2 ORDER BY height"#
        )
        .bind(cursor)
        .bind(end)
        .fetch_all(&*pool)
        .await?;
        let set: HashSet<i64> = rows.iter().map(|r| r.get::<i64, _>("height")).collect();
        for h in cursor..=end {
            if !set.contains(&h) {
                missing.push(h);
                if (missing.len() as i64) >= limit { complete = false; break 'outer; }
            }
        }
        cursor = end + 1;
    }

    Ok(Json(json!({
        "missing": missing,
        "count": missing.len(),
        "min": scan_start,
        "max": scan_end,
        "db_min": db_min,
        "db_max": db_max,
        "complete": complete
    })))
}

/// Backfill an explicit height range [start, end], inserting only heights not present
pub async fn backfill_block_range(
    State(pool): State<Arc<PgPool>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let start: i64 = params.get("start").and_then(|v| v.parse::<i64>().ok()).ok_or_else(|| ApiError::bad_request("start is required"))?;
    let end: i64 = params.get("end").and_then(|v| v.parse::<i64>().ok()).ok_or_else(|| ApiError::bad_request("end is required"))?;
    if end < start { return Ok(Json(json!({"processed": 0, "message": "empty range"}))); }

    let rpc = ArchRpcClient::new(std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string()));

    let mut processed = 0i64;
    let mut inserted = 0i64;
    let mut skipped = 0i64;
    for h in start..=end {
        // Skip if exists
        if let Some(_) = sqlx::query_scalar::<_, i64>("SELECT 1 FROM blocks WHERE height = $1")
            .bind(h)
            .fetch_optional(&*pool)
            .await? {
            skipped += 1; processed += 1; continue;
        }

        // Fetch block by height -> hash -> block data
        match rpc.get_block_hash(h).await {
            Ok(hash) => {
                match rpc.get_block(&hash, h).await {
                    Ok(block) => {
                        let ts_seconds = block.timestamp as i64;
                        let _ = sqlx::query(
                            r#"INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height)
                               VALUES ($1, $2, to_timestamp($3), $4)
                               ON CONFLICT (height) DO UPDATE SET hash = EXCLUDED.hash, timestamp = EXCLUDED.timestamp, bitcoin_block_height = EXCLUDED.bitcoin_block_height"#
                        )
                        .bind(h)
                        .bind(&block.hash)
                        .bind(ts_seconds)
                        .bind(block.bitcoin_block_height)
                        .execute(&*pool)
                        .await?;
                        inserted += 1;
                    },
                    Err(e) => {
                        return Ok(Json(json!({"processed": processed, "inserted": inserted, "skipped": skipped, "error": format!("get_block failed at {}: {}", h, e)})));
                    }
                }
            },
            Err(e) => {
                return Ok(Json(json!({"processed": processed, "inserted": inserted, "skipped": skipped, "error": format!("get_block_hash failed at {}: {}", h, e)})));
            }
        }
        processed += 1;
        if processed % 500 == 0 { tokio::task::yield_now().await; }
    }

    Ok(Json(json!({"processed": processed, "inserted": inserted, "skipped": skipped})))
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
    created_at: chrono::DateTime<chrono::Utc>,
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

    let ts_utc = row.get::<chrono::DateTime<Utc>, _>("timestamp");
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
            created_at::timestamptz as "created_at!: chrono::DateTime<chrono::Utc>"
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

    let ts_utc = row.get::<chrono::DateTime<Utc>, _>("timestamp");
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

    // Fetch paginated transactions newest-first, excluding placeholder rows
    // Placeholder rows are those with no runtime transaction payload (no data.message)
    // and no status.type field (e.g., legacy {"status":0}).
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
        WHERE (data ? 'message') OR (jsonb_typeof(status) = 'object' AND (status ? 'type') AND NULLIF(status->>'type','') IS NOT NULL)
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
            created_at: r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        })
        .collect();

    // Total transactions count for pagination (apply the same placeholder filter)
    let total_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM transactions
        WHERE (data ? 'message') OR (jsonb_typeof(status) = 'object' AND (status ? 'type') AND NULLIF(status->>'type','') IS NOT NULL)
        "#
    )
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
            created_at::timestamptz as "created_at!: chrono::DateTime<chrono::Utc>"
        FROM transactions 
        WHERE txid = $1
        "#,
        txid
    )
    .fetch_optional(&*pool)
    .await {
        Ok(Some(transaction)) => Ok(Json(transaction)),
        Ok(None) => {
            // Fallback: try RPC so we can serve transactions not yet persisted.
            // Additionally, opportunistically persist the transaction into Postgres so
            // account participation and program links populate immediately via triggers.
            let rpc_url = std::env::var("ARCH_NODE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string());
            let arch_client = ArchRpcClient::new(rpc_url);
            match arch_client.get_processed_transaction(&txid).await {
                Ok(rpc_tx) => {
                    let now = chrono::Utc::now();

                    // Best-effort persist into DB (fires DB triggers on INSERT/UPDATE)
                    let _ = sqlx::query(
                        r#"
                        INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids, created_at)
                        VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP)
                        ON CONFLICT (txid) DO UPDATE SET data = $3, status = $4, bitcoin_txids = $5
                        "#
                    )
                    .bind(&txid)
                    .bind(0i64)
                    .bind(&rpc_tx.runtime_transaction)
                    .bind(serde_json::to_value(&rpc_tx.status).unwrap_or(serde_json::json!({})))
                    .bind(rpc_tx.bitcoin_txids.as_deref())
                    .execute(&*pool)
                    .await;

                    // Synthesize a Transaction-like response so the UI can render immediately
                    let synthesized = Transaction {
                        txid,
                        block_height: 0, // unknown until fully indexed in a block
                        data: rpc_tx.runtime_transaction,
                        status: serde_json::to_value(&rpc_tx.status).unwrap_or(serde_json::json!({"type":"processed"})),
                        bitcoin_txids: rpc_tx.bitcoin_txids.clone(),
                        created_at: now,
                    };
                    Ok(Json(synthesized))
                }
                Err(_) => Err(ApiError::NotFound),
            }
        },
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
    // Try to extract logs from the runtime_transaction first
    if let Some(logs) = r.runtime_transaction.get("logs") {
        if let Some(logs_array) = logs.as_array() {
            return logs_array.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
    }
    
    // Fallback to the logs field if present
    if !r.logs.is_empty() {
        return r.logs.clone();
    }
    
    // Try to extract from status if it contains error information
    if let Some(status_obj) = r.status.as_object() {
        if let Some(error_msg) = status_obj.get("message").and_then(|v| v.as_str()) {
            return vec![format!("Error: {}", error_msg)];
        }
    }
    
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
    pub program_name: Option<String>,
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
        println!("program_b58: {}", program_b58);
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
        // BPF Loader (program deployment/management)
        if program_b58 == pid::BPF_LOADER_BASE58 {
            if data.len() >= 4 {
                let tag = u32_le(&data[0..4]).unwrap_or(9999);
                let rest = &data[4..];
                // 0: Write { offset: u64, bytes: Vec<u8> (len: u64, then bytes) }
                if tag == 0 {
                    let mut offset_val: Option<u64> = None;
                    let mut bytes_hex: Option<String> = None;
                    if rest.len() >= 8 {
                        let offset = u64_le(&rest[0..8]).unwrap_or(0);
                        offset_val = Some(offset);
                        if rest.len() >= 16 {
                            let len = u64_le(&rest[8..16]).unwrap_or(0) as usize;
                            if rest.len() >= 16 + len {
                                let slice = &rest[16..16+len];
                                bytes_hex = Some(hex::encode(slice));
                            }
                        }
                    }
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag},
                        "offset": offset_val,
                        "bytes_hex": bytes_hex,
                    });
                    return (Some("BPF Loader: Write".to_string()), Some(decoded));
                }
                // 1: Truncate { new_size: u64 }
                if tag == 1 {
                    let new_size = if rest.len() >= 8 { Some(u64_le(&rest[0..8]).unwrap_or(0)) } else { None };
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag},
                        "new_size": new_size
                    });
                    return (Some("BPF Loader: Truncate".to_string()), Some(decoded));
                }
                // 2: Deploy
                if tag == 2 {
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag}
                    });
                    return (Some("BPF Loader: Deploy".to_string()), Some(decoded));
                }
                // 3: Retract
                if tag == 3 {
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag}
                    });
                    return (Some("BPF Loader: Retract".to_string()), Some(decoded));
                }
                // 4: TransferAuthority
                if tag == 4 {
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag}
                    });
                    return (Some("BPF Loader: TransferAuthority".to_string()), Some(decoded));
                }
                // 5: Finalize
                if tag == 5 {
                    let decoded = json!({
                        "discriminator": {"type":"u32", "data": tag}
                    });
                    return (Some("BPF Loader: Finalize".to_string()), Some(decoded));
                }
            }
        }
        // Token Program (Arch APL Token)
        if program_b58 == pid::APL_TOKEN_PROGRAM_BASE58 {
            if !data.is_empty() {
                let tag = data[0];
                println!("tag: {}", tag);
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
                // 2: InitializeMultisig { m: u8 } ; accounts: [multisig, signer1..signerN]
                if tag == 2 && data.len() >= 2 {
                    let m = data[1];
                    let multisig = accounts.get(0).cloned();
                    let signers: Vec<String> = if accounts.len() > 1 { accounts[1..].to_vec() } else { Vec::new() };
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "m": {"type":"u8", "data": m},
                        "multisig": multisig,
                        "signers": signers,
                    });
                    return (Some("Token: InitializeMultisig".to_string()), Some(decoded));
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
                // 6: SetAuthority { authority_type: u8, new_authority: COption<Pubkey> }
                if tag == 6 && data.len() >= 2 {
                    let authority_type = data[1];
                    let mut idx = 2usize;
                    let mut new_authority_str: Option<String> = None;
                    if data.len() > idx {
                        let flag = data[idx];
                        idx += 1;
                        if flag == 1 && data.len() >= idx + 32 {
                            new_authority_str = Some(bs58::encode(&data[idx..idx+32]).into_string());
                        }
                    }
                    let owned = accounts.get(0).cloned();
                    let owner = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "authority_type": {"type":"u8", "data": authority_type},
                        "new_authority": new_authority_str,
                        "owned_account": owned,
                        "current_authority": owner,
                    });
                    return (Some("Token: SetAuthority".to_string()), Some(decoded));
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
                // 13: ApproveChecked { amount: u64, decimals: u8 }
                if tag == 13 && data.len() >= 1 + 8 + 1 {
                    let amount = u64_le(&data[1..9]).unwrap_or(0);
                    let decimals = data[9];
                    let source = accounts.get(0).cloned();
                    let mint = accounts.get(1).cloned();
                    let delegate = accounts.get(2).cloned();
                    let owner = accounts.get(3).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "amount": {"type":"u64", "data": amount},
                        "decimals": {"type":"u8", "data": decimals},
                        "source": source,
                        "mint": mint,
                        "delegate": delegate,
                        "owner": owner,
                    });
                    return (Some("Token: ApproveChecked".to_string()), Some(decoded));
                }
                // 14: MintToChecked { amount: u64, decimals: u8 }
                if tag == 14 && data.len() >= 1 + 8 + 1 {
                    let amount = u64_le(&data[1..9]).unwrap_or(0);
                    let decimals = data[9];
                    let mint = accounts.get(0).cloned();
                    let destination = accounts.get(1).cloned();
                    let authority = accounts.get(2).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "amount": {"type":"u64", "data": amount},
                        "decimals": {"type":"u8", "data": decimals},
                        "mint": mint,
                        "destination": destination,
                        "authority": authority,
                    });
                    return (Some("Token: MintToChecked".to_string()), Some(decoded));
                }
                // 15: BurnChecked { amount: u64, decimals: u8 }
                if tag == 15 && data.len() >= 1 + 8 + 1 {
                    let amount = u64_le(&data[1..9]).unwrap_or(0);
                    let decimals = data[9];
                    let account = accounts.get(0).cloned();
                    let mint = accounts.get(1).cloned();
                    let owner = accounts.get(2).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "amount": {"type":"u64", "data": amount},
                        "decimals": {"type":"u8", "data": decimals},
                        "account": account,
                        "mint": mint,
                        "owner": owner,
                    });
                    return (Some("Token: BurnChecked".to_string()), Some(decoded));
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
                // 16: InitializeAccount2 { owner: Pubkey }
                if tag == 16 && data.len() >= 1 + 32 {
                    let owner = bs58::encode(&data[1..33]).into_string();
                    let account = accounts.get(0).cloned();
                    let mint = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "owner": owner,
                        "account": account,
                        "mint": mint,
                    });
                    return (Some("Token: InitializeAccount2".to_string()), Some(decoded));
                }
                // 17: InitializeAccount3 { owner: Pubkey }
                if tag == 17 && data.len() >= 1 + 32 {
                    let owner = bs58::encode(&data[1..33]).into_string();
                    let account = accounts.get(0).cloned();
                    let mint = accounts.get(1).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "owner": owner,
                        "account": account,
                        "mint": mint,
                    });
                    return (Some("Token: InitializeAccount3".to_string()), Some(decoded));
                }
                // 18: InitializeMint2 { decimals, mint_authority, freeze_authority: COption<Pubkey> }
                if tag == 18 && data.len() >= 1 + 1 + 32 + 1 {
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
                    return (Some("Token: InitializeMint2".to_string()), Some(serde_json::Value::Object(obj)));
                }
                // 19: GetAccountDataSize (no data)
                if tag == 19 {
                    let mint = accounts.get(0).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "mint": mint,
                    });
                    return (Some("Token: GetAccountDataSize".to_string()), Some(decoded));
                }
                // 20: InitializeImmutableOwner (no data)
                if tag == 20 {
                    let account = accounts.get(0).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "account": account,
                    });
                    return (Some("Token: InitializeImmutableOwner".to_string()), Some(decoded));
                }
                // 21: AmountToUiAmount { amount: u64 }
                if tag == 21 && data.len() >= 1 + 8 {
                    let amount = u64_le(&data[1..9]).unwrap_or(0);
                    let mint = accounts.get(0).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "amount": {"type":"u64", "data": amount},
                        "mint": mint,
                    });
                    return (Some("Token: AmountToUiAmount".to_string()), Some(decoded));
                }
                // 22: UiAmountToAmount { ui_amount: string }
                if tag == 22 && data.len() >= 1 {
                    let ui_amount = match std::str::from_utf8(&data[1..]) { Ok(s) => s.to_string(), Err(_) => String::new() };
                    let mint = accounts.get(0).cloned();
                    let decoded = json!({
                        "discriminator": {"type":"u8", "data": tag},
                        "ui_amount": ui_amount,
                        "mint": mint,
                    });
                    return (Some("Token: UiAmountToAmount".to_string()), Some(decoded));
                }
            }
        }
        // Associated Token Account Program (Arch). Instruction has no data
        if program_b58 == pid::APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_BASE58 && data.is_empty() {
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
        let program_name = super::program_ids::get_program_name(&program_b58).map(|s| s.to_string());
        let acc_idx: Vec<usize> = ins.get("accounts").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as usize)).collect()).unwrap_or_default();
        let accounts: Vec<String> = acc_idx.into_iter().filter_map(|i| keys.get(i).map(|k| key_to_base58(k))).collect();
        let data_vec: Vec<u8> = ins.get("data").and_then(|d| d.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as u8)).collect()).unwrap_or_default();
        let data_len = data_vec.len();
        let data_hex = hex::encode(&data_vec);
        let (action, decoded) = decode_instruction(&program_b58, &program_hex, &data_vec, &accounts);
        println!("action: {:?}", action);
        out.push(InstructionRow { index: idx, program_id_hex: program_hex, program_id_base58: program_b58, program_name, accounts, data_len, action, decoded, data_hex });
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

    let network_total_blocks = node_tip.saturating_add(1);
    let indexed_height = stats.max_height.unwrap_or(0);
    let indexed_blocks = indexed_height.saturating_add(1);

    // Compute missing blocks (gaps) quickly: network_total_blocks - COUNT(blocks)
    let missing_count: i64 = network_total_blocks.saturating_sub(stats.total_blocks.unwrap_or(0).max(0));

    let response = NetworkStats {
        total_transactions: stats.total_tx.unwrap_or(0),
        total_blocks: stats.total_blocks.unwrap_or(0),
        indexed_height,
        indexed_blocks,
        network_total_blocks,
        latest_block_height: node_tip,
        block_height: node_tip,
        slot_height: node_tip,
        current_tps,
        average_tps,
        peak_tps,
        daily_transactions: stats.daily_tx.unwrap_or(0),
        // new field (serde will ignore unknown on clients not using it)
        missing_blocks: missing_count.max(0),
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
                created_at::timestamptz as "created_at!: chrono::DateTime<chrono::Utc>"
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
            let ts_utc = r.get::<chrono::DateTime<Utc>, _>("timestamp");
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
                    created_at::timestamptz as created_at
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
                    created_at: row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
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
                let ts_utc = r.get::<chrono::DateTime<Utc>, _>("timestamp");
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
                        created_at: row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
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
            t.created_at::timestamptz as "created_at!: chrono::DateTime<chrono::Utc>"
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
    let programs = sqlx::query_as::<_, ProgramStats>(
        r#"
        SELECT 
            program_id,
            transaction_count,
            first_seen_at,
            last_seen_at
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
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
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
        created_at: base.and_then(|r| r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("created_at").ok()).flatten(),
    };

    Ok(Json(response))
}

pub async fn get_program_details(
    State(pool): State<Arc<PgPool>>,
    Path(program_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Normalize input to our canonical hex storage
    // Prefer mapping well-known base58 IDs to their canonical ASCII-label hex (e.g., AplToken111...)
    let pid_hex = if program_id == super::program_ids::APL_TOKEN_PROGRAM_BASE58 {
        hex::encode(super::program_ids::APL_TOKEN_PROGRAM.as_bytes())
    } else if program_id == super::program_ids::APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_BASE58 {
        hex::encode(super::program_ids::APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM.as_bytes())
    } else if program_id == super::program_ids::APL_TOKEN_PROGRAM {
        hex::encode(super::program_ids::APL_TOKEN_PROGRAM.as_bytes())
    } else if program_id == super::program_ids::APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM {
        hex::encode(super::program_ids::APL_ASSOCIATED_TOKEN_ACCOUNT_PROGRAM.as_bytes())
    } else if program_id == super::program_ids::SYSTEM_PROGRAM {
        hex::encode(super::program_ids::SYSTEM_PROGRAM.as_bytes())
    } else if program_id == super::program_ids::VOTE_PROGRAM {
        hex::encode(super::program_ids::VOTE_PROGRAM.as_bytes())
    } else if program_id == super::program_ids::STAKE_PROGRAM {
        hex::encode(super::program_ids::STAKE_PROGRAM.as_bytes())
    } else if program_id == super::program_ids::BPF_LOADER {
        hex::encode(super::program_ids::BPF_LOADER.as_bytes())
    } else if program_id == super::program_ids::NATIVE_LOADER {
        hex::encode(super::program_ids::NATIVE_LOADER.as_bytes())
    } else if program_id == super::program_ids::COMPUTE_BUDGET {
        hex::encode(super::program_ids::COMPUTE_BUDGET.as_bytes())
    } else if program_id == super::program_ids::SOL_LOADER {
        hex::encode(super::program_ids::SOL_LOADER.as_bytes())
    } else if program_id == super::program_ids::SOL_COMPUTE_BUDGET {
        hex::encode(super::program_ids::SOL_COMPUTE_BUDGET.as_bytes())
    } else if program_id == super::program_ids::SOL_MEMO {
        hex::encode(super::program_ids::SOL_MEMO.as_bytes())
    } else if program_id == super::program_ids::SOL_SPL_TOKEN {
        hex::encode(super::program_ids::SOL_SPL_TOKEN.as_bytes())
    } else if program_id == super::program_ids::SOL_ASSOCIATED_TOKEN_ACCOUNT {
        hex::encode(super::program_ids::SOL_ASSOCIATED_TOKEN_ACCOUNT.as_bytes())
    } else if program_id.len() == 64 && program_id.chars().all(|c| c.is_ascii_hexdigit()) {
        // Treat as hex only for exact 32-byte (64-char) hex strings
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
                    "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
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
        // Fallback: derive from transactions if not present in programs table
        let row = sqlx::query(
            r#"
            WITH acc_txs AS (
                SELECT t.created_at,
                       t.data->'message'->'account_keys' AS keys,
                       ins
                FROM transactions t,
                LATERAL jsonb_array_elements(t.data->'message'->'instructions') ins
            ),
            progs AS (
                SELECT 
                    CASE 
                        WHEN ins ? 'program_id' THEN normalize_program_id(ins->>'program_id')
                        WHEN ins ? 'program_id_index' THEN normalize_program_id(
                            CASE 
                                WHEN jsonb_typeof(keys -> ((ins->>'program_id_index')::int)) = 'string' THEN trim(both '"' from (keys -> ((ins->>'program_id_index')::int))::text)
                                ELSE (keys -> ((ins->>'program_id_index')::int))::text
                            END
                        )
                        ELSE NULL 
                    END AS program_id,
                    created_at
                FROM acc_txs
            )
            SELECT 
                COUNT(*)::bigint AS tx_count,
                MIN(created_at) AS first_seen,
                MAX(created_at) AS last_seen
            FROM progs
            WHERE program_id IS NOT NULL AND program_id ILIKE $1
            "#
        )
        .bind(&pid_hex)
        .fetch_one(&*pool)
        .await
        .map_err(ApiError::Database)?;

        let tx_count: i64 = row.get::<i64, _>("tx_count");

        // For program IDs that are known/canonical but have no transactions yet,
        // return a zero-count payload instead of a 404 so the UI can render.
        let first_seen: Option<DateTime<Utc>> = row.try_get::<Option<DateTime<Utc>>, _>("first_seen").ok().flatten();
        let last_seen: Option<DateTime<Utc>> = row.try_get::<Option<DateTime<Utc>>, _>("last_seen").ok().flatten();
        let b58 = try_hex_to_base58(&pid_hex);
        let display_name = fallback_program_name_from_b58(&b58).or_else(|| fallback_program_name_from_hex(&pid_hex));

        let empty_recent: Vec<serde_json::Value> = Vec::new();
        let payload = json!({
            "program": {
                "program_id": pid_hex,
                "program_id_hex": pid_hex,
                "program_id_base58": if b58.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(b58) },
                "display_name": display_name,
                "transaction_count": tx_count,
                "first_seen_at": first_seen,
                "last_seen_at": last_seen
            },
            "recent_transactions": empty_recent
        });
        Ok(Json(payload))
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

// ---- helper utilities for v2 account transactions ----
fn v2_u32_le(bytes: &[u8]) -> Option<u32> { if bytes.len() >= 4 { Some(u32::from_le_bytes([bytes[0],bytes[1],bytes[2],bytes[3]])) } else { None } }
fn v2_u64_le(bytes: &[u8]) -> Option<u64> { if bytes.len() >= 8 { Some(u64::from_le_bytes([bytes[0],bytes[1],bytes[2],bytes[3],bytes[4],bytes[5],bytes[6],bytes[7]])) } else { None } }

fn v2_is_system_b58(s: &str) -> bool {
    if s.is_empty() { return false; }
    if s.chars().all(|c| c == '1') { return true; }
    let ones = "111111111111111111111111111111";
    s.starts_with(ones) && s.ends_with('2')
}

fn compute_native_balance_delta_for_address_in_tx(data: &serde_json::Value, address_hex: &str) -> Option<i128> {
    let message = data.get("message")?;
    let keys = message.get("account_keys")?.as_array()?;
    let instructions = message.get("instructions")?.as_array()?;
    let address_b58 = try_hex_to_base58(address_hex);
    let keys_b58: Vec<String> = keys.iter().map(|k| key_to_base58(k)).collect();
    let mut delta: i128 = 0;
    for ins in instructions {
        let program_val = if let Some(v) = ins.get("program_id") { Some(v.clone()) } else if let Some(ix) = ins.get("program_id_index").and_then(|v| v.as_i64()) { keys.get(ix as usize).cloned() } else { None };
        let program_hex = program_val.as_ref().map(|v| key_to_hex(v)).unwrap_or_default();
        let program_b58 = program_val.as_ref().map(|v| key_to_base58(v)).unwrap_or_default();
        let is_system = v2_is_system_b58(&program_b58)
            || program_hex.eq_ignore_ascii_case("0000000000000000000000000000000000000000000000000000000000000000")
            || program_hex.eq_ignore_ascii_case("0000000000000000000000000000000000000000000000000000000000000001");
        if !is_system { continue; }
        let acc_idx: Vec<usize> = ins.get("accounts").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as usize)).collect()).unwrap_or_default();
        let accounts_b58: Vec<String> = acc_idx.iter().filter_map(|i| keys_b58.get(*i)).cloned().collect();
        let data_vec: Vec<u8> = ins.get("data").and_then(|d| d.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as u8)).collect()).unwrap_or_default();
        if data_vec.len() < 4 || accounts_b58.len() < 2 { continue; }
        let tag = v2_u32_le(&data_vec[0..4]).unwrap_or(9999);
        if tag == 0 || tag == 2 || tag == 4 || data_vec.len() == 12 {
            let lamports = v2_u64_le(&data_vec[4..12]).unwrap_or(0) as i128;
            let src = accounts_b58.get(0);
            let dst = accounts_b58.get(1);
            if let Some(d) = dst { if d == &address_b58 { delta += lamports; } }
            if let Some(s) = src { if s == &address_b58 { delta -= lamports; } }
        }
    }
    Some(delta)
}

fn build_instruction_summaries_from_tx(data: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let message = match data.get("message") { Some(m) => m, None => return out };
    let keys = match message.get("account_keys").and_then(|a| a.as_array()) { Some(k) => k, None => return out };
    let instructions = match message.get("instructions").and_then(|a| a.as_array()) { Some(i) => i, None => return out };
    let key_b58 = |k: &serde_json::Value| -> String { key_to_base58(k) };
    for ins in instructions {
        let program_val = if let Some(v) = ins.get("program_id") { Some(v.clone()) } else if let Some(ix) = ins.get("program_id_index").and_then(|v| v.as_i64()) { keys.get(ix as usize).cloned() } else { None };
        let program_b58 = program_val.as_ref().map(|v| key_to_base58(v)).unwrap_or_default();
        let acc_idx: Vec<usize> = ins.get("accounts").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as usize)).collect()).unwrap_or_default();
        let accounts: Vec<String> = acc_idx.into_iter().filter_map(|i| keys.get(i).map(|k| key_b58(k))).collect();
        let data_vec: Vec<u8> = ins.get("data").and_then(|d| d.as_array()).map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as u8)).collect()).unwrap_or_default();

        // Compute Budget unit price
        if program_b58 == "ComputeBudget111111111111111111111111111111" {
            if let Some((&tag, rest)) = data_vec.split_first() { if tag == 3 && rest.len() >= 8 {
                let price = v2_u64_le(rest).unwrap_or(0);
                out.push(json!({"action": "Compute Budget: SetComputeUnitPrice", "decoded": {"price_micro_lamports": {"type": "u64", "data": price}} }));
                continue;
            }}
        }
        // Token transfers
        if program_b58 == pid::APL_TOKEN_PROGRAM_BASE58 {
            if !data_vec.is_empty() {
                let tag = data_vec[0];
                if tag == 3 && data_vec.len() >= 9 {
                    let amount = v2_u64_le(&data_vec[1..9]).unwrap_or(0);
                    let source = accounts.get(0).cloned();
                    let destination = accounts.get(1).cloned();
                    let authority = accounts.get(2).cloned();
                    out.push(json!({"action":"Token: Transfer","decoded": {"type":"transfer","amount": amount, "from": source, "to": destination, "authority": authority}}));
                    continue;
                }
                if tag == 12 && data_vec.len() >= 10 {
                    let amount = v2_u64_le(&data_vec[1..9]).unwrap_or(0);
                    let decimals = data_vec[9] as u64;
                    out.push(json!({"action":"Token: TransferChecked","decoded": {"amount": amount, "decimals": decimals}}));
                    continue;
                }
            }
        }
        // System transfer summary
        if v2_is_system_b58(&program_b58) && data_vec.len() >= 12 {
            let tag = v2_u32_le(&data_vec[0..4]).unwrap_or(9999);
            if tag == 2 || tag == 4 {
                let lamports = v2_u64_le(&data_vec[4..12]).unwrap_or(0);
                let src = accounts.get(0).cloned();
                let dst = accounts.get(1).cloned();
                out.push(json!({"action":"System: Transfer","decoded": {"lamports": {"type":"u64","data": lamports}, "source": src, "destination": dst}}));
                continue;
            }
        }
        // Fallback chip: program name
        let label = pid::get_program_name(&program_b58).unwrap_or("Program").to_string();
        out.push(json!({"action": label }));
    }
    out
}
