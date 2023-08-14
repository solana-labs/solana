use {
    chrono::{DateTime, Utc},
    clap::{Parser, ValueEnum},
    solana_core::banking_trace::{ChannelLabel, TimedTracedEvent, TracedEvent},
    std::{
        fs::File,
        io::Read,
        path::{Path, PathBuf},
        process::exit,
        time::SystemTime,
    },
};

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
}

impl std::fmt::Display for TraceToolMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceToolMode::SimpleLog => write!(f, "simple-log"),
            TraceToolMode::NonVoteLog => write!(f, "non-vote-log"),
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
        TraceToolMode::SimpleLog => process_event_files(&event_file_paths, &simple_logger),
        TraceToolMode::NonVoteLog => process_event_files(&event_file_paths, &non_vote_logger),
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

fn process_event_files(
    event_file_paths: &[PathBuf],
    handler_fn: &impl Fn(TimedTracedEvent),
) -> std::io::Result<()> {
    for event_file_path in event_file_paths {
        println!("Processing events from {}:", event_file_path.display());
        process_event_file(&event_file_path, handler_fn)?;
    }
    Ok(())
}

fn process_event_file(
    path: impl AsRef<Path>,
    handler_fn: &impl Fn(TimedTracedEvent),
) -> std::io::Result<()> {
    let data = std::fs::read(path)?;

    // Deserialize events from the buffer
    let mut offset = 0;
    while offset < data.len() {
        match bincode::deserialize::<TimedTracedEvent>(&data[offset..]) {
            Ok(event) => {
                // Update the offset to the next event
                offset += bincode::serialized_size(&event).unwrap() as usize;
                handler_fn(event);
            }
            Err(err) => {
                eprintln!(
                    "Error deserializing event at offset {}. File size {}. Error: {:?}",
                    offset,
                    data.len(),
                    err,
                );
                return Ok(()); // TODO: Return an error
            }
        }
    }

    Ok(())
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
