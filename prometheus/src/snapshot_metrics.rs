use crate::utils::{write_metric, Metric, MetricFamily};
use solana_runtime::snapshot_archive_info::{SnapshotArchiveInfo, SnapshotArchiveInfoGetter};
use solana_runtime::snapshot_config::SnapshotConfig;
use solana_runtime::snapshot_utils;
use solana_sdk::clock::Slot;
use std::io;

pub fn write_snapshot_metrics<W: io::Write>(
    snapshot_config: &Option<SnapshotConfig>,
    out: &mut W,
) -> io::Result<()> {
    let snapshot_config = match snapshot_config.as_ref() {
        Some(config) => config,
        None => return Ok(()),
    };

    let full_snapshot_info = match get_full_snapshot_info(snapshot_config) {
        Some(info) => info,
        None => return Ok(()),
    };
    write_metric(
        out,
        &MetricFamily {
            name: "solana_full_snapshot_info",
            help: "The slot height of the most recent full snapshot",
            type_: "counter",
            metrics: vec![Metric::new(full_snapshot_info.slot)
                .with_label("hash", full_snapshot_info.hash.to_string())],
        },
    )?;

    // Incremental snapshots may be disabled, so we write full snapshot
    // metric before and just return early if that is the case.
    let incremental_snapshot_info =
        match get_incremental_snapshot_info(snapshot_config, full_snapshot_info.slot) {
            None => return Ok(()),
            Some(info) => info,
        };
    write_metric(
        out,
        &MetricFamily {
            name: "solana_incremental_snapshot_info",
            help: "The slot height of the most recent incremental snapshot",
            type_: "counter",
            metrics: vec![Metric::new(incremental_snapshot_info.slot)
                .with_label("hash", incremental_snapshot_info.hash.to_string())],
        },
    )
}

fn get_full_snapshot_info(snapshot_config: &SnapshotConfig) -> Option<SnapshotArchiveInfo> {
    snapshot_utils::get_highest_full_snapshot_archive_info(&snapshot_config.snapshot_archives_dir)
        .map(|full_snapshot_info| full_snapshot_info.snapshot_archive_info().clone())
}

fn get_incremental_snapshot_info(
    snapshot_config: &SnapshotConfig,
    slot: Slot,
) -> Option<SnapshotArchiveInfo> {
    snapshot_utils::get_highest_incremental_snapshot_archive_info(
        &snapshot_config.snapshot_archives_dir,
        slot,
    )
    .map(|inc_snapshot_info| inc_snapshot_info.snapshot_archive_info().clone())
}
