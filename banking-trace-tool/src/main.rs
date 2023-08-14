use std::process::exit;

use {
    chrono::{DateTime, Utc},
    clap::{Parser, ValueEnum},
    solana_core::banking_trace::{TimedTracedEvent, TracedEvent},
    std::path::{Path, PathBuf},
};

#[derive(Parser)]
struct Args {
    /// The path to the banking trace event files.
    #[clap(short, long)]
    path: PathBuf,
    /// The mode to run the trace tool in.
    #[clap(short, long, default_value = "log")]
    mode: TraceToolMode,
}

#[derive(Copy, Clone, ValueEnum, PartialEq, Debug)]
enum TraceToolMode {
    /// Simply log the events to stdout.
    Log,
}

fn main() {
    let Args { path, mode: _mode } = Args::parse();

    if !path.is_dir() {
        eprintln!("Error: {} is not a directory", path.display());
        exit(1);
    }

    for index in 0.. {
        let event_filename = if index == 0 {
            "events".to_owned()
        } else {
            format!("events.{index}")
        };
        let event_file = path.join(event_filename);
        if !event_file.exists() {
            break;
        }
        println!("Reading events from {}:", event_file.display());
        read_event_file(&event_file).unwrap();
    }
}

fn read_event_file(path: impl AsRef<Path>) -> std::io::Result<()> {
    let data = std::fs::read(path)?;

    // Deserialize events from the buffer
    let mut offset = 0;
    while offset < data.len() {
        match bincode::deserialize::<TimedTracedEvent>(&data[offset..]) {
            Ok(event) => {
                // Update the offset to the next event
                offset += bincode::serialized_size(&event).unwrap() as usize;
                handle_event(event);
            }
            Err(_) => {
                eprintln!("Error deserializing event at offset {}", offset);
                break;
            }
        }
    }

    Ok(())
}

fn handle_event(TimedTracedEvent(timestamp, event): TimedTracedEvent) {
    let utc = DateTime::<Utc>::from(timestamp);

    match event {
        TracedEvent::PacketBatch(label, banking_packet_batch) => {
            let packets = &banking_packet_batch.0;
            let num_packets = packets.len();

            // ignores tracer stats
            if num_packets > 0 {
                println!("{utc}: recv {label} {num_packets}");
            }
        }
        TracedEvent::BlockAndBankHash(slot, blockhash, bankhash) => {
            println!("{utc}: tick {slot} {blockhash} {bankhash}");
        }
    }
}
