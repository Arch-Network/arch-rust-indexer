#![cfg(feature = "atlas_ingestion")]

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use atlas_arch as core;
use core::datasource::{Datasource, DatasourceId, UpdateType, Updates};
use core::metrics::{Metrics, MetricsCollection};
use core::pipeline::{Pipeline, ShutdownStrategy};
use core::sync::{CheckpointStore, SyncConfig, SyncingDatasource, TipSource, BackfillSource, LiveSource};

use atlas_arch_rpc_datasource::{ArchBackfillDatasource, ArchDatasourceConfig, ArchLiveDatasource};
use std::fs;
use std::path::PathBuf;
use sqlx::{PgPool, QueryBuilder};
use tokio_postgres::{NoTls};
use std::collections::{VecDeque, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};
use tracing::info;

struct PromMetrics;

#[async_trait]
impl Metrics for PromMetrics {
    async fn initialize(&self) -> core::error::IndexerResult<()> { Ok(()) }
    async fn flush(&self) -> core::error::IndexerResult<()> { Ok(()) }
    async fn shutdown(&self) -> core::error::IndexerResult<()> { Ok(()) }
    async fn update_gauge(&self, key: &str, value: f64) -> core::error::IndexerResult<()> {
        metrics::gauge!("atlas_gauge", value, "name" => key.to_owned());
        Ok(())
    }
    async fn increment_counter(&self, key: &str, n: u64) -> core::error::IndexerResult<()> {
        metrics::counter!("atlas_counter", n as u64, "name" => key.to_owned());
        Ok(())
    }
    async fn record_histogram(&self, key: &str, value: f64) -> core::error::IndexerResult<()> {
        metrics::histogram!("atlas_histogram", value, "name" => key.to_owned());
        Ok(())
    }
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
        .metrics(Arc::new(PromMetrics))
        .shutdown_strategy(ShutdownStrategy::Immediate)
        .build()?;

    pipeline.run().await?;
    Ok(())
}

