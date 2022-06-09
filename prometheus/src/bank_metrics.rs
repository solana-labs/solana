use solana_runtime::bank::Bank;

use crate::utils::{write_metric, Metric, MetricFamily};
use std::{io, sync::Arc};

pub fn write_bank_metrics<W: io::Write>(bank: &Arc<Bank>, out: &mut W) -> io::Result<()> {
    write_metric(
        out,
        &MetricFamily {
            name: "solana_bank_slot",
            help: "Current Slot",
            type_: "gauge",
            metrics: vec![Metric::new(bank.slot())],
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_bank_epoch",
            help: "Current Epoch",
            type_: "gauge",
            metrics: vec![Metric::new(bank.epoch())],
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_bank_successful_transaction_count",
            help: "Number of transactions in the block that executed successfully",
            type_: "gauge",
            metrics: vec![Metric::new(bank.transaction_count())],
        },
    )?;
    write_metric(
        out,
        &MetricFamily {
            name: "solana_bank_error_transaction_count",
            help: "Number of transactions in the block that executed with error",
            type_: "gauge",
            metrics: vec![Metric::new(bank.transaction_error_count())],
        },
    )?;

    Ok(())
}
