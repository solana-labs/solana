use {
    crate::process::process_event_files,
    solana_core::{
        banking_stage::immutable_deserialized_packet::ImmutableDeserializedPacket,
        banking_trace::{BankingPacketBatch, ChannelLabel, TimedTracedEvent, TracedEvent},
    },
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::path::PathBuf,
};

pub fn do_account_scan(event_file_paths: &[PathBuf], pubkey: Pubkey) -> std::io::Result<()> {
    let mut scanner = AccountScanner::new(pubkey);
    process_event_files(event_file_paths, &mut |event| scanner.handle_event(event))?;
    Ok(())
}

struct AccountScanner {
    pubkey: Pubkey,
    current_pending: Vec<ImmutableDeserializedPacket>,
}

impl AccountScanner {
    fn new(pubkey: Pubkey) -> Self {
        Self {
            pubkey,
            current_pending: vec![],
        }
    }

    fn handle_event(&mut self, TimedTracedEvent(_, event): TimedTracedEvent) {
        match event {
            TracedEvent::PacketBatch(label, banking_packet_batch) => {
                self.handle_packets(label, banking_packet_batch)
            }
            TracedEvent::BlockAndBankHash(slot, _, _) => self.handle_slot_end(slot),
        }
    }

    fn handle_packets(&mut self, label: ChannelLabel, banking_packet_batch: BankingPacketBatch) {
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
            .filter_map(|p| ImmutableDeserializedPacket::new(p).ok())
            .filter(|p| {
                p.transaction()
                    .get_message()
                    .message
                    .static_account_keys()
                    .iter()
                    .any(|k| *k == self.pubkey)
            });
        self.current_pending.extend(packets);
    }

    fn handle_slot_end(&mut self, slot: Slot) {
        let current_pending = core::mem::take(&mut self.current_pending);
        if current_pending.is_empty() {
            return;
        }

        println!("{}: {}", slot, current_pending.len());
    }
}
