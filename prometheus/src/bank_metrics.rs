use crate::{
    banks_with_commitments::BanksWithCommitments,
    utils::{write_metric, Metric, MetricFamily},
};
use solana_sdk::sysvar;
use solana_sdk::sysvar::epoch_schedule::EpochSchedule;
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
            name: "solana_block_epoch_start_slot",
            help: "The first slot in the current epoch",
            type_: "gauge",
            metrics: banks_with_commitments.for_each_commitment(|bank| {
                // Note, the bank actually has a field that holds the EpochSchedule,
                // but it is not public, so we can't easily access it  here. We could
                // make it public, but to make our patches less invasive, load the
                // epoch schedule from the sysvar instead. It should always exist.
                let epoch_schedule: EpochSchedule = bank
                    .get_account(&sysvar::epoch_schedule::id())?
                    .deserialize_data()
                    .ok()?;
                let clock = bank.clock();
                Some(Metric::new(
                    epoch_schedule.get_first_slot_in_epoch(clock.epoch),
                ))
            }),
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_block_epoch_slots_total",
            help: "The duration of the current epoch, in slots.",
            type_: "gauge",
            metrics: banks_with_commitments.for_each_commitment(|bank| {
                // Note, the bank actually has a field that holds the EpochSchedule,
                // but it is not public, so we can't easily access it  here. We could
                // make it public, but to make our patches less invasive, load the
                // epoch schedule from the sysvar instead. It should always exist.
                let epoch_schedule: EpochSchedule = bank
                    .get_account(&sysvar::epoch_schedule::id())?
                    .deserialize_data()
                    .ok()?;
                let clock = bank.clock();
                Some(Metric::new(epoch_schedule.get_slots_in_epoch(clock.epoch)))
            }),
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
