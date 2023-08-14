use {
    clap::{Parser, Subcommand},
    count_metrics::do_count_metrics,
    log::{do_logging, LoggingKind},
    setup::get_event_file_paths,
    std::{path::PathBuf, process::exit},
};

mod count_metrics;
mod log;
mod process;
mod setup;
mod slot_priority_tracker;

#[derive(Parser)]
struct Args {
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
    SlotPriorityTracker,
}

fn main() {
    let Args { path, mode } = Args::parse();

    if !path.is_dir() {
        eprintln!("Error: {} is not a directory", path.display());
        exit(1);
    }

    let event_file_paths = get_event_file_paths(&path);
    let result = match mode {
        TraceToolMode::Log { kind } => do_logging(&event_file_paths, kind),
        TraceToolMode::CountMetrics => do_count_metrics(&event_file_paths),
        TraceToolMode::SlotPriorityTracker => slot_priority_tracker::run(&event_file_paths),
    };

    if let Err(err) = result {
        eprintln!("Error: {}", err);
        exit(1);
    }
}
