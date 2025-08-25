use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use tracing::{error, info};
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
	pub previous_block_hash: Vec<u8>,
	pub timestamp: i64,
	pub transactions: Vec<serde_json::Value>,
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
	pub fn new(url: String) -> Self {
		let client = Client::builder()
			.danger_accept_invalid_certs(true)
			.timeout(Duration::from_secs(30))
			.pool_max_idle_per_host(100)
			.pool_idle_timeout(Duration::from_secs(90))
			.tcp_keepalive(Some(Duration::from_secs(60)))
			.build()
			.unwrap_or_else(|_| Client::new());

		info!("Initialized Arch RPC client for: {}", url);
		Self { client, url }
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
		Ok(json_response["result"].as_i64().unwrap_or(0))
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
						attempts += 1;
						if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
						return Err(anyhow::anyhow!("HTTP error after {} attempts", max_attempts));
					}
					let json_response = response.json::<serde_json::Value>().await?;
					if let Some(error) = json_response.get("error") {
						attempts += 1;
						if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
						return Err(anyhow::anyhow!("RPC error for height {}: {:?}", height, error));
					}
					if let Some(hash) = json_response.get("result").and_then(|r| r.as_str()) {
						return Ok(hash.to_string());
					}
					attempts += 1;
					if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
					return Err(anyhow::anyhow!("Invalid result type for height {}", height));
				},
				Err(e) => {
					attempts += 1;
					if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
					return Err(anyhow::anyhow!("Request error for height {}: {}", height, e));
				}
			}
		}
		Err(anyhow::anyhow!("Failed to get block hash after {} attempts", max_attempts))
	}

	pub async fn get_block(&self, hash: &str, _height: i64) -> Result<Block> {
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
						attempts += 1; if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
						return Err(anyhow::anyhow!("HTTP error for block {}: {}", hash, response.status()));
					}
					let json_response = response.json::<serde_json::Value>().await?;
					if let Some(error) = json_response.get("error") {
						attempts += 1; if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
						return Err(anyhow::anyhow!("RPC error for block {}: {:?}", hash, error));
					}
					match serde_json::from_value::<BlockResponse>(json_response["result"].clone()) {
						Ok(block_response) => {
							// Convert transactions to hex strings when provided as byte arrays
							let transaction_strings: Vec<String> = block_response
								.transactions
								.iter()
								.map(|tx| {
									if let Some(s) = tx.as_str() {
										s.to_string()
									} else if let Some(arr) = tx.as_array() {
										let bytes: Vec<u8> = arr
											.iter()
											.filter_map(|v| v.as_i64().map(|n| if n < 0 { (n + 256) as u8 } else { n as u8 }))
											.collect();
										hex::encode(bytes)
									} else {
										tx.to_string()
									}
								})
								.collect();
							return Ok(Block {
								height: block_response.block_height,
								hash: hash.to_string(),
								timestamp: block_response.timestamp,
								bitcoin_block_height: block_response.bitcoin_block_height,
								transactions: transaction_strings,
								transaction_count: block_response.transactions.len() as i64,
								previous_block_hash: Some(hex::encode(&block_response.previous_block_hash)),
							});
						},
						Err(e) => {
							attempts += 1; if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
							return Err(anyhow::anyhow!("Block deserialization error for {}: {}", hash, e));
						}
					}
				},
				Err(e) => {
					attempts += 1; if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
					return Err(anyhow::anyhow!("Request error for block {}: {}", hash, e));
				}
			}
		}
		Err(anyhow::anyhow!("Failed to get block after {} attempts", max_attempts))
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
						attempts += 1; if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
						return Err(anyhow::anyhow!("HTTP error for tx {}: {}", txid, response.status()));
					}
					let json_response = response.json::<serde_json::Value>().await?;
					if let Some(error) = json_response.get("error") {
						attempts += 1; if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
						return Err(anyhow::anyhow!("RPC error for tx {}: {:?}", txid, error));
					}
					match serde_json::from_value::<ProcessedTransaction>(json_response["result"].clone()) {
						Ok(ptx) => return Ok(ptx),
						Err(e) => {
							attempts += 1; if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
							return Err(anyhow::anyhow!("Transaction deserialization error for {}: {}", txid, e));
						}
					}
				},
				Err(e) => {
					attempts += 1; if attempts < max_attempts { sleep(base_delay * attempts as u32).await; continue; }
					return Err(anyhow::anyhow!("Request error for tx {}: {}", txid, e));
				}
			}
		}
		Err(anyhow::anyhow!("Failed to get transaction after {} attempts", max_attempts))
	}
}
