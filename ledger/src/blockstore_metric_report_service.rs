//! The `blockstore_metric_report_service` periodically reports ledger store metrics.

use {
    crate::blockstore::Blockstore,
    std::{
        string::ToString,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

// Determines how often we report blockstore metrics under
// BlockstoreMetricReportService. Note that there are other blockstore
// metrics that are reported outside BlockstoreMetricReportService.
const BLOCKSTORE_METRICS_REPORT_PERIOD_MILLIS: u64 = 10000;

pub struct BlockstoreMetricReportService {
    t_cf_metric: JoinHandle<()>,
}

impl BlockstoreMetricReportService {
    pub fn new(blockstore: Arc<Blockstore>, exit: Arc<AtomicBool>) -> Self {
        let t_cf_metric = Builder::new()
            .name("solRocksCfMtrcs".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(
                    BLOCKSTORE_METRICS_REPORT_PERIOD_MILLIS,
                ));
                blockstore.submit_rocksdb_cf_metrics_for_all_cfs();
                blockstore.report_rpc_api_metrics();
            })
            .unwrap();
        Self { t_cf_metric }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_cf_metric.join()
    }
}
