//! The `entry_stream_stage` implements optional streaming of entries using the
//! `entry_stream` module, providing client services such as a block explorer with
//! real-time access to entries.

use crate::entry::{EntryReceiver, EntrySender};
#[cfg(test)]
use crate::entry_stream::MockEntryStream as EntryStream;
#[cfg(not(test))]
use crate::entry_stream::SocketEntryStream as EntryStream;
use crate::entry_stream::{EntryStreamBlock, EntryStreamHandler};
use crate::leader_scheduler::LeaderScheduler;
use crate::result::{Error, Result};
use crate::service::Service;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

pub struct EntryStreamStage {
    t_entry_stream: JoinHandle<()>,
}

impl EntryStreamStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        ledger_entry_receiver: EntryReceiver,
        entry_stream_socket: String,
        mut tick_height: u64,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        exit: Arc<AtomicBool>,
    ) -> (Self, EntryReceiver) {
        let (entry_stream_sender, entry_stream_receiver) = channel();
        let mut entry_stream = EntryStream::new(entry_stream_socket, leader_scheduler);
        let t_entry_stream = Builder::new()
            .name("solana-entry-stream".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = Self::process_entries(
                    &ledger_entry_receiver,
                    &entry_stream_sender,
                    &mut tick_height,
                    &mut entry_stream,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => info!("Error from process_entries: {:?}", e),
                    }
                }
            })
            .unwrap();
        (Self { t_entry_stream }, entry_stream_receiver)
    }
    fn process_entries(
        ledger_entry_receiver: &EntryReceiver,
        entry_stream_sender: &EntrySender,
        tick_height: &mut u64,
        entry_stream: &mut EntryStream,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let entries = ledger_entry_receiver.recv_timeout(timeout)?;
        let leader_scheduler = entry_stream.leader_scheduler.read().unwrap();

        for entry in &entries {
            if entry.is_tick() {
                *tick_height += 1
            }
            let slot = leader_scheduler.tick_height_to_slot(*tick_height);
            let leader_id = leader_scheduler
                .get_leader_for_slot(slot)
                .map(|leader| leader.to_string())
                .unwrap_or_else(|| "None".to_string());

            if entry.is_tick() && entry_stream.queued_block.is_some() {
                let queued_block = entry_stream.queued_block.as_ref();
                let block_slot = queued_block.unwrap().slot;
                let block_tick_height = queued_block.unwrap().tick_height;
                let block_id = queued_block.unwrap().id;
                entry_stream
                    .emit_block_event(block_slot, &leader_id, block_tick_height, block_id)
                    .unwrap_or_else(|e| {
                        debug!("Entry Stream error: {:?}, {:?}", e, entry_stream.output);
                    });
                entry_stream.queued_block = None;
            }
            entry_stream
                .emit_entry_event(slot, &leader_id, &entry)
                .unwrap_or_else(|e| {
                    debug!("Entry Stream error: {:?}, {:?}", e, entry_stream.output);
                });
            if 0 == leader_scheduler.num_ticks_left_in_slot(*tick_height) {
                entry_stream.queued_block = Some(EntryStreamBlock {
                    slot,
                    tick_height: *tick_height,
                    id: entry.id,
                });
            }
        }

        entry_stream_sender.send(entries)?;
        Ok(())
    }
}

impl Service for EntryStreamStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_entry_stream.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::entry::Entry;
    use crate::leader_scheduler::LeaderSchedulerConfig;
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;

    #[test]
    fn test_entry_stream_stage_process_entries() {
        // Set up the bank and leader_scheduler
        let ticks_per_slot = 5;

        let (genesis_block, _mint_keypair) = GenesisBlock::new(1_000_000);
        let bank = Bank::new(&genesis_block);
        let leader_scheduler_config = LeaderSchedulerConfig::new(ticks_per_slot, 2, 10);
        let leader_scheduler = LeaderScheduler::new_with_bank(&leader_scheduler_config, &bank);
        let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));

        // Set up entry stream
        let mut entry_stream =
            EntryStream::new("test_stream".to_string(), leader_scheduler.clone());

        // Set up dummy channels to host an EntryStreamStage
        let (ledger_entry_sender, ledger_entry_receiver) = channel();
        let (entry_stream_sender, entry_stream_receiver) = channel();

        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        let mut expected_entries = Vec::new();
        for x in 0..6 {
            let entry = Entry::new(&mut last_id, x, 1, vec![]); //just ticks
            last_id = entry.id;
            expected_entries.push(entry.clone());
            entries.push(entry);
        }
        let keypair = Keypair::new();
        let tx = SystemTransaction::new_account(&keypair, keypair.pubkey(), 1, Hash::default(), 0);
        let entry = Entry::new(&mut last_id, ticks_per_slot - 1, 1, vec![tx]);
        expected_entries.insert(ticks_per_slot as usize, entry.clone());
        entries.insert(ticks_per_slot as usize, entry);

        ledger_entry_sender.send(entries).unwrap();
        EntryStreamStage::process_entries(
            &ledger_entry_receiver,
            &entry_stream_sender,
            &mut 1,
            &mut entry_stream,
        )
        .unwrap();
        assert_eq!(entry_stream.entries().len(), 8);

        let (entry_events, block_events): (Vec<Value>, Vec<Value>) = entry_stream
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
        let recv_entries = entry_stream_receiver.recv().unwrap();
        assert_eq!(expected_entries, recv_entries);
    }
}
