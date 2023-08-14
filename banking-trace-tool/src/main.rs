use {
    chrono::{DateTime, Utc},
    clap::{Parser, ValueEnum},
    histogram::Histogram,
    process::process_event_files,
    solana_core::{
        banking_stage::immutable_deserialized_packet::ImmutableDeserializedPacket,
        banking_trace::{ChannelLabel, TimedTracedEvent, TracedEvent},
    },
    std::{
        fs::File,
        io::Read,
        path::{Path, PathBuf},
        process::exit,
        time::SystemTime,
    },
};

mod process;
mod slot_priority_tracker;

#[derive(Parser)]
struct Args {
    /// The path to the banking trace event files.
    #[clap(short, long)]
    path: PathBuf,
    /// The mode to run the trace tool in.
    #[clap(short, long, default_value_t = TraceToolMode::SimpleLog)]
    mode: TraceToolMode,
}

#[derive(Copy, Clone, ValueEnum, PartialEq, Debug)]
enum TraceToolMode {
    /// Log ticks and packet batch labels/counts with timestamps.
    SimpleLog,
    /// Log only non-vote packet batches and counts with timestamps.
    NonVoteLog,
    /// Collect metrics on packet batches and counts for NonVote packets.
    NonVoteCountMetrics,
    /// Deserialize transactions and collect the transactions w/ timestamps.
    TransactionLog,
    /// Slot priority tracker.
    SlotPriorityTracker,
}

impl std::fmt::Display for TraceToolMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceToolMode::SimpleLog => write!(f, "simple-log"),
            TraceToolMode::NonVoteLog => write!(f, "non-vote-log"),
            TraceToolMode::NonVoteCountMetrics => write!(f, "non-vote-count-metrics"),
            TraceToolMode::TransactionLog => write!(f, "transaction-log"),
            TraceToolMode::SlotPriorityTracker => write!(f, "slot-priority-tracker"),
        }
    }
}

fn main() {
    let Args { path, mode } = Args::parse();

    if !path.is_dir() {
        eprintln!("Error: {} is not a directory", path.display());
        exit(1);
    }

    let event_file_paths = get_event_file_paths(&path);
    let result = match mode {
        TraceToolMode::SimpleLog => process_event_files(&event_file_paths, &mut simple_logger),
        TraceToolMode::NonVoteLog => process_event_files(&event_file_paths, &mut non_vote_logger),
        TraceToolMode::NonVoteCountMetrics => non_vote_count_metrics(&event_file_paths),
        TraceToolMode::TransactionLog => {
            process_event_files(&event_file_paths, &mut transaction_logger)
        }
        TraceToolMode::SlotPriorityTracker => slot_priority_tracker::run(&event_file_paths),
    };

    if let Err(err) = result {
        eprintln!("Error: {}", err);
        exit(1);
    }
}

/// Get event file paths ordered by first timestamp.
fn get_event_file_paths(path: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut event_file_paths = get_event_file_paths_unordered(path);
    event_file_paths.sort_by_key(|event_filepath| {
        read_first_timestamp(event_filepath).unwrap_or_else(|err| {
            eprintln!(
                "Error reading first timestamp from {}: {}",
                event_filepath.display(),
                err
            );
            exit(1);
        })
    });
    event_file_paths
}

fn get_event_file_paths_unordered(path: impl AsRef<Path>) -> Vec<PathBuf> {
    (0..)
        .map(|index| {
            let event_filename = if index == 0 {
                "events".to_owned()
            } else {
                format!("events.{index}")
            };
            path.as_ref().join(event_filename)
        })
        .take_while(|event_filepath| event_filepath.exists())
        .collect()
}

fn read_first_timestamp(path: impl AsRef<Path>) -> std::io::Result<SystemTime> {
    const SYSTEM_TIME_BYTES: usize = core::mem::size_of::<SystemTime>();
    let mut buffer = [0u8; SYSTEM_TIME_BYTES];

    let mut file = File::open(path)?;
    file.read_exact(&mut buffer)?;

    let system_time = bincode::deserialize(&buffer).unwrap();
    Ok(system_time)
}

fn simple_logger(TimedTracedEvent(timestamp, event): TimedTracedEvent) {
    let utc = DateTime::<Utc>::from(timestamp);
    match event {
        TracedEvent::PacketBatch(label, banking_packet_batch) => {
            let packet_batches = &banking_packet_batch.0;
            let num_batches = packet_batches.len();

            // ignores tracer stats
            if num_batches > 0 {
                let num_packets = packet_batches
                    .iter()
                    .map(|batch| batch.len())
                    .sum::<usize>();
                println!(
                    "{utc}: recv {label} num_batches: {num_batches} num_packets: {num_packets}"
                );
            }
        }
        TracedEvent::BlockAndBankHash(slot, blockhash, bankhash) => {
            println!("{utc}: tick {slot} {blockhash} {bankhash}");
        }
    }
}

