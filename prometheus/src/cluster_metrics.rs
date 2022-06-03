use solana_gossip::cluster_info::ClusterInfo;
use solana_runtime::bank::Bank;

use crate::{utils::{write_metric, Metric, MetricFamily}, token::Lamports};
use std::{io, sync::Arc, time::SystemTime};

pub fn write_cluster_metrics<W: io::Write>(
    at: SystemTime,
    bank: &Arc<Bank>,
    cluster_info: &Arc<ClusterInfo>,
    out: &mut W,
) -> io::Result<()> {
    let identity_pubkey = cluster_info.id();
    write_metric(
        out,
        &MetricFamily {
            name: "solana_cluster_identity_info",
            help: "The current node's identity",
            type_: "count",
            metrics: vec![Metric::new(1)
                .with_label("identity", identity_pubkey.to_string())
                .at(at)],
        },
    )?;

    let identity_balance = Lamports(bank.get_balance(&identity_pubkey));
    write_metric(
        out,
        &MetricFamily {
            name: "solana_cluster_identity_balance_total",
            help: "The current node's identity balance",
            type_: "count",
            metrics: vec![Metric::new_sol(identity_balance).at(at)],
        },
    )?;

    Ok(())
}
