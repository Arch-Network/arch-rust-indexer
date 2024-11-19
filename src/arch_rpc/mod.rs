use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone)]
pub struct ArchRpcClient {
    client: Client,
    url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Block {
    pub hash: String,
    pub height: i64,
    pub timestamp: i64,
    pub bitcoin_block_height: Option<i64>,
    pub transactions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BlockResponse {
    pub bitcoin_block_height: Option<i64>,
    pub merkle_root: String,
    pub previous_block_hash: String,
    pub timestamp: i64,
    pub transaction_count: i64,
    pub transactions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessedTransaction {
    pub runtime_transaction: serde_json::Value,
    pub status: String,
    pub bitcoin_txids: Option<Vec<String>>,
}

impl ArchRpcClient {
    pub fn new(url: String) -> Self {
        Self {
            client: Client::new(),
            url,
        }
    }

    pub async fn is_node_ready(&self) -> Result<bool> {
        tracing::info!("Checking if Arch node is ready");
        tracing::info!("URL: {}", self.url);
        let response = self.client
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "is_node_ready",
                "params": [],
                "id": 1
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        // tracing::info!("Response: {:?}", response);

        Ok(response["result"].as_bool().unwrap_or(false))
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
            .await?
            .json::<serde_json::Value>()
            .await?;

        Ok(response["result"].as_i64().unwrap_or(0))
    }

    pub async fn get_block_hash(&self, height: i64) -> Result<String> {
        let response = self.client
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "get_block_hash",
                "params": height,
                "id": 1
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        // tracing::info!("Response: {:?}", response);

        Ok(response["result"].as_str().unwrap_or("").to_string())
    }

    pub async fn get_block(&self, hash: &str, height: i64) -> Result<Block> {
        let response = self.client
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "get_block",
                "params": hash,
                "id": 1
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
    
        // tracing::info!("Response result: {:?}", response["result"]);
        
        // Deserialize into the intermediate struct
        let block_response: BlockResponse = serde_json::from_value(response["result"].clone())?;
        
        // Convert to the Block struct
        let block = Block {
            height: height,
            hash: hash.to_string(),
            timestamp: block_response.timestamp,
            bitcoin_block_height: block_response.bitcoin_block_height,
            transactions: block_response.transactions,
        };
    
        // tracing::info!("Block in get_block: {:?}", block);
        Ok(block)
    }

    pub async fn get_processed_transaction(&self, txid: &str) -> Result<ProcessedTransaction> {
        let response = self.client
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "get_processed_transaction",
                "params": [txid],
                "id": 1
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let tx: ProcessedTransaction = serde_json::from_value(response["result"].clone())?;
        Ok(tx)
    }
}