use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use std::time::Duration;

pub fn setup_metrics_recorder() -> PrometheusHandle {
    const EXPONENTIAL_SECONDS: &[f64] = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];

    PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("block_processing_time".to_string()),
            EXPONENTIAL_SECONDS,
        )
        .unwrap()
        .install_recorder()
        .unwrap()
}

#[derive(Clone)]
pub struct Metrics {
    pub prometheus_handle: PrometheusHandle,
}

impl Metrics {
    pub fn new(prometheus_handle: PrometheusHandle) -> Self {
        Self { prometheus_handle }
    }

    /// This function records the number of blocks processed.
    /// It increments the counter by 1.
    pub fn record_block_processed(&self) {
        metrics::increment_counter!("blocks_processed_total");
    }

    /// This function records the time taken to process a block.
    /// It records the duration in seconds.
    pub fn record_block_processing_time(&self, duration: Duration) {
        metrics::histogram!("block_processing_time", duration.as_secs_f64());
    }

    /// This function records the number of transactions processed.
    /// It increments the counter by 1.
    pub fn record_transaction_processed(&self) {
        metrics::increment_counter!("transactions_processed_total");
    }

    /// This function records the sync progress in terms of current and target height.
    /// It updates gauges for the current height, target height, and progress percentage.
    pub fn record_sync_progress(&self, current_height: i64, target_height: i64) {
        metrics::gauge!("sync_current_height", current_height as f64);
        metrics::gauge!("sync_target_height", target_height as f64);

        let progress = if target_height > 0 {
            (current_height as f64 / target_height as f64) * 100.0
        } else {
            0.0
        };
        metrics::gauge!("sync_progress_percentage", progress);
    }
}
