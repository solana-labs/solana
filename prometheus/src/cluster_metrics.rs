use solana_gossip::cluster_info::ClusterInfo;
use solana_runtime::bank::Bank;

use crate::{
    token::Lamports,
    utils::{write_metric, Metric, MetricFamily},
};
use std::{io, sync::Arc};

pub fn write_cluster_metrics<W: io::Write>(
    bank: &Arc<Bank>,
    cluster_info: &Arc<ClusterInfo>,
    out: &mut W,
) -> io::Result<()> {
    let identity_pubkey = cluster_info.id();
    let version = cluster_info
        .get_node_version(&identity_pubkey)
        .unwrap_or_default();

    write_metric(
        out,
        &MetricFamily {
            name: "solana_node_identity_public_key_info",
            help: "The current node's identity",
            type_: "count",
            metrics: vec![Metric::new(1).with_label("identity", identity_pubkey.to_string())],
        },
    )?;

    let identity_balance = Lamports(bank.get_balance(&identity_pubkey));
    write_metric(
        out,
        &MetricFamily {
            name: "solana_node_identity_balance_total",
            help: "The current node's identity balance",
            type_: "gauge",
            metrics: vec![Metric::new_sol(identity_balance)],
        },
    )?;

    write_metric(
        out,
        &MetricFamily {
            name: "solana_node_version_info",
            help: "The current Solana node's version",
            type_: "count",
            metrics: vec![Metric::new(1).with_label("version", version.to_string())],
        },
    )?;

    Ok(())
}
