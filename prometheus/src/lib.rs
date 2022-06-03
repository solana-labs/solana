mod bank_metrics;
mod cluster_metrics;
mod token;
mod utils;

use solana_gossip::cluster_info::ClusterInfo;
use solana_runtime::bank_forks::BankForks;
use std::{
    sync::{Arc, RwLock},
    time::SystemTime,
};

pub fn render_prometheus(
    bank_forks: &Arc<RwLock<BankForks>>,
    cluster_info: &Arc<ClusterInfo>,
) -> Vec<u8> {
    let current_bank = bank_forks.read().unwrap().working_bank();
    let now = SystemTime::now();
    let mut out: Vec<u8> = Vec::new();
    bank_metrics::write_bank_metrics(now, &current_bank, &mut out).expect("IO error");
    cluster_metrics::write_cluster_metrics(now, &current_bank, &cluster_info, &mut out)
        .expect("IO error");
    out
}
