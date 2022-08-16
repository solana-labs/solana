use crate::utils::{write_metric, Metric, MetricFamily};
use solana_runtime::snapshot_archive_info::{SnapshotArchiveInfo, SnapshotArchiveInfoGetter};
use solana_runtime::snapshot_config::SnapshotConfig;
use solana_runtime::snapshot_utils;
use solana_sdk::clock::Slot;
use std::io;

pub fn write_snapshot_metrics<W: io::Write>(
    snapshot_config: &SnapshotConfig,
    out: &mut W,
) -> io::Result<()> {
    let full_snapshot_info = match snapshot_utils::get_highest_full_snapshot_archive_info(
        &snapshot_config.snapshot_archives_dir,
    )
    .map(|full_snapshot_info| full_snapshot_info.snapshot_archive_info())
    {
        Some(info) => info,
        None => return Ok(()),
    };
    write_metric(
        out,
        &MetricFamily {
            name: "solana_snapshot_last_full_snapshot_slot",
            help: "The slot height of the most recent full snapshot",
            type_: "gauge",
            metrics: vec![Metric::new(full_snapshot_info.slot)],
        },
    )?;

    // Incremental snapshots may be disabled, so we write full snapshot
    // metric before and just return early if that is the case.
    let incremental_snapshot_info =
        match snapshot_utils::get_highest_incremental_snapshot_archive_info(
            &snapshot_config.snapshot_archives_dir,
            slot,
        )
        .map(|inc_snapshot_info| inc_snapshot_info.snapshot_archive_info())
        {
            None => return Ok(()),
            Some(info) => info,
        };
    write_metric(
        out,
        &MetricFamily {
            name: "solana_snapshot_last_incremental_snapshot_slot",
            help: "The slot height of the most recent incremental snapshot",
            type_: "gauge",
            metrics: vec![Metric::new(incremental_snapshot_info.slot)],
        },
    )
}
