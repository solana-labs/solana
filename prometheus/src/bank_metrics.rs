use crate::{
    banks_with_commitments::BanksWithCommitments,
    utils::{write_metric, Metric, MetricFamily},
};
use std::io;

pub fn write_bank_metrics<W: io::Write>(
    banks_with_commitments: &BanksWithCommitments,
    out: &mut W,
) -> io::Result<()> {
    let clock_finalized = banks_with_commitments.finalized_bank.clock();

    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_slot",
            help: "Finalized Slot",
            type_: "gauge",
            metrics: vec![Metric::new(clock_finalized.slot)
                .with_label("commitment_level", "finalized".to_owned())],
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_epoch",
            help: "Finalized Epoch",
            type_: "gauge",
            metrics: vec![Metric::new(clock_finalized.epoch)
                .with_label("commitment_level", "finalized".to_owned())],
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_timestamp_seconds",
            help: "The block's finalized UNIX timestamp, in seconds since epoch, UTC",
            type_: "gauge",
            metrics: vec![Metric::new(clock_finalized.unix_timestamp as u64)
                .with_label("commitment_level", "finalized".to_owned())],
        },
    )?;

    Ok(())
}
