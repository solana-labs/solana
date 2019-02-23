//! The `blockstream_service` implements optional streaming of entries and block metadata
//! using the `blockstream` module, providing client services such as a block explorer with
//! real-time access to entries.

#[cfg(test)]
use crate::blockstream::MockBlockstream as Blockstream;
#[cfg(not(test))]
use crate::blockstream::SocketBlockstream as Blockstream;
use crate::blockstream::{BlockData, BlockstreamEvents};
use crate::entry::{EntryReceiver, EntrySender};
use crate::leader_scheduler::LeaderScheduler;
use crate::result::{Error, Result};
use crate::service::Service;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

pub struct BlockstreamService {
    t_blockstream: JoinHandle<()>,
}

impl BlockstreamService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        ledger_entry_receiver: EntryReceiver,
        blockstream_socket: String,
        mut tick_height: u64,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        exit: Arc<AtomicBool>,
    ) -> (Self, EntryReceiver) {
        let (blockstream_sender, blockstream_receiver) = channel();
        let mut blockstream = Blockstream::new(blockstream_socket, leader_scheduler);
        let t_blockstream = Builder::new()
            .name("solana-blockstream".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = Self::process_entries(
                    &ledger_entry_receiver,
                    &blockstream_sender,
                    &mut tick_height,
                    &mut blockstream,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => info!("Error from process_entries: {:?}", e),
                    }
                }
            })
            .unwrap();
        (Self { t_blockstream }, blockstream_receiver)
    }
    fn process_entries(
        ledger_entry_receiver: &EntryReceiver,
        blockstream_sender: &EntrySender,
        tick_height: &mut u64,
        blockstream: &mut Blockstream,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let entries = ledger_entry_receiver.recv_timeout(timeout)?;
        let leader_scheduler = blockstream.leader_scheduler.read().unwrap();

        for entry in &entries {
            if entry.is_tick() {
                *tick_height += 1
            }
            let slot = leader_scheduler.tick_height_to_slot(*tick_height);
            let leader_id = leader_scheduler
                .get_leader_for_slot(slot)
                .map(|leader| leader.to_string())
                .unwrap_or_else(|| "None".to_string());

            if entry.is_tick() && blockstream.queued_block.is_some() {
                let queued_block = blockstream.queued_block.as_ref();
                let block_slot = queued_block.unwrap().slot;
                let block_tick_height = queued_block.unwrap().tick_height;
                let block_id = queued_block.unwrap().id;
                blockstream
                    .emit_block_event(block_slot, block_tick_height, &leader_id, block_id)
                    .unwrap_or_else(|e| {
                        debug!("Blockstream error: {:?}, {:?}", e, blockstream.output);
                    });
                blockstream.queued_block = None;
            }
            blockstream
                .emit_entry_event(slot, *tick_height, &leader_id, &entry)
                .unwrap_or_else(|e| {
                    debug!("Blockstream error: {:?}, {:?}", e, blockstream.output);
                });
            if 0 == leader_scheduler.num_ticks_left_in_slot(*tick_height) {
                blockstream.queued_block = Some(BlockData {
                    slot,
                    tick_height: *tick_height,
                    id: entry.id,
                });
            }
        }

        blockstream_sender.send(entries)?;
        Ok(())
    }
}

impl Service for BlockstreamService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_blockstream.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::entry::Entry;
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;

    #[test]
    fn test_blockstream_stage_process_entries() {
        // Set up the bank and leader_scheduler
        let ticks_per_slot = 5;
        let starting_tick_height = 1;

        let (mut genesis_block, _mint_keypair) = GenesisBlock::new(1_000_000);
        genesis_block.ticks_per_slot = ticks_per_slot;
        genesis_block.slots_per_epoch = 2;

        let bank = Bank::new(&genesis_block);
        let leader_scheduler = LeaderScheduler::new_with_bank(&bank);
        let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));

        // Set up blockstream
        let mut blockstream = Blockstream::new("test_stream".to_string(), leader_scheduler.clone());

        // Set up dummy channels to host an BlockstreamService
        let (ledger_entry_sender, ledger_entry_receiver) = channel();
        let (blockstream_sender, blockstream_receiver) = channel();

        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        let mut expected_entries = Vec::new();
        let mut expected_tick_heights = Vec::new();
        for x in 0..6 {
            let entry = Entry::new(&mut last_id, 1, vec![]); //just ticks
            last_id = entry.id;
            expected_entries.push(entry.clone());
            expected_tick_heights.push(starting_tick_height + x);
            entries.push(entry);
        }
        let keypair = Keypair::new();
        let tx = SystemTransaction::new_account(&keypair, keypair.pubkey(), 1, Hash::default(), 0);
        let entry = Entry::new(&mut last_id, 1, vec![tx]);
        expected_entries.insert(ticks_per_slot as usize, entry.clone());
        expected_tick_heights.insert(
            ticks_per_slot as usize,
            starting_tick_height + ticks_per_slot - 1, // Populated entries should share the tick height of the previous tick.
        );
        entries.insert(ticks_per_slot as usize, entry);

        ledger_entry_sender.send(entries).unwrap();
        BlockstreamService::process_entries(
            &ledger_entry_receiver,
            &blockstream_sender,
            &mut (starting_tick_height - 1),
            &mut blockstream,
        )
        .unwrap();
        assert_eq!(blockstream.entries().len(), 8);

        let (entry_events, block_events): (Vec<Value>, Vec<Value>) = blockstream
            .entries()
            .iter()
            .map(|item| {
                let json: Value = serde_json::from_str(&item).unwrap();
                let dt_str = json["dt"].as_str().unwrap();
                // Ensure `ts` field parses as valid DateTime
                let _dt: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(dt_str).unwrap();
                json
            })
            .partition(|json| {
                let item_type = json["t"].as_str().unwrap();
                item_type == "entry"
            });
        for (i, json) in entry_events.iter().enumerate() {
            let height = json["h"].as_u64().unwrap();
            assert_eq!(height, expected_tick_heights[i]);
            let entry_obj = json["entry"].clone();
            let tx = entry_obj["transactions"].as_array().unwrap();
            if tx.len() == 0 {
                // TODO: There is a bug in Transaction deserialize methods such that
                // `serde_json::from_str` does not work for populated Entries.
                // Remove this `if` when fixed.
                let entry: Entry = serde_json::from_value(entry_obj).unwrap();
                assert_eq!(entry, expected_entries[i]);
            }
        }
        for json in block_events {
            let slot = json["s"].as_u64().unwrap();
            assert_eq!(0, slot);
            let height = json["h"].as_u64().unwrap();
            assert_eq!(ticks_per_slot - 1, height);
        }

        // Ensure entries pass through stage unadulterated
        let recv_entries = blockstream_receiver.recv().unwrap();
        assert_eq!(expected_entries, recv_entries);
    }
}
