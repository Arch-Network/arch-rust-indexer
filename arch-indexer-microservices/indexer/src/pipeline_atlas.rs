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
pub async fn run_syncing_pipeline(rpc_url: &str, ws_url: &str, rocks_path: &str) -> Result<()> {
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

    let mut pipeline: Pipeline = Pipeline::builder()
        .datasource(syncing_ds)
        .metrics(Arc::new(NoopMetrics))
        .shutdown_strategy(ShutdownStrategy::Immediate)
        .build()?;

    pipeline.run().await?;
    Ok(())
}


