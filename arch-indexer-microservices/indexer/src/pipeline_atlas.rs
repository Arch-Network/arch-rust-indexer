#![cfg(feature = "atlas_ingestion")]

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use atlas_core as core;
use core::datasource::{Datasource, DatasourceId, UpdateType, Updates};
use core::metrics::{Metrics, MetricsCollection};
use core::pipeline::{Pipeline, ShutdownStrategy};
use core::sync::{CheckpointStore, SyncConfig, SyncingDatasource, TipSource, BackfillSource, LiveSource};

use atlas_arch_rpc_datasource::{ArchBackfillDatasource, ArchDatasourceConfig, ArchLiveDatasource};
use atlas_rocksdb_checkpoint_store::RocksCheckpointStore;
use sqlx::{PgPool, QueryBuilder};
use tokio_postgres::{NoTls};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};
use tracing::info;

struct NoopMetrics;

#[async_trait]
impl Metrics for NoopMetrics {
    async fn initialize(&self) -> core::error::IndexerResult<()> { Ok(()) }
    async fn flush(&self) -> core::error::IndexerResult<()> { Ok(()) }
    async fn shutdown(&self) -> core::error::IndexerResult<()> { Ok(()) }
    async fn update_gauge(&self, _key: &str, _value: f64) -> core::error::IndexerResult<()> { Ok(()) }
    async fn increment_counter(&self, _key: &str, _n: u64) -> core::error::IndexerResult<()> { Ok(()) }
    async fn record_histogram(&self, _key: &str, _value: f64) -> core::error::IndexerResult<()> { Ok(()) }
}

struct NoopDatasource;

#[async_trait]
impl Datasource for NoopDatasource {
    async fn consume(
        &self,
        _id: DatasourceId,
        _sender: tokio::sync::mpsc::Sender<(Updates, DatasourceId)>,
        cancellation: CancellationToken,
        _metrics: Arc<MetricsCollection>,
    ) -> core::error::IndexerResult<()> {
        let _ = cancellation.cancelled().await;
        Ok(())
    }

    fn update_types(&self) -> Vec<UpdateType> { vec![] }
}

pub async fn run_minimal_pipeline() -> Result<()> {
    let mut pipeline: Pipeline = Pipeline::builder()
        .datasource(NoopDatasource)
        .metrics(Arc::new(NoopMetrics))
        .shutdown_strategy(ShutdownStrategy::Immediate)
        .build()?;

    pipeline.run().await?;
    Ok(())
}

