use {
    crate::{block_history::get_block, process::process_event_files},
    solana_core::{
        banking_stage::immutable_deserialized_packet::ImmutableDeserializedPacket,
        banking_trace::{BankingPacketBatch, ChannelLabel, TimedTracedEvent, TracedEvent},
    },
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{collections::HashSet, path::PathBuf},
};

pub fn do_account_scan(
    event_file_paths: &[PathBuf],
    pubkey: Pubkey,
    check_included: bool,
) -> std::io::Result<()> {
    let mut scanner = AccountScanner::new(pubkey, check_included);
    process_event_files(event_file_paths, &mut |event| scanner.handle_event(event))?;
    Ok(())
}

struct AccountScanner {
    pubkey: Pubkey,
    check_included: bool,
    current_pending: Vec<ImmutableDeserializedPacket>,
}

impl AccountScanner {
    fn new(pubkey: Pubkey, check_included: bool) -> Self {
        Self {
            pubkey,
            check_included,
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
        let mut current_pending = core::mem::take(&mut self.current_pending);
        if current_pending.is_empty() {
            return;
        }

        let num_received = current_pending.len();
        if self.check_included {
            let client = RpcClient::new("https://api.mainnet-beta.solana.com");
            let block = get_block(&client, slot).unwrap();

            let included_sigs: HashSet<_> = block
                .transactions
                .unwrap()
                .into_iter()
                .map(|tx| tx.transaction.decode().unwrap().signatures[0])
                .collect();

            current_pending
                .retain(|p| included_sigs.contains(&p.transaction().get_signatures()[0]));
            println!(
                "{}: received={} recorded={}",
                slot,
                num_received,
                current_pending.len()
            );
        } else {
            println!("{}: received={}", slot, num_received);
        }
    }
}