/// Run full syncing pipeline using Atlas SyncingDatasource wired to Arch RPC + WS and RocksDB checkpoint.
pub async fn run_syncing_pipeline(rpc_url: &str, ws_url: &str, rocks_path: &str, db_pool: Arc<PgPool>) -> Result<()> {
    // Simple file-based checkpoint store compatible with atlas-arch::sync::CheckpointStore
    struct FileCheckpointStore { path: PathBuf }
    #[async_trait]
    impl core::sync::CheckpointStore for FileCheckpointStore {
        async fn last_indexed_height(&self) -> core::error::IndexerResult<u64> {
            match tokio::fs::read_to_string(&self.path).await {
                Ok(s) => s.trim().parse::<u64>().map_err(|e| core::error::Error::Custom(format!("parse checkpoint: {}", e))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
                Err(e) => Err(core::error::Error::Custom(format!("read checkpoint: {}", e))),
            }
        }
        async fn set_last_indexed_height(&self, height: u64) -> core::error::IndexerResult<()> {
            let tmp = self.path.with_extension("tmp");
            tokio::fs::write(&tmp, height.to_string())
                .await
                .map_err(|e| core::error::Error::Custom(format!("write tmp cp: {}", e)))?;
            tokio::fs::rename(&tmp, &self.path)
                .await
                .map_err(|e| core::error::Error::Custom(format!("persist cp: {}", e)))?
            ;
            Ok(())
        }
    }
    // Optional Postgres-backed checkpoint store
    struct PgCheckpointStore { pool: Arc<PgPool> }
    #[async_trait]
    impl core::sync::CheckpointStore for PgCheckpointStore {
        async fn last_indexed_height(&self) -> core::error::IndexerResult<u64> {
            let res: Option<i64> = sqlx::query_scalar("SELECT height FROM atlas_checkpoint WHERE id = $1")
                .bind("default")
                .fetch_optional(&*self.pool)
                .await
                .map_err(|e| core::error::Error::Custom(format!("pg cp select: {}", e)))?;
            Ok(res.unwrap_or(0) as u64)
        }
        async fn set_last_indexed_height(&self, height: u64) -> core::error::IndexerResult<()> {
            sqlx::query("INSERT INTO atlas_checkpoint (id, height) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET height = EXCLUDED.height, updated_at = now()")
                .bind("default")
                .bind(height as i64)
                .execute(&*self.pool)
                .await
                .map_err(|e| core::error::Error::Custom(format!("pg cp upsert: {}", e)))?;
            Ok(())
        }
    }

    // Choose checkpoint backend: postgres|file (default file)
    let backend = std::env::var("ATLAS_CHECKPOINT_BACKEND").unwrap_or_else(|_| "file".to_string());
    let checkpoint: Arc<dyn CheckpointStore> = if backend.eq_ignore_ascii_case("postgres") {
        // Ensure table exists
        sqlx::query("CREATE TABLE IF NOT EXISTS atlas_checkpoint (id TEXT PRIMARY KEY, height BIGINT NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now())")
            .execute(&*db_pool)
            .await
            .map_err(|e| anyhow::anyhow!("create atlas_checkpoint: {}", e))?;

        // Seed from file checkpoint if row missing and file exists
        let existing: Option<i64> = sqlx::query_scalar("SELECT height FROM atlas_checkpoint WHERE id = $1")
            .bind("default")
            .fetch_optional(&*db_pool)
            .await
            .unwrap_or(None);
        if existing.is_none() {
            let file_path = PathBuf::from(rocks_path).join("checkpoint.height");
            if let Ok(s) = tokio::fs::read_to_string(&file_path).await {
                if let Ok(h) = s.trim().parse::<u64>() {
                    let _ = sqlx::query("INSERT INTO atlas_checkpoint (id, height) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET height = EXCLUDED.height")
                        .bind("default")
                        .bind(h as i64)
                        .execute(&*db_pool)
                        .await;
                }
            }
        }

        Arc::new(PgCheckpointStore { pool: db_pool.clone() })
    } else {
        Arc::new(FileCheckpointStore { path: PathBuf::from(rocks_path).join("checkpoint.height") })
    };

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
    let checkpoint_store: Arc<dyn CheckpointStore> = checkpoint;

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
        fn parse_instruction(_instruction: &arch_program::instruction::Instruction) -> Option<core::instruction::DecodedInstruction<Self>> { None }
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

    // Provide AccountDatasource to enable account refresh after rollback/reapply
    let account_provider = atlas_arch_rpc_datasource::ArchRpcClient::new(rpc_url);

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
                        .push_bind(bitcoin_txids);
                });
                qb.push(" ON CONFLICT (txid) DO UPDATE SET block_height = EXCLUDED.block_height, data = EXCLUDED.data, status = EXCLUDED.status, bitcoin_txids = EXCLUDED.bitcoin_txids");
                if let Err(e) = qb.build().execute(&mut *tx).await {
                    let _ = metrics.increment_counter("tx_write_failed", 1).await;
                    return Err(core::error::Error::Custom(format!("tx upsert failed: {}", e)));
                } else {
                    let _ = metrics.increment_counter("tx_write_success", data.len() as u64).await;
                }
            } else if !data.is_empty() {
                // COPY into temp staging and upsert within a single transaction
                let mut client = open_copy_conn().await.map_err(|e| core::error::Error::Custom(format!("copy conn: {}", e)))?;
                let transaction = client.transaction().await.map_err(|e| core::error::Error::Custom(format!("copy tx begin: {}", e)))?;
                transaction.batch_execute("CREATE TEMP TABLE IF NOT EXISTS tmp_transactions (txid text, block_height bigint, data jsonb, status jsonb, bitcoin_txids text[]) ON COMMIT DROP;").await.map_err(|e| core::error::Error::Custom(format!("tmp table: {}", e)))?;
                let sink = transaction.copy_in("COPY tmp_transactions (txid, block_height, data, status, bitcoin_txids) FROM STDIN BINARY").await.map_err(|e| core::error::Error::Custom(format!("copy in: {}", e)))?;
                let writer = tokio_postgres::binary_copy::BinaryCopyInWriter::new(sink, &[tokio_postgres::types::Type::TEXT, tokio_postgres::types::Type::INT8, tokio_postgres::types::Type::JSONB, tokio_postgres::types::Type::JSONB, tokio_postgres::types::Type::TEXT_ARRAY]);
                let mut writer = std::pin::pin!(writer);
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
                    let txid: String = meta.id.clone();
                    let height_i64: i64 = meta.block_height as i64;
                    let json_data: serde_json::Value = data_json;
                    let json_status: serde_json::Value = status_json;
                    let txids_arr: Vec<String> = bitcoin_txids;
                    let params: [&(dyn ToSql + Sync); 5] = [
                        &txid,
                        &height_i64,
                        &json_data,
                        &json_status,
                        &txids_arr,
                    ];
                    writer.as_mut().write(&params).await.map_err(|e| core::error::Error::Custom(format!("copy write: {}", e)))?;
                }
                writer.as_mut().finish().await.map_err(|e| core::error::Error::Custom(format!("copy finish: {}", e)))?;
                transaction.batch_execute("INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids) SELECT txid, block_height, data, status, bitcoin_txids FROM tmp_transactions ON CONFLICT (txid) DO UPDATE SET block_height = EXCLUDED.block_height, data = EXCLUDED.data, status = EXCLUDED.status, bitcoin_txids = EXCLUDED.bitcoin_txids;").await.map_err(|e| core::error::Error::Custom(format!("copy upsert: {}", e)))?;
                transaction.commit().await.map_err(|e| core::error::Error::Custom(format!("copy tx commit: {}", e)))?;
                let _ = metrics.increment_counter("tx_write_success", data.len() as u64).await;
            }

            tx.commit().await.map_err(|e| core::error::Error::Custom(format!("db commit: {}", e)))?;
            Ok(())
        }
    }

    let mut pipeline_builder = Pipeline::builder()
        .datasource(syncing_ds)
        .metrics(Arc::new(PromMetrics))
        .account_datasource(Arc::new(account_provider))
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
        use_copy_bulk: bool,
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
            if !data.is_empty() && !self.use_copy_bulk {
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
                // COPY bulk for blocks within a single transaction
                let mut client = open_copy_conn().await.map_err(|e| core::error::Error::Custom(format!("copy conn: {}", e)))?;
                let transaction = client.transaction().await.map_err(|e| core::error::Error::Custom(format!("copy tx begin: {}", e)))?;
                transaction.batch_execute("CREATE TEMP TABLE IF NOT EXISTS tmp_blocks (height bigint, hash text, ts_seconds double precision) ON COMMIT DROP;").await.map_err(|e| core::error::Error::Custom(format!("tmp table: {}", e)))?;
                let sink = transaction.copy_in("COPY tmp_blocks (height, hash, ts_seconds) FROM STDIN BINARY").await.map_err(|e| core::error::Error::Custom(format!("copy in: {}", e)))?;
                let writer = tokio_postgres::binary_copy::BinaryCopyInWriter::new(sink, &[tokio_postgres::types::Type::INT8, tokio_postgres::types::Type::TEXT, tokio_postgres::types::Type::FLOAT8]);
                let mut writer = std::pin::pin!(writer);
                for b in &data {
                    let micros: i64 = b.block_time.unwrap_or(0);
                    let secs_f64: f64 = (micros as f64) / 1_000_000_f64;
                    let hash = b.block_hash.map(|h| format!("{:?}", h)).unwrap_or_default();
                    use tokio_postgres::types::ToSql;
                    let height_i64: i64 = b.height as i64;
                    let hash_owned: String = hash;
                    let secs: f64 = secs_f64;
                    let params: [&(dyn ToSql + Sync); 3] = [
                        &height_i64,
                        &hash_owned,
                        &secs,
                    ];
                    writer.as_mut().write(&params).await.map_err(|e| core::error::Error::Custom(format!("copy write: {}", e)))?;
                    let h_i64 = b.height as i64;
                    if h_i64 > max_height_in_batch { max_height_in_batch = h_i64; }
                    if h_i64 < min_height_in_batch { min_height_in_batch = h_i64; }
                }
                writer.as_mut().finish().await.map_err(|e| core::error::Error::Custom(format!("copy finish: {}", e)))?;
                transaction.batch_execute("INSERT INTO blocks (height, hash, timestamp) SELECT height, hash, to_timestamp(ts_seconds) FROM tmp_blocks ON CONFLICT (height) DO UPDATE SET hash = EXCLUDED.hash, timestamp = EXCLUDED.timestamp;").await.map_err(|e| core::error::Error::Custom(format!("copy upsert: {}", e)))?;
                transaction.commit().await.map_err(|e| core::error::Error::Custom(format!("copy tx commit: {}", e)))?;
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
        pool: db_pool.clone(),
        tip_height: tip_height.clone(),
        last_report_instant: Instant::now(),
        last_report_height: -1,
        ema_rate_hps: 0.0,
        start_height: None,
        initial_tip_height: None,
        start_instant: Instant::now(),
        growth_samples: VecDeque::new(),
        use_copy_bulk,
    };
    pipeline_builder = pipeline_builder.block_details(block_proc);

    // Rollback/Reapplied processors: update transactions + request account refresh
    // Helpers to resolve affected pubkeys from tx data
    fn json_value_to_pubkey(value: &serde_json::Value) -> Option<arch_program::pubkey::Pubkey> {
        if let Some(arr) = value.as_array() {
            let bytes: Vec<u8> = arr.iter().filter_map(|x| x.as_u64().map(|n| n as u8)).collect();
            if bytes.len() == 32 { return Some(arch_program::pubkey::Pubkey::from_slice(&bytes)); }
            return None;
        }
        if let Some(s) = value.as_str() {
            if s.chars().all(|c| c.is_ascii_hexdigit()) && s.len() >= 2 {
                if let Ok(bytes) = hex::decode(s) { if bytes.len() == 32 { return Some(arch_program::pubkey::Pubkey::from_slice(&bytes)); } }
            } else if let Ok(bytes) = bs58::decode(s).into_vec() {
                if bytes.len() == 32 { return Some(arch_program::pubkey::Pubkey::from_slice(&bytes)); }
            }
        }
        None
    }

    fn extract_pubkeys_from_tx_json(data: &serde_json::Value) -> Vec<arch_program::pubkey::Pubkey> {
        let mut out: Vec<arch_program::pubkey::Pubkey> = Vec::new();
        if let Some(keys) = data
            .get("message")
            .and_then(|m| m.get("account_keys"))
            .and_then(|v| v.as_array())
        {
            for k in keys { if let Some(pk) = json_value_to_pubkey(k) { out.push(pk); } }
        }
        out.sort();
        out.dedup();
        out
    }

    async fn resolve_pubkeys_for_txids(
        pool: &PgPool,
        rpc: &crate::arch_rpc::ArchRpcClient,
        txids: &[String],
    ) -> anyhow::Result<HashSet<arch_program::pubkey::Pubkey>> {
        use std::collections::HashSet as StdHashSet;
        let mut result: StdHashSet<arch_program::pubkey::Pubkey> = StdHashSet::new();
        if txids.is_empty() { return Ok(result); }
        let rows: Vec<(String, serde_json::Value)> = sqlx::query_as(
            "SELECT txid, data FROM transactions WHERE txid = ANY($1)"
        )
        .bind(txids)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let present: StdHashSet<String> = rows.iter().map(|(id, _)| id.clone()).collect();
        for (_, data_json) in &rows { for pk in extract_pubkeys_from_tx_json(data_json) { result.insert(pk); } }

        for txid in txids {
            if present.contains(txid) { continue; }
            if let Ok(processed) = rpc.get_processed_transaction(txid).await {
                let data = processed.runtime_transaction;
                for pk in extract_pubkeys_from_tx_json(&data) { result.insert(pk); }
            }
        }
        Ok(result)
    }

    struct RolledbackTxProcessor { pool: Arc<PgPool>, rpc: Arc<crate::arch_rpc::ArchRpcClient> }
    #[async_trait::async_trait]
    impl core::processor::Processor for RolledbackTxProcessor {
        type InputType = core::datasource::RolledbackTransactionsEvent;
        type OutputType = HashSet<arch_program::pubkey::Pubkey>;
        async fn process(
            &mut self,
            data: Vec<Self::InputType>,
            metrics: Arc<core::metrics::MetricsCollection>,
        ) -> core::error::IndexerResult<Self::OutputType> {
            let mut all_txids: Vec<String> = Vec::new();
            for ev in &data { all_txids.extend(ev.transaction_hashes.clone()); }
            if !all_txids.is_empty() {
                let _ = sqlx::query("UPDATE transactions SET status = jsonb_set(COALESCE(status, '{}'), '{rolled_back}', 'true') WHERE txid = ANY($1)")
                    .bind(&all_txids[..])
                    .execute(&*self.pool)
                    .await;
            }
            let mut pubkeys: HashSet<arch_program::pubkey::Pubkey> = HashSet::new();
            if let Ok(set) = resolve_pubkeys_for_txids(&self.pool, &self.rpc, &all_txids).await { pubkeys = set; }
            let _ = metrics.increment_counter("rollback_refresh_pubkeys", pubkeys.len() as u64).await;
            Ok(pubkeys)
        }
    }

    struct ReappliedTxProcessor { pool: Arc<PgPool>, rpc: Arc<crate::arch_rpc::ArchRpcClient> }
    #[async_trait::async_trait]
    impl core::processor::Processor for ReappliedTxProcessor {
        type InputType = core::datasource::ReappliedTransactionsEvent;
        type OutputType = HashSet<arch_program::pubkey::Pubkey>;
        async fn process(
            &mut self,
            data: Vec<Self::InputType>,
            metrics: Arc<core::metrics::MetricsCollection>,
        ) -> core::error::IndexerResult<Self::OutputType> {
            let mut all_txids: Vec<String> = Vec::new();
            for ev in &data { all_txids.extend(ev.transaction_hashes.clone()); }
            if !all_txids.is_empty() {
                let _ = sqlx::query("UPDATE transactions SET status = (status - 'rolled_back') WHERE txid = ANY($1)")
                    .bind(&all_txids[..])
                    .execute(&*self.pool)
                    .await;
            }
            let mut pubkeys: HashSet<arch_program::pubkey::Pubkey> = HashSet::new();
            if let Ok(set) = resolve_pubkeys_for_txids(&self.pool, &self.rpc, &all_txids).await { pubkeys = set; }
            let _ = metrics.increment_counter("reapplied_refresh_pubkeys", pubkeys.len() as u64).await;
            Ok(pubkeys)
        }
    }

    let rpc_for_resolver = Arc::new(crate::arch_rpc::ArchRpcClient::new(rpc_url.to_string()));
    let rb_proc = RolledbackTxProcessor { pool: db_pool.clone(), rpc: rpc_for_resolver.clone() };
    pipeline_builder = pipeline_builder.rolledback_transactions(rb_proc);
    let rp_proc = ReappliedTxProcessor { pool: db_pool.clone(), rpc: rpc_for_resolver.clone() };
    pipeline_builder = pipeline_builder.reapplied_transactions(rp_proc);

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