fn non_vote_logger(TimedTracedEvent(timestamp, event): TimedTracedEvent) {
    let TracedEvent::PacketBatch(label, banking_packet_batch) = event else {
        return;
    };

    if !matches!(label, ChannelLabel::NonVote) {
        return;
    }

    let packet_batches = &banking_packet_batch.0;
    let num_batches = packet_batches.len();

    // ignores tracer stats
    if num_batches > 0 {
        let utc = DateTime::<Utc>::from(timestamp);
        let num_packets = packet_batches
            .iter()
            .map(|batch| batch.len())
            .sum::<usize>();
        println!("{utc}: recv {label} num_batches: {num_batches} num_packets: {num_packets}");
    }
}

fn transaction_logger(TimedTracedEvent(timestamp, event): TimedTracedEvent) {
    let TracedEvent::PacketBatch(label, banking_packet_batch) = event else {
        return;
    };

    if !matches!(label, ChannelLabel::NonVote) {
        return;
    }

    let utc = DateTime::<Utc>::from(timestamp);
    for packet in banking_packet_batch.0.iter().flatten() {
        let Ok(packet) = ImmutableDeserializedPacket::new(packet.clone()) else {
            continue;
        };

        if packet
            .transaction()
            .get_message()
            .message
            .address_table_lookups()
            .is_some()
        {
            static mut LOOKUP_TABLES_COUNT: usize = 0;
            // SAFETY: Single-threaded
            unsafe {
                println!("{utc}: skipping due to lookup tables - {LOOKUP_TABLES_COUNT}");
                LOOKUP_TABLES_COUNT += 1;
            }
        }
        let priority = packet.priority();
        let transaction = packet.transaction();
        let signature = transaction.get_signatures().first().unwrap();
        let message = &transaction.get_message().message;
        let keys = message.static_account_keys();
        let writable_accounts = keys
            .iter()
            .enumerate()
            .filter(|(index, _)| message.is_maybe_writable(*index))
            .collect::<Vec<_>>();
        let readable_accounts = keys
            .iter()
            .enumerate()
            .filter(|(index, _)| !message.is_maybe_writable(*index))
            .collect::<Vec<_>>();
        println!("{utc}: signature: {signature} priority: {priority} writable_accounts: {writable_accounts:?} readable_accounts: {readable_accounts:?}");
    }
}

fn non_vote_count_metrics(event_file_paths: &[PathBuf]) -> std::io::Result<()> {
    let mut total_batches = 0;
    let mut total_packets = 0;
    let mut batch_count_histogram = Histogram::new();
    let mut non_zero_batch_count_histogram = Histogram::new();
    let mut batch_length_histogram = Histogram::new();
    let mut packet_count_histogram = Histogram::new();

    let mut metric_collector = |TimedTracedEvent(_, event): TimedTracedEvent| {
        let TracedEvent::PacketBatch(label, banking_packet_batch) = event else {
            return;
        };

        if !matches!(label, ChannelLabel::NonVote) {
            return;
        }

        let packet_batches = &banking_packet_batch.0;

        let num_batches = packet_batches.len();
        batch_count_histogram.increment(num_batches as u64).unwrap();
        if num_batches > 0 {
            non_zero_batch_count_histogram
                .increment(num_batches as u64)
                .unwrap();
            let mut num_packets = 0;
            for batch in packet_batches {
                num_packets += batch.len();
                batch_length_histogram
                    .increment(batch.len() as u64)
                    .unwrap();
            }
            packet_count_histogram
                .increment(num_packets as u64)
                .unwrap();

            total_batches += num_batches;
            total_packets += num_packets;
        }
    };

    process_event_files(event_file_paths, &mut metric_collector)?;

    println!("total_batches: {}", total_batches);
    println!("total_packets: {}", total_packets);

    pretty_print_histogram("batch_count", &batch_count_histogram);
    pretty_print_histogram("non-zero batch_count", &non_zero_batch_count_histogram);
    pretty_print_histogram("batch_length", &batch_length_histogram);
    pretty_print_histogram("packet_count", &packet_count_histogram);

    Ok(())
}

fn pretty_print_histogram(name: &str, histogram: &Histogram) {
    print!("{name}: [");

    const PERCENTILES: &[f64] = &[5.0, 10.0, 25.0, 50.0, 75.0, 90.0, 95.0, 99.0, 99.9];
    for percentile in PERCENTILES.into_iter().copied() {
        print!(
            "{}: {}, ",
            percentile,
            histogram.percentile(percentile).unwrap()
        );
    }
    println!("]");
}