/// Run full syncing pipeline using Atlas SyncingDatasource wired to Arch RPC + WS and RocksDB checkpoint.
pub async fn run_syncing_pipeline(rpc_url: &str, ws_url: &str, rocks_path: &str, db_pool: Arc<PgPool>) -> Result<()> {
    let checkpoint = RocksCheckpointStore::open(rocks_path)
        .map_err(|e| anyhow::anyhow!("open rocks checkpoint: {:?}", e))?;

    // Datasource implementations
    // Read tuning knobs from env with sensible defaults
    let max_concurrency = std::env::var("ARCH_MAX_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(64);
    let batch_emit_size = std::env::var("ARCH_BULK_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1000);
    let fetch_window_size = std::env::var("ARCH_FETCH_WINDOW_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4096);
    let initial_backoff_ms = std::env::var("ARCH_INITIAL_BACKOFF_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(50);
    let max_retries = std::env::var("ARCH_MAX_RETRIES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5);

    let ds_cfg = ArchDatasourceConfig {
        max_concurrency,
        batch_emit_size,
        fetch_window_size,
        initial_backoff_ms,
        max_retries,
    };
    let backfill = ArchBackfillDatasource::new(rpc_url, ds_cfg.clone());
    let live_id = core::datasource::DatasourceId::new_named("arch_live");
    let live = ArchLiveDatasource::new(ws_url, rpc_url, live_id);

    // TipSource comes from backfill datasource
    let tip: Arc<dyn TipSource> = Arc::new(backfill.clone());
    let backfill_src: Arc<dyn BackfillSource> = Arc::new(backfill);
    let live_src: Arc<dyn LiveSource> = Arc::new(live);
    let checkpoint_store: Arc<dyn CheckpointStore> = Arc::new(checkpoint);

    let update_types = vec![
        core::datasource::UpdateType::BlockDetails,
        core::datasource::UpdateType::Transaction,
        core::datasource::UpdateType::AccountUpdate,
        core::datasource::UpdateType::RolledbackTransactions,
        core::datasource::UpdateType::ReappliedTransactions,
    ];

    let sync_cfg = SyncConfig::default();
    let syncing_ds = SyncingDatasource::new(
        tip,
        checkpoint_store,
        backfill_src,
        live_src,
        update_types,
        sync_cfg,
    );

    // Transaction bridge processor wiring
    // Define a minimal instruction decoder collection (no-op)
    #[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
    struct EmptyCollection;
    impl core::collection::InstructionDecoderCollection for EmptyCollection {
        type InstructionType = ();
        fn parse_instruction(_instruction: &arch_program_atlas::instruction::Instruction) -> Option<core::instruction::DecodedInstruction<Self>> { None }
        fn get_type(&self) -> Self::InstructionType { () }
    }

    // Helper: open a raw tokio-postgres connection (COPY support)
    async fn open_copy_conn() -> anyhow::Result<tokio_postgres::Client> {
        let db_url = std::env::var("DATABASE_URL")?;
        let (client, connection) = tokio_postgres::connect(&db_url, NoTls).await?;
        tokio::spawn(async move { let _ = connection.await; });
        Ok(client)
    }

    let use_copy_bulk = std::env::var("ATLAS_USE_COPY_BULK").ok().as_deref() == Some("1");

    // Processor that writes transactions into the existing DB
    struct TransactionDbProcessor {
        pool: Arc<PgPool>,
    }

    #[async_trait::async_trait]
    impl core::processor::Processor for TransactionDbProcessor {
        type InputType = core::transaction::TransactionProcessorInputType<EmptyCollection, ()>;
        type OutputType = ();

        async fn process(
            &mut self,
            data: Vec<Self::InputType>,
            metrics: Arc<core::metrics::MetricsCollection>,
        ) -> core::error::IndexerResult<Self::OutputType> {
            let mut tx = self.pool.begin().await.map_err(|e| core::error::Error::Custom(format!("db begin: {}", e)))?;

            // Batch upsert transactions with one statement or COPY bulk path
            if !data.is_empty() && !std::env::var("ATLAS_USE_COPY_BULK").ok().as_deref().eq(&Some("1")) {
                let mut qb = QueryBuilder::<sqlx::Postgres>::new(
                    "INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids) VALUES ",
                );
                qb.push_values(data.iter(), |mut b, (meta, _parsed, _matched)| {
                    let status_json = serde_json::to_value(&meta.status)
                        .unwrap_or(serde_json::json!(null));
                    let data_json = serde_json::json!({
                        "id": meta.id,
                        "block_height": meta.block_height,
                        "message": meta.message,
                        "rollback_status": meta.rollback_status,
                    });
                    let bitcoin_txids: Vec<String> = meta
                        .bitcoin_txid
                        .as_ref()
                        .map(|s| vec![s.to_string()])
                        .unwrap_or_default();
                    b.push_bind(&meta.id)
                        .push_bind(meta.block_height as i64)
                        .push_bind(data_json)
                        .push_bind(status_json)
                        .push_bind(&bitcoin_txids[..]);
                });
                qb.push(" ON CONFLICT (txid) DO UPDATE SET block_height = EXCLUDED.block_height, data = EXCLUDED.data, status = EXCLUDED.status, bitcoin_txids = EXCLUDED.bitcoin_txids");
                if let Err(e) = qb.build().execute(&mut *tx).await {
                    let _ = metrics.increment_counter("tx_write_failed", 1).await;
                    return Err(core::error::Error::Custom(format!("tx upsert failed: {}", e)));
                } else {
                    let _ = metrics.increment_counter("tx_write_success", data.len() as u64).await;
                }
            } else if !data.is_empty() {
                // COPY into temp staging and upsert
                let client = open_copy_conn().await.map_err(|e| core::error::Error::Custom(format!("copy conn: {}", e)))?;
                client.batch_execute("CREATE TEMP TABLE IF NOT EXISTS tmp_transactions (txid text, block_height bigint, data jsonb, status jsonb, bitcoin_txids text[]) ON COMMIT DROP;").await.map_err(|e| core::error::Error::Custom(format!("tmp table: {}", e)))?;
                let sink = client.copy_in("COPY tmp_transactions (txid, block_height, data, status, bitcoin_txids) FROM STDIN BINARY").await.map_err(|e| core::error::Error::Custom(format!("copy in: {}", e)))?;
                let mut writer = tokio_postgres::binary_copy::BinaryCopyInWriter::new(sink, &[tokio_postgres::types::Type::TEXT, tokio_postgres::types::Type::INT8, tokio_postgres::types::Type::JSONB, tokio_postgres::types::Type::JSONB, tokio_postgres::types::Type::TEXT_ARRAY]);
                for (meta, _parsed, _matched) in &data {
                    let status_json: serde_json::Value = serde_json::to_value(&meta.status).unwrap_or(serde_json::json!(null));
                    let data_json: serde_json::Value = serde_json::json!({
                        "id": meta.id,
                        "block_height": meta.block_height,
                        "message": meta.message,
                        "rollback_status": meta.rollback_status,
                    });
                    let bitcoin_txids: Vec<String> = meta
                        .bitcoin_txid
                        .as_ref()
                        .map(|s| vec![s.to_string()])
                        .unwrap_or_default();
                    use tokio_postgres::types::ToSql;
                    let txid: &str = &meta.id;
                    let height_i64: i64 = meta.block_height as i64;
                    let json_data: &serde_json::Value = &data_json;
                    let json_status: &serde_json::Value = &status_json;
                    let txids_arr: &Vec<String> = &bitcoin_txids;
                    writer.write(&[&txid as &dyn ToSql, &height_i64, json_data, json_status, txids_arr]).await.map_err(|e| core::error::Error::Custom(format!("copy write: {}", e)))?;
                }
                writer.finish().await.map_err(|e| core::error::Error::Custom(format!("copy finish: {}", e)))?;
                client.batch_execute("INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids) SELECT txid, block_height, data, status, bitcoin_txids FROM tmp_transactions ON CONFLICT (txid) DO UPDATE SET block_height = EXCLUDED.block_height, data = EXCLUDED.data, status = EXCLUDED.status, bitcoin_txids = EXCLUDED.bitcoin_txids;").await.map_err(|e| core::error::Error::Custom(format!("copy upsert: {}", e)))?;
                let _ = metrics.increment_counter("tx_write_success", data.len() as u64).await;
            }

            tx.commit().await.map_err(|e| core::error::Error::Custom(format!("db commit: {}", e)))?;
            Ok(())
        }
    }

    let mut pipeline_builder = Pipeline::builder()
        .datasource(syncing_ds)
        .metrics(Arc::new(NoopMetrics))
        .shutdown_strategy(ShutdownStrategy::Immediate);

    // Register transaction bridge
    let tx_processor = TransactionDbProcessor { pool: db_pool.clone() };
    pipeline_builder = pipeline_builder.transaction::<EmptyCollection, ()>(tx_processor, None);

    // Set up a live tip-height poller for accurate rate/ETA reporting
    let tip_height: Arc<AtomicI64> = Arc::new(AtomicI64::new(0));
    {
        let rpc = crate::arch_rpc::ArchRpcClient::new(rpc_url.to_string());
        let tip_clone = tip_height.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(best) = rpc.get_block_count().await {
                    tip_clone.store(best as i64, Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });
    }

    // BlockDetails processor → blocks table
    struct BlockDetailsDbProcessor {
        pool: Arc<PgPool>,
        tip_height: Arc<AtomicI64>,
        last_report_instant: Instant,
        last_report_height: i64,
        ema_rate_hps: f64,
        start_height: Option<i64>,
        initial_tip_height: Option<i64>,
        start_instant: Instant,
        growth_samples: VecDeque<(Instant, i64)>,
    }

    #[async_trait::async_trait]
    impl core::processor::Processor for BlockDetailsDbProcessor {
        type InputType = core::datasource::BlockDetails;
        type OutputType = ();

        async fn process(
            &mut self,
            data: Vec<Self::InputType>,
            metrics: Arc<core::metrics::MetricsCollection>,
        ) -> core::error::IndexerResult<Self::OutputType> {
            let mut tx = self.pool.begin().await.map_err(|e| core::error::Error::Custom(format!("db begin: {}", e)))?;
            let mut max_height_in_batch: i64 = -1;
            let mut min_height_in_batch: i64 = i64::MAX;
            if !data.is_empty() && !use_copy_bulk {
                // Batch upsert blocks with one statement
                let mut qb = QueryBuilder::<sqlx::Postgres>::new(
                    "INSERT INTO blocks (height, hash, timestamp) VALUES ",
                );
                for b in &data {
                    let micros: i64 = b.block_time.unwrap_or(0);
                    let secs_f64: f64 = (micros as f64) / 1_000_000_f64;
                    let hash = b.block_hash.map(|h| format!("{:?}", h)).unwrap_or_default();
                    qb.push("(")
                        .push_bind(b.height as i64)
                        .push(", ")
                        .push_bind(hash)
                        .push(", to_timestamp(")
                        .push_bind(secs_f64)
                        .push(") )");
                    qb.separated(',');
                    let h_i64 = b.height as i64;
                    if h_i64 > max_height_in_batch { max_height_in_batch = h_i64; }
                    if h_i64 < min_height_in_batch { min_height_in_batch = h_i64; }
                }
                qb.push(" ON CONFLICT (height) DO UPDATE SET hash = EXCLUDED.hash, timestamp = EXCLUDED.timestamp");
                if let Err(e) = qb.build().execute(&mut *tx).await {
                    let _ = metrics.increment_counter("block_write_failed", 1).await;
                    return Err(core::error::Error::Custom(format!("block upsert failed: {}", e)));
                } else {
                    let _ = metrics.increment_counter("block_write_success", data.len() as u64).await;
                }
            } else if !data.is_empty() {
                // COPY bulk for blocks
                let client = open_copy_conn().await.map_err(|e| core::error::Error::Custom(format!("copy conn: {}", e)))?;
                client.batch_execute("CREATE TEMP TABLE IF NOT EXISTS tmp_blocks (height bigint, hash text, ts_seconds double precision) ON COMMIT DROP;").await.map_err(|e| core::error::Error::Custom(format!("tmp table: {}", e)))?;
                let sink = client.copy_in("COPY tmp_blocks (height, hash, ts_seconds) FROM STDIN BINARY").await.map_err(|e| core::error::Error::Custom(format!("copy in: {}", e)))?;
                let mut writer = tokio_postgres::binary_copy::BinaryCopyInWriter::new(sink, &[tokio_postgres::types::Type::INT8, tokio_postgres::types::Type::TEXT, tokio_postgres::types::Type::FLOAT8]);
                for b in &data {
                    let micros: i64 = b.block_time.unwrap_or(0);
                    let secs_f64: f64 = (micros as f64) / 1_000_000_f64;
                    let hash = b.block_hash.map(|h| format!("{:?}", h)).unwrap_or_default();
                    use tokio_postgres::types::ToSql;
                    let height_i64: i64 = b.height as i64;
                    writer.write(&[&height_i64 as &dyn ToSql, &hash, &secs_f64]).await.map_err(|e| core::error::Error::Custom(format!("copy write: {}", e)))?;
                    let h_i64 = b.height as i64;
                    if h_i64 > max_height_in_batch { max_height_in_batch = h_i64; }
                    if h_i64 < min_height_in_batch { min_height_in_batch = h_i64; }
                }
                writer.finish().await.map_err(|e| core::error::Error::Custom(format!("copy finish: {}", e)))?;
                client.batch_execute("INSERT INTO blocks (height, hash, timestamp) SELECT height, hash, to_timestamp(ts_seconds) FROM tmp_blocks ON CONFLICT (height) DO UPDATE SET hash = EXCLUDED.hash, timestamp = EXCLUDED.timestamp;").await.map_err(|e| core::error::Error::Custom(format!("copy upsert: {}", e)))?;
                let _ = metrics.increment_counter("block_write_success", data.len() as u64).await;
            }
            tx.commit().await.map_err(|e| core::error::Error::Custom(format!("db commit: {}", e)))?;

            // Accurate rate/ETA reporting using EMA over a sliding window
            if max_height_in_batch >= 0 {
                if self.start_height.is_none() { self.start_height = Some(min_height_in_batch); }
                let current_tip = self.tip_height.load(Ordering::Relaxed);
                if self.initial_tip_height.is_none() && current_tip > 0 { self.initial_tip_height = Some(current_tip); }
                let now = Instant::now();
                let elapsed = now.duration_since(self.last_report_instant);
                if elapsed >= Duration::from_secs(5) {
                    let delta_h = (max_height_in_batch - self.last_report_height).max(0) as f64;
                    let delta_s = elapsed.as_secs_f64().max(1e-6);
                    let inst_rate = delta_h / delta_s; // heights per second
                    // Time-based EMA ~60s window: alpha = 1 - exp(-dt/60)
                    let alpha = 1.0 - (-delta_s / 60.0).exp();
                    self.ema_rate_hps = if self.ema_rate_hps <= 0.0 { inst_rate } else { alpha * inst_rate + (1.0 - alpha) * self.ema_rate_hps };

                    let tip = current_tip;
                    // Remaining to a fixed goal: the initial safe tip captured at start
                    let remaining_to_initial = if let Some(init_tip) = self.initial_tip_height { (init_tip - max_height_in_batch).max(0) as f64 } else { 0.0 };
                    // Percent complete relative to fixed goal (avoids wobble as live tip advances)
                    let percent = if let (Some(start_h), Some(init_tip)) = (self.start_height, self.initial_tip_height) {
                        let denom = (init_tip - start_h) as f64;
                        if denom > 0.0 { (((max_height_in_batch - start_h) as f64) / denom * 100.0).clamp(0.0, 100.0) } else { 0.0 }
                    } else { 0.0 };
                    // Estimate live tip growth rate using a 5-minute sliding window
                    self.growth_samples.push_back((now, tip));
                    while let Some(&(t_old, _)) = self.growth_samples.front() {
                        if now.duration_since(t_old) > Duration::from_secs(300) { self.growth_samples.pop_front(); } else { break; }
                    }
                    let growth_rate_hps = if let (Some(&(t0, tip0)), Some(&(t1, tip1))) = (self.growth_samples.front(), self.growth_samples.back()) {
                        let dt = t1.duration_since(t0).as_secs_f64().max(1e-6);
                        ((tip1 - tip0) as f64 / dt).max(0.0)
                    } else { 0.0 };
                    // Effective processing rate toward fixed goal subtracts tip growth
                    let effective_rate = (self.ema_rate_hps - growth_rate_hps).max(0.001);
                    let eta_secs = remaining_to_initial / effective_rate;

                    fn fmt_hms(mut s: f64) -> String {
                        if !s.is_finite() { return "inf".to_string(); }
                        if s < 0.0 { s = 0.0; }
                        let total = s.round() as i64;
                        let h = total / 3600;
                        let m = (total % 3600) / 60;
                        let ss = total % 60;
                        format!("{:02}:{:02}:{:02}", h, m, ss)
                    }

                    info!(
                        target: "atlas_progress",
                        height = max_height_in_batch,
                        tip = tip,
                        rate_hps = self.ema_rate_hps,
                        growth_hps = growth_rate_hps,
                        effective_rate_hps = effective_rate,
                        remaining_fixed = remaining_to_initial,
                        percent = percent,
                        eta_secs = eta_secs,
                        "backfill progress: {:.2}% complete (eta ~ {})",
                        percent,
                        fmt_hms(eta_secs)
                    );

                    self.last_report_instant = now;
                    self.last_report_height = max_height_in_batch;
                }
            }
            Ok(())
        }
    }

    let block_proc = BlockDetailsDbProcessor {
        pool: db_pool,
        tip_height: tip_height.clone(),
        last_report_instant: Instant::now(),
        last_report_height: -1,
        ema_rate_hps: 0.0,
        start_height: None,
        initial_tip_height: None,
        start_instant: Instant::now(),
    };
    pipeline_builder = pipeline_builder.block_details(block_proc);

    // Accounts processor: upsert into accounts table
    struct RawAccountDecoder;
    impl<'a> core::account::AccountDecoder<'a> for RawAccountDecoder {
        type AccountType = Vec<u8>;
        fn decode_account(&self, account: &'a arch_sdk::AccountInfo) -> Option<core::account::DecodedAccount<Self::AccountType>> {
            Some(core::account::DecodedAccount {
                lamports: account.lamports,
                owner: account.owner,
                data: account.data.clone(),
                utxo: String::new(),
                executable: false,
            })
        }
    }

    struct AccountDbProcessor {
        pool: Arc<PgPool>,
    }

    #[async_trait::async_trait]
    impl core::processor::Processor for AccountDbProcessor {
        type InputType = core::account::AccountProcessorInputType<Vec<u8>>;
        type OutputType = ();

        async fn process(
            &mut self,
            data: Vec<Self::InputType>,
            _metrics: Arc<core::metrics::MetricsCollection>,
        ) -> core::error::IndexerResult<Self::OutputType> {
            if data.is_empty() { return Ok(()); }
            let mut qb = QueryBuilder::<sqlx::Postgres>::new(
                "INSERT INTO accounts (pubkey, lamports, owner, data, height) VALUES ",
            );
            qb.push_values(data.iter(), |mut b, (meta, decoded, _raw)| {
                b.push_bind(hex::encode(meta.pubkey))
                    .push_bind(decoded.lamports as i64)
                    .push_bind(format!("{:?}", decoded.owner))
                    .push_bind(&decoded.data)
                    .push_bind(meta.height as i64);
            });
            qb.push(" ON CONFLICT (pubkey) DO UPDATE SET lamports = EXCLUDED.lamports, owner = EXCLUDED.owner, data = EXCLUDED.data, height = EXCLUDED.height, updated_at = CURRENT_TIMESTAMP");
            qb.build().execute(&*self.pool).await.map_err(|e| core::error::Error::Custom(format!("accounts upsert: {}", e)))?;
            Ok(())
        }
    }

    struct AccountDeletionDbProcessor { pool: Arc<PgPool> }
    #[async_trait::async_trait]
    impl core::processor::Processor for AccountDeletionDbProcessor {
        type InputType = core::datasource::AccountDeletion;
        type OutputType = ();
        async fn process(
            &mut self,
            data: Vec<Self::InputType>,
            _metrics: Arc<core::metrics::MetricsCollection>,
        ) -> core::error::IndexerResult<Self::OutputType> {
            if data.is_empty() { return Ok(()); }
            let pubkeys: Vec<String> = data.into_iter().map(|d| hex::encode(d.pubkey)).collect();
            let query = "DELETE FROM accounts WHERE pubkey = ANY($1)";
            sqlx::query(query).bind(&pubkeys[..]).execute(&*self.pool).await.map_err(|e| core::error::Error::Custom(format!("accounts delete: {}", e)))?;
            Ok(())
        }
    }

    // Wire accounts
    let acct_decoder = RawAccountDecoder;
    let acct_proc = AccountDbProcessor { pool: db_pool.clone() };
    pipeline_builder = pipeline_builder.account(acct_decoder, acct_proc);
    let acct_del_proc = AccountDeletionDbProcessor { pool: db_pool.clone() };
    pipeline_builder = pipeline_builder.account_deletions(acct_del_proc);

    let mut pipeline: Pipeline = pipeline_builder.build()?;

    pipeline.run().await?;
    Ok(())
}
