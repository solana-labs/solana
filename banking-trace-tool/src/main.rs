use {
    block_history::save_history_before,
    clap::{Args, Parser, Subcommand},
    count_metrics::do_count_metrics,
    leader_priority_heatmap::do_leader_priority_heatmap,
    log::{do_logging, LoggingKind},
    setup::get_event_file_paths,
    slot_priority_tracker::{do_slot_priority_tracking, TrackingKind, TrackingVerbosity},
    slot_range_report::do_log_slot_range,
    slot_ranges::do_get_slot_ranges,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{path::PathBuf, process::exit},
};

mod block_history;
mod count_metrics;
mod leader_priority_heatmap;
mod leader_slots_tracker;
mod log;
mod process;
mod setup;
mod slot_priority_tracker;
mod slot_range_report;
mod slot_ranges;

#[derive(Parser)]
struct AppArgs {
    /// The path to the banking trace event files.
    #[clap(short, long)]
    path: PathBuf,
    /// The mode to run the trace tool in.
    #[command(subcommand)]
    mode: TraceToolMode,
}

#[derive(Clone, Debug, PartialEq, Subcommand)]
enum TraceToolMode {
    /// Simply log without additional processing.
    Log {
        kind: LoggingKind,
    },
    /// Collect metrics on batch and packet count.
    CountMetrics,
    /// Get the ranges of slots for data in directory.
    SlotRanges,
    /// Collect metrics on packets by slot and priority.
    SlotPriorityTracker(SlotPriorityTrackerArgs),
    /// Log non-vote transactions in a slot range.
    LogSlotRange {
        /// Priority-sort transactions (within slot)
        #[arg(short, long)]
        priority_sort: bool,
        /// Filter already-processed tx signatures from logging. This requires using RPC client.
        #[arg(short, long)]
        filter_already_processed: bool,
        /// Start of slot range (inclusive).
        start: Slot,
        /// End of slot range (inclusive).
        end: Slot,
    },
    SaveBlockHistory {
        slot: Slot,
    },
    /// Heatmap of non-vote transaction priority and time-offset from beginning of slot range.
    SlotPriorityHeatmap {
        /// Directory to save heatmaps into.
        #[arg(short, long, default_value = "./heatmaps")]
        output_dir: PathBuf,

        /// Filter transactions using any of these keys.
        #[arg(short, long)]
        filter_keys: Vec<Pubkey>,
    },
}

#[derive(Args, Copy, Clone, Debug, PartialEq)]
struct SlotPriorityTrackerArgs {
    /// The kind of tracking to perform.
    kind: TrackingKind,
    /// The verbosity of the report.
    #[arg(default_value_t = TrackingVerbosity::default())]
    verbosity: TrackingVerbosity,
}

fn main() {
    let AppArgs { path, mode } = AppArgs::parse();

    if !path.is_dir() {
        eprintln!("Error: {} is not a directory", path.display());
        exit(1);
    }

    let event_file_paths = get_event_file_paths(&path);
    let result = match mode {
        TraceToolMode::Log { kind } => do_logging(&event_file_paths, kind),
        TraceToolMode::CountMetrics => do_count_metrics(&event_file_paths),
        TraceToolMode::SlotRanges => do_get_slot_ranges(&event_file_paths),
        TraceToolMode::SlotPriorityTracker(SlotPriorityTrackerArgs { kind, verbosity }) => {
            do_slot_priority_tracking(&event_file_paths, kind, verbosity)
        }
        TraceToolMode::LogSlotRange {
            start,
            end,
            priority_sort,
            filter_already_processed,
        } => do_log_slot_range(
            &event_file_paths,
            start,
            end,
            priority_sort,
            filter_already_processed,
        ),
        TraceToolMode::SaveBlockHistory { slot } => {
            save_history_before(slot);
            Ok(())
        }
        TraceToolMode::SlotPriorityHeatmap {
            output_dir,
            filter_keys,
        } => do_leader_priority_heatmap(&event_file_paths, output_dir, filter_keys),
    };

    if let Err(err) = result {
        eprintln!("Error: {}", err);
        exit(1);
    }
}
