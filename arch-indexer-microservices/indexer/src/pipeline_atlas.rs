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
use sqlx::PgPool;

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
    let ds_cfg = ArchDatasourceConfig::default();
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

            for (meta, _parsed, _matched) in data.into_iter() {
                // Map to our schema columns
                let txid = &meta.id; // stored as text primary key
                let block_height = meta.block_height as i64;
                let status_json = serde_json::to_value(&meta.status)
                    .unwrap_or(serde_json::json!(null));

                let query = "INSERT INTO transactions (txid, block_height, data, status, bitcoin_txids) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (txid) DO UPDATE SET block_height = EXCLUDED.block_height, data = EXCLUDED.data, status = EXCLUDED.status, bitcoin_txids = EXCLUDED.bitcoin_txids";

                // Minimal data payload until full mapping is implemented
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

                if let Err(e) = sqlx::query(query)
                    .bind(txid)
                    .bind(block_height)
                    .bind(data_json)
                    .bind(status_json)
                    .bind(&bitcoin_txids[..])
                    .execute(&mut *tx)
                    .await
                {
                    let _ = metrics.increment_counter("tx_write_failed", 1).await;
                    return Err(core::error::Error::Custom(format!("tx upsert failed: {}", e)));
                } else {
                    let _ = metrics.increment_counter("tx_write_success", 1).await;
                }
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

    // BlockDetails processor → blocks table
    struct BlockDetailsDbProcessor {
        pool: Arc<PgPool>,
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
            for b in data.into_iter() {
                // Atlas block_time appears to be in microseconds; convert to seconds for to_timestamp
                let micros: i64 = b.block_time.unwrap_or(0);
                let secs_f64: f64 = (micros as f64) / 1_000_000_f64;

                let query = "INSERT INTO blocks (height, hash, timestamp) VALUES ($1, $2, to_timestamp($3)) ON CONFLICT (height) DO UPDATE SET hash = EXCLUDED.hash, timestamp = EXCLUDED.timestamp";
                let hash = b.block_hash.map(|h| format!("{:?}", h)).unwrap_or_default();
                if let Err(e) = sqlx::query(query)
                    .bind(b.height as i64)
                    .bind(&hash)
                    .bind(secs_f64)
                    .execute(&mut *tx)
                    .await
                {
                    let _ = metrics.increment_counter("block_write_failed", 1).await;
                    return Err(core::error::Error::Custom(format!("block upsert failed: {}", e)));
                } else {
                    let _ = metrics.increment_counter("block_write_success", 1).await;
                }
            }
            tx.commit().await.map_err(|e| core::error::Error::Custom(format!("db commit: {}", e)))?;
            Ok(())
        }
    }

    let block_proc = BlockDetailsDbProcessor { pool: db_pool };
    pipeline_builder = pipeline_builder.block_details(block_proc);

    let mut pipeline: Pipeline = pipeline_builder.build()?;

    pipeline.run().await?;
    Ok(())
}
