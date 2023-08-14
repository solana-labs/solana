use {
    crate::process::process_event_files,
    chrono::{DateTime, Utc},
    clap::ValueEnum,
    solana_core::{
        banking_stage::immutable_deserialized_packet::ImmutableDeserializedPacket,
        banking_trace::{ChannelLabel, TimedTracedEvent, TracedEvent},
    },
    std::path::PathBuf,
};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum LoggingKind {
    #[default]
    Simple,
    NonVoteSimple,
    NonVoteTransaction,
}

pub fn do_logging(event_file_paths: &[PathBuf], logging_kind: LoggingKind) -> std::io::Result<()> {
    match logging_kind {
        LoggingKind::Simple => process_event_files(event_file_paths, &mut simple_logger),
        LoggingKind::NonVoteSimple => process_event_files(event_file_paths, &mut non_vote_logger),
        LoggingKind::NonVoteTransaction => {
            process_event_files(event_file_paths, &mut transaction_logger)
        }
    }
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
