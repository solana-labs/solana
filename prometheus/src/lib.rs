mod bank_metrics;
mod cluster_metrics;
mod utils;

use solana_gossip::cluster_info::ClusterInfo;
use solana_runtime::bank_forks::BankForks;
use std::sync::{Arc, RwLock};

pub struct Lamports(pub u64);

pub fn render_prometheus(
    bank_forks: &Arc<RwLock<BankForks>>,
    cluster_info: &Arc<ClusterInfo>,
) -> Vec<u8> {
    let current_bank = bank_forks.read().unwrap().working_bank();
    let mut out: Vec<u8> = Vec::new();
    bank_metrics::write_bank_metrics(&current_bank, &mut out).expect("IO error");
    cluster_metrics::write_cluster_metrics(&current_bank, &cluster_info, &mut out)
        .expect("IO error");
    out
}
