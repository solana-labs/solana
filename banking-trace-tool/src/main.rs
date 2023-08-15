use {
    clap::{Args, Parser, Subcommand},
    count_metrics::do_count_metrics,
    log::{do_logging, LoggingKind},
    setup::get_event_file_paths,
    slot_priority_tracker::{TrackingKind, TrackingVerbosity},
    std::{path::PathBuf, process::exit},
};

mod count_metrics;
mod log;
mod process;
mod setup;
mod slot_priority_tracker;

#[derive(Parser)]
struct AppArgs {
    /// The path to the banking trace event files.
    #[clap(short, long)]
    path: PathBuf,
    /// The mode to run the trace tool in.
    #[command(subcommand)]
    mode: TraceToolMode,
}

#[derive(Copy, Clone, Debug, PartialEq, Subcommand)]
enum TraceToolMode {
    /// Simply log without additional processing.
    Log { kind: LoggingKind },
    /// Collect metrics on batch and packet count.
    CountMetrics,
    /// Collect metrics on packets by slot and priority.
    SlotPriorityTracker(SlotPriorityTrackerArgs),
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
        TraceToolMode::SlotPriorityTracker(SlotPriorityTrackerArgs { kind, verbosity }) => {
            slot_priority_tracker::do_slot_priority_tracking(&event_file_paths, kind, verbosity)
        }
    };

    if let Err(err) = result {
        eprintln!("Error: {}", err);
        exit(1);
    }
}
