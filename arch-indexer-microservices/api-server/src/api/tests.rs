use super::*;
use crate::{
    api::test_helpers::{cleanup_test_db, setup_test_db}, 
    indexer::BlockProcessor,
    arch_rpc::ArchRpcClient
};
use axum::{
    body::Body, http::{Request, StatusCode}, response::Response, Router
};
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;
use redis::Client as RedisClient;

async fn create_test_app() -> Result<(Router, Arc<PgPool>), anyhow::Error> {
    let pool = setup_test_db().await?;
    
    // Create Redis client
    let redis_client = RedisClient::open("redis://127.0.0.1/")?;
    
    // Create Arch RPC client
    let arch_rpc = ArchRpcClient::new("http://localhost:8080".to_owned());
    
    let processor = BlockProcessor::new(
        pool.clone(),
        redis_client,
        Arc::new(arch_rpc)
    );
    
    let pool_arc = Arc::new(pool);
    let app = create_router(pool_arc.clone());
    Ok((app, pool_arc))
}

#[tokio::test]
async fn test_root_endpoint() -> Result<(), anyhow::Error> {
    let (app, pool) = create_test_app().await?;
    
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    
    // Use the existing read_body function but specify Value as the type
    let json: Value = read_body(response).await;
    
    assert_eq!(
        json,
        serde_json::json!({
            "message": "Arch Indexer API is running"
        })
    );

    cleanup_test_db(&pool).await?;
    Ok(())
}

#[tokio::test]
async fn test_get_network_stats() -> Result<(), anyhow::Error> {
    let (app, pool) = create_test_app().await?;
    
    // Insert test block
    sqlx::query!(
        "INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height)
         VALUES ($1, $2, $3, $4)",
        1_i64,
        "test_hash",
        chrono::DateTime::from_timestamp(1234567890, 0).unwrap(),
        100_i64
    )
    .execute(&*pool)
    .await?;

    // Insert test transaction
    sqlx::query!(
        "INSERT INTO transactions (txid, block_height, data, status, created_at)
         VALUES ($1, $2, $3, $4, $5)",
        "test_txid",
        1_i64,
        serde_json::json!({"test": "data"}),
        0_i32,
        chrono::DateTime::<chrono::Utc>::from_timestamp(1234567890, 0).unwrap()
    )
    .execute(&*pool)
    .await?;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/network-stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    
    let stats: NetworkStats = read_body(response).await;
    
    assert_eq!(stats.block_height, 1);
    assert_eq!(stats.total_transactions, 1);
    assert_eq!(stats.slot_height, 1);
    assert_eq!(stats.tps, 0.0);
    assert_eq!(stats.true_tps, 0.0);

    cleanup_test_db(&pool).await?;
    Ok(())
}

async fn read_body<T>(response: Response) -> T 
where 
    T: serde::de::DeserializeOwned,
{
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    println!("Response body: {:?}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}