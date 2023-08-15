use {
    crate::process::process_event_files,
    solana_core::{
        banking_stage::immutable_deserialized_packet::ImmutableDeserializedPacket,
        banking_trace::{BankingPacketBatch, ChannelLabel, TimedTracedEvent, TracedEvent},
    },
    solana_sdk::clock::Slot,
    std::{path::PathBuf, time::SystemTime},
};

pub fn do_log_slot_range(
    event_file_paths: &[PathBuf],
    start: Slot,
    end: Slot,
) -> std::io::Result<()> {
    let mut collector = SlotRangeCollector::new(start, end);
    process_event_files(event_file_paths, &mut |event| collector.handle_event(event))?;
    collector.report();
    Ok(())
}

/// Collects all non-vote transactions for specific slot range
pub struct SlotRangeCollector {
    /// Start of slot range to collect data for.
    start: Slot,
    /// End of slot range (inclusive) to collect data for.
    end: Slot,
    /// Packets in the range separated by slot.
    packets: Vec<(Slot, Vec<ImmutableDeserializedPacket>)>,

    /// Packets being accumulated before we know the slot.
    pending_packets: Vec<ImmutableDeserializedPacket>,
}

impl SlotRangeCollector {
    fn new(start: Slot, end: Slot) -> Self {
        Self {
            start,
            end,
            packets: Vec::new(),
            pending_packets: Vec::new(),
        }
    }

    fn report(&self) {
        for (slot, packets) in self.packets.iter() {
            println!("{slot}: [");
            for packet in packets {
                let ip = packet.original_packet().meta().addr;
                // TODO: verbosity
                let priority = packet.priority();
                let compute_units = packet.compute_unit_limit();

                let transaction = packet.transaction();
                let Some(signature) = transaction.get_signatures().first().copied() else {
                    // Should never fail here because sigverify?
                    continue;
                };
                let message = &transaction.get_message().message;
                let account_keys = message.static_account_keys();
                let write_keys: Vec<_> = account_keys
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| message.is_maybe_writable(*index))
                    .map(|(_, k)| *k)
                    .collect();
                let read_keys: Vec<_> = account_keys
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !message.is_maybe_writable(*index))
                    .map(|(_, k)| *k)
                    .collect();

                println!("  {signature}: ({ip}, {priority}, {compute_units}) - [{write_keys:?}] [{read_keys:?}],");

                // println!("  {:?},", packet);
            }
            println!("]");
        }
    }

    fn handle_event(&mut self, TimedTracedEvent(timestamp, event): TimedTracedEvent) {
        match event {
            TracedEvent::PacketBatch(label, packets) => {
                self.handle_packets(timestamp, label, packets)
            }
            TracedEvent::BlockAndBankHash(slot, _, _) => self.handle_slot(timestamp, slot),
        }
    }

    fn handle_packets(
        &mut self,
        timestamp: SystemTime,
        label: ChannelLabel,
        banking_packet_batch: BankingPacketBatch,
    ) {
        if !matches!(label, ChannelLabel::NonVote) {
            return;
        }
        if banking_packet_batch.0.is_empty() {
            return;
        }

        let packets = banking_packet_batch
            .0
            .iter()
            .flatten()
            .cloned()
            .filter_map(|p| ImmutableDeserializedPacket::new(p).ok());
        self.pending_packets.extend(packets);
    }

    fn handle_slot(&mut self, timestamp: SystemTime, slot: Slot) {
        if slot < self.start || slot > self.end {
            self.pending_packets.clear();
            return;
        }

        self.packets
            .push((slot, core::mem::take(&mut self.pending_packets)));
    }
}
