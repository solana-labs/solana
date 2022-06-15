use solana_runtime::bank::Bank;

use crate::utils::{write_metric, Metric, MetricFamily};
use std::{io, sync::Arc};

pub fn write_bank_metrics<W: io::Write>(bank: &Arc<Bank>, out: &mut W) -> io::Result<()> {
    let clock = bank.clock();

    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_slot",
            help: "Current Slot",
            type_: "gauge",
            metrics: vec![Metric::new(clock.slot).with_label("commitment_level", "finalized")],
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_epoch",
            help: "Current Epoch",
            type_: "gauge",
            metrics: vec![Metric::new(clock.epoch)],
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_timestamp_seconds",
            help: "The block's UNIX timestamp, in seconds since epoch, UTC",
            type_: "gauge",
            metrics: vec![Metric::new(clock.unix_timestamp as u64)],
        },
    )?;

    Ok(())
}
