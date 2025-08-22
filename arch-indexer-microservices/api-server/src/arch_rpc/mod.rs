use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use tracing::{error, info, warn};
use tokio::time::sleep;

pub mod websocket;
pub use websocket::{WebSocketClient, WebSocketEvent};

#[derive(Debug, Clone)]
pub struct ArchRpcClient {
    client: Client,
    url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Block {
    pub hash: String,
    pub height: i64,
    pub timestamp: i64,
    pub bitcoin_block_height: Option<i64>,
    pub transactions: Vec<String>,
    pub transaction_count: i64,
    pub previous_block_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BlockResponse {
    pub bitcoin_block_height: Option<i64>,
    pub block_height: i64,
    pub previous_block_hash: Vec<u8>, // Raw bytes array
    pub timestamp: i64,
    pub transactions: Vec<serde_json::Value>, // Flexible transaction format
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProcessedTransaction {
    pub runtime_transaction: serde_json::Value,
    pub status: serde_json::Value,
    pub bitcoin_txids: Option<Vec<String>>,
    #[serde(default)]
    pub accounts_tags: Vec<serde_json::Value>,
}

impl ArchRpcClient {
    pub fn new(_url: String) -> Self {
        // Create a client with optimized settings for high-throughput indexing
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(100)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .build()
            .unwrap_or_else(|_| Client::new());
        
        // Temporarily hardcode the beta network URL for testing
        let _url = "http://44.196.173.35:8081".to_string();
        info!("Initialized Arch RPC client for: {}", _url);
        Self { client, url: _url }
    }

    pub async fn is_node_ready(&self) -> Result<bool> {
        info!("Checking if Arch node is ready at: {}", self.url);
        
        let response = self.client
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "is_node_ready",
                "params": [],
                "id": 1
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            warn!("Node not ready, status: {}", response.status());
            return Ok(false);
        }

        let json_response = response.json::<serde_json::Value>().await?;
        let result = json_response["result"].as_bool().unwrap_or(false);
        
        info!("Node ready status: {}", result);
        Ok(result)
    }

    pub async fn get_block_count(&self) -> Result<i64> {
        let response = self.client
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "get_block_count",
                "params": [],
                "id": 1
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", response.status()));
        }

        let json_response = response.json::<serde_json::Value>().await?;
        
        if let Some(error) = json_response.get("error") {
            return Err(anyhow::anyhow!("RPC error: {:?}", error));
        }

        let result = json_response["result"].as_i64().unwrap_or(0);
        Ok(result)
    }

    pub async fn get_block_hash(&self, height: i64) -> Result<String> {
        let mut attempts = 0;
        let max_attempts = 5;
        let base_delay = Duration::from_millis(100);

        while attempts < max_attempts {
            match self.client
                .post(&self.url)
                .json(&json!({
                    "jsonrpc": "2.0",
                    "method": "get_block_hash",
                    "params": height,
                    "id": 1
                }))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        warn!("HTTP error for height {}: {}", height, response.status());
                        attempts += 1;
                        if attempts < max_attempts {
                            sleep(base_delay * attempts as u32).await;
                            continue;
                        }
                        return Err(anyhow::anyhow!("HTTP error after {} attempts", max_attempts));
                    }

                    match response.json::<serde_json::Value>().await {
                        Ok(json_response) => {
                            if let Some(error) = json_response.get("error") {
                                warn!("RPC error for height {}: {:?}", height, error);
                                attempts += 1;
                                if attempts < max_attempts {
                                    sleep(base_delay * attempts as u32).await;
                                    continue;
                                }
                                return Err(anyhow::anyhow!("RPC error for height {}: {:?}", height, error));
                            }

                            let result = json_response.get("result");
                            match result {
                                Some(result) => {
                                    if let Some(hash) = result.as_str() {
                                        return Ok(hash.to_string());
                                    } else {
                                        warn!("Unexpected result type for height {}: {:?}", height, result);
                                        attempts += 1;
                                        if attempts < max_attempts {
                                            sleep(base_delay * attempts as u32).await;
                                            continue;
                                        }
                                        return Err(anyhow::anyhow!("Invalid result type for height {}", height));
                                    }
                                },
                                None => {
                                    warn!("No result in response for height {}: {:?}", height, json_response);
                                    attempts += 1;
                                    if attempts < max_attempts {
                                        sleep(base_delay * attempts as u32).await;
                                        continue;
                                    }
                                    return Err(anyhow::anyhow!("No result in response for height {}", height));
                                }
                            }
                        },
                        Err(e) => {
                            error!("JSON decode error for height {}: {}", height, e);
                            attempts += 1;
                            if attempts < max_attempts {
                                sleep(base_delay * attempts as u32).await;
                                continue;
                            }
                            return Err(anyhow::anyhow!("JSON decode error for height {}: {}", height, e));
                        }
                    }
                },
                Err(e) => {
                    error!("Request error for height {}: {}", height, e);
                    attempts += 1;
                    if attempts < max_attempts {
                        sleep(base_delay * attempts as u32).await;
                        continue;
                    }
                    return Err(anyhow::anyhow!("Request error for height {}: {}", height, e));
                }
            }
        }

        Err(anyhow::anyhow!("Failed to get block hash for height {} after {} attempts", height, max_attempts))
    }

    pub async fn get_block(&self, hash: &str, height: i64) -> Result<Block> {
        let mut attempts = 0;
        let max_attempts = 5;
        let base_delay = Duration::from_millis(200);

        while attempts < max_attempts {
            match self.client
                .post(&self.url)
                .json(&json!({
                    "jsonrpc": "2.0",
                    "method": "get_block",
                    "params": [hash],
                    "id": 1
                }))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        warn!("HTTP error for block {}: {}", hash, response.status());
                        attempts += 1;
                        if attempts < max_attempts {
                            sleep(base_delay * attempts as u32).await;
                            continue;
                        }
                        return Err(anyhow::anyhow!("HTTP error for block {}: {}", hash, response.status()));
                    }

                    match response.json::<serde_json::Value>().await {
                        Ok(json_response) => {
                            if let Some(error) = json_response.get("error") {
                                error!("RPC error for block {}: {:?}", hash, error);
                                attempts += 1;
                                if attempts < max_attempts {
                                    sleep(base_delay * attempts as u32).await;
                                    continue;
                                }
                                return Err(anyhow::anyhow!("RPC error for block {}: {:?}", hash, error));
                            }

                            // Debug: Log the actual response structure
                            info!("🔍 Raw RPC response for block {}: {:?}", hash, json_response["result"]);
                            
                            match serde_json::from_value::<BlockResponse>(json_response["result"].clone()) {
                                Ok(block_response) => {
                                    // Convert raw bytes to hex string for previous_block_hash
                                    let previous_hash = hex::encode(&block_response.previous_block_hash);
                                    
                                    // Convert transactions to string format
                                    let transaction_strings: Vec<String> = block_response.transactions
                                        .iter()
                                        .map(|tx| tx.to_string())
                                        .collect();
                                    
                                    return Ok(Block {
                                        height: block_response.block_height,
                                        hash: hash.to_string(),
                                        timestamp: block_response.timestamp,
                                        bitcoin_block_height: block_response.bitcoin_block_height,
                                        transactions: transaction_strings,
                                        transaction_count: block_response.transactions.len() as i64,
                                        previous_block_hash: Some(previous_hash),
                                    });
                                },
                                Err(e) => {
                                    error!("Block deserialization error for {}: {}", hash, e);
                                    // Log the raw response for debugging
                                    error!("🔍 Raw response that failed to deserialize: {:?}", json_response["result"]);
                                    attempts += 1;
                                    if attempts < max_attempts {
                                        sleep(base_delay * attempts as u32).await;
                                        continue;
                                    }
                                    return Err(anyhow::anyhow!("Block deserialization error for {}: {}", hash, e));
                                }
                            }
                        },
                        Err(e) => {
                            error!("JSON decode error for block {}: {}", hash, e);
                            attempts += 1;
                            if attempts < max_attempts {
                                sleep(base_delay * attempts as u32).await;
                                continue;
                            }
                            return Err(anyhow::anyhow!("JSON decode error for block {}: {}", hash, e));
                        }
                    }
                },
                Err(e) => {
                    error!("Request error for block {}: {}", hash, e);
                    attempts += 1;
                    if attempts < max_attempts {
                        sleep(base_delay * attempts as u32).await;
                        continue;
                    }
                    return Err(anyhow::anyhow!("Request error for block {}: {}", hash, e));
                }
            }
        }

        Err(anyhow::anyhow!("Failed to get block after {} attempts", max_attempts))
    }

    pub async fn get_mempool_txids(&self) -> Result<Vec<String>> {
        let response = self.client
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "get_mempool_txids",
                "params": [],
                "id": 1
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", response.status()));
        }

        let json_response = response.json::<serde_json::Value>().await?;
        
        if let Some(error) = json_response.get("error") {
            return Err(anyhow::anyhow!("RPC error: {:?}", error));
        }

        let result = json_response["result"].as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid response format"))?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        Ok(result)
    }

    pub async fn get_mempool_entry(&self, txid: &str) -> Result<Option<serde_json::Value>> {
        let response = self.client
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "get_mempool_entry",
                "params": [txid],
                "id": 1
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", response.status()));
        }

        let json_response = response.json::<serde_json::Value>().await?;
        
        if let Some(error) = json_response.get("error") {
            return Err(anyhow::anyhow!("RPC error: {:?}", error));
        }

        let result = json_response["result"].as_object().map(|obj| serde_json::Value::Object(obj.clone()));
        Ok(result)
    }

    pub async fn get_processed_transaction(&self, txid: &str) -> Result<ProcessedTransaction> {
        let mut attempts = 0;
        let max_attempts = 3;
        let base_delay = Duration::from_millis(100);

        while attempts < max_attempts {
            match self.client
                .post(&self.url)
                .json(&json!({
                    "jsonrpc": "2.0",
                    "method": "get_processed_transaction",
                    "params": txid,
                    "id": 1
                }))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        warn!("HTTP error for tx {}: {}", txid, response.status());
                        attempts += 1;
                        if attempts < max_attempts {
                            sleep(base_delay * attempts as u32).await;
                            continue;
                        }
                        return Err(anyhow::anyhow!("HTTP error for tx {}: {}", txid, response.status()));
                    }

                    match response.json::<serde_json::Value>().await {
                        Ok(json_response) => {
                            if let Some(error) = json_response.get("error") {
                                error!("RPC error for tx {}: {:?}", txid, error);
                                attempts += 1;
                                if attempts < max_attempts {
                                    sleep(base_delay * attempts as u32).await;
                                    continue;
                                }
                                return Err(anyhow::anyhow!("RPC error for tx {}: {:?}", txid, error));
                            }

                            match serde_json::from_value::<ProcessedTransaction>(json_response["result"].clone()) {
                                Ok(tx) => return Ok(tx),
                                Err(e) => {
                                    error!("Transaction deserialization error for {}: {}", txid, e);
                                    attempts += 1;
                                    if attempts < max_attempts {
                                        sleep(base_delay * attempts as u32).await;
                                        continue;
                                    }
                                    return Err(anyhow::anyhow!("Transaction deserialization error for {}: {}", txid, e));
                                }
                            }
                        },
                        Err(e) => {
                            error!("JSON decode error for tx {}: {}", txid, e);
                            attempts += 1;
                            if attempts < max_attempts {
                                sleep(base_delay * attempts as u32).await;
                                continue;
                            }
                            return Err(anyhow::anyhow!("JSON decode error for tx {}: {}", txid, e));
                        }
                    }
                },
                Err(e) => {
                    error!("Request error for tx {}: {}", txid, e);
                    attempts += 1;
                    if attempts < max_attempts {
                        sleep(base_delay * attempts as u32).await;
                        continue;
                    }
                    return Err(anyhow::anyhow!("Request error for tx {}: {}", txid, e));
                }
            }
        }

        Err(anyhow::anyhow!("Failed to get transaction after {} attempts", max_attempts))
    }
}