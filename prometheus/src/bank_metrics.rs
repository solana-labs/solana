use crate::{
    banks_with_commitments::BanksWithCommitments,
    utils::{write_metric, Metric, MetricFamily},
};
use std::io;

pub fn write_bank_metrics<W: io::Write>(
    banks_with_commitments: &BanksWithCommitments,
    out: &mut W,
) -> io::Result<()> {
    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_slot",
            help: "Block Slot",
            type_: "gauge",
            metrics: banks_with_commitments
                .for_each_commitment(|bank| Some(Metric::new(bank.clock().slot))),
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_epoch",
            help: "Block Epoch",
            type_: "gauge",
            metrics: banks_with_commitments
                .for_each_commitment(|bank| Some(Metric::new(bank.clock().epoch))),
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_timestamp_seconds",
            help: "The block's UNIX timestamp, in seconds since epoch, UTC",
            type_: "gauge",
            metrics: banks_with_commitments
                .for_each_commitment(|bank| Some(Metric::new(bank.clock().unix_timestamp as u64))),
        },
    )?;

    Ok(())
}
