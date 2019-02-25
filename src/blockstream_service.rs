//! The `blockstream_service` implements optional streaming of entries and block metadata
//! using the `blockstream` module, providing client services such as a block explorer with
//! real-time access to entries.

#[cfg(test)]
use crate::blockstream::MockBlockstream as Blockstream;
#[cfg(not(test))]
use crate::blockstream::SocketBlockstream as Blockstream;
use crate::blockstream::{BlockData, BlockstreamEvents};
use crate::entry::{EntryReceiver, EntrySender};
use crate::result::{Error, Result};
use crate::service::Service;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
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
        exit: Arc<AtomicBool>,
    ) -> (Self, EntryReceiver) {
        let (blockstream_sender, blockstream_receiver) = channel();
        let mut blockstream = Blockstream::new(blockstream_socket);
        let t_blockstream = Builder::new()
            .name("solana-blockstream".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = Self::process_entries(
                    &ledger_entry_receiver,
                    &blockstream_sender,
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
        blockstream: &mut Blockstream,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let entries_with_meta = ledger_entry_receiver.recv_timeout(timeout)?;

        for entry_meta in &entries_with_meta {
            if entry_meta.entry.is_tick() && blockstream.queued_block.is_some() {
                let queued_block = blockstream.queued_block.as_ref();
                let block_slot = queued_block.unwrap().slot;
                let block_tick_height = queued_block.unwrap().tick_height;
                let block_id = queued_block.unwrap().id;
                let block_leader = queued_block.unwrap().leader_id;
                blockstream
                    .emit_block_event(block_slot, block_tick_height, block_leader, block_id)
                    .unwrap_or_else(|e| {
                        debug!("Blockstream error: {:?}, {:?}", e, blockstream.output);
                    });
                blockstream.queued_block = None;
            }
            blockstream
                .emit_entry_event(
                    entry_meta.slot,
                    entry_meta.tick_height,
                    entry_meta.slot_leader,
                    &entry_meta.entry,
                )
                .unwrap_or_else(|e| {
                    debug!("Blockstream error: {:?}, {:?}", e, blockstream.output);
                });
            if 0 == entry_meta.num_ticks_left_in_slot {
                blockstream.queued_block = Some(BlockData {
                    slot: entry_meta.slot,
                    tick_height: entry_meta.tick_height,
                    id: entry_meta.entry.id,
                    leader_id: entry_meta.slot_leader,
                });
            }
        }

        blockstream_sender.send(entries_with_meta)?;
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
    use crate::entry::{Entry, EntryMeta};
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;

    #[test]
    fn test_blockstream_service_process_entries() {
        let ticks_per_slot = 5;
        let leader_id = Keypair::new().pubkey();

        // Set up blockstream
        let mut blockstream = Blockstream::new("test_stream".to_string());

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
            let slot_height = x / ticks_per_slot;
            let parent_slot = if slot_height > 0 {
                Some(slot_height - 1)
            } else {
                None
            };
            let entry_meta = EntryMeta {
                tick_height: x,
                slot: slot_height,
                slot_leader: leader_id,
                num_ticks_left_in_slot: ticks_per_slot - ((x + 1) % ticks_per_slot),
                parent_slot,
                entry,
            };
            expected_entries.push(entry_meta.clone());
            expected_tick_heights.push(x);
            entries.push(entry_meta);
        }
        let keypair = Keypair::new();
        let tx = SystemTransaction::new_account(&keypair, keypair.pubkey(), 1, Hash::default(), 0);
        let entry = Entry::new(&mut last_id, 1, vec![tx]);
        let entry_meta = EntryMeta {
            tick_height: ticks_per_slot - 1,
            slot: 0,
            slot_leader: leader_id,
            num_ticks_left_in_slot: 0,
            parent_slot: None,
            entry,
        };
        expected_entries.insert(ticks_per_slot as usize, entry_meta.clone());
        expected_tick_heights.insert(
            ticks_per_slot as usize,
            ticks_per_slot - 1, // Populated entries should share the tick height of the previous tick.
        );
        entries.insert(ticks_per_slot as usize, entry_meta);

        ledger_entry_sender.send(entries).unwrap();
        BlockstreamService::process_entries(
            &ledger_entry_receiver,
            &blockstream_sender,
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
                assert_eq!(entry, expected_entries[i].entry);
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
