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
        arch_rpc
    );
    
    let pool_arc = Arc::new(pool);
    let app = create_router(pool_arc.clone(), Arc::new(processor));
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
    
    let body = read_body::<Vec<u8>>(response).await;
    let json: Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(
        json,
        serde_json::json!({"message": "Arch Indexer API is running"})
    );

    cleanup_test_db(&pool).await?;
    Ok(())
}

#[tokio::test]
async fn test_get_network_stats() -> Result<(), anyhow::Error> {
    let (app, pool) = create_test_app().await?;
    
    // Insert test block
    sqlx::query(
        "INSERT INTO blocks (height, hash, timestamp, bitcoin_block_height)
         VALUES ($1, $2, $3, $4)"
    )
    .bind(1_i64)
    .bind("test_hash")
    .bind(chrono::DateTime::from_timestamp(1234567890, 0).unwrap())
    .bind(100_i64)
    .execute(&*pool)
    .await?;

    // Insert test transaction
    sqlx::query(
        "INSERT INTO transactions (txid, block_height, data, status)
         VALUES ($1, $2, $3, $4)"
    )
    .bind("test_txid")
    .bind(1_i64)
    .bind(serde_json::json!({"test": "data"}))
    .bind(0_i32)
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
    
    let body = read_body::<Vec<u8>>(response).await;
    let stats: NetworkStats = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(stats.block_height, 1);
    assert_eq!(stats.total_transactions, 1);

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
    serde_json::from_slice(&bytes).unwrap()
}