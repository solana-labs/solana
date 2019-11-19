//! The `blockstream_service` implements optional streaming of entries and block metadata
//! using the `blockstream` module, providing client services such as a block explorer with
//! real-time access to entries.

use crate::blockstream::BlockstreamEvents;
#[cfg(test)]
use crate::blockstream::MockBlockstream as Blockstream;
#[cfg(not(test))]
use crate::blockstream::SocketBlockstream as Blockstream;
use crate::result::{Error, Result};
use solana_ledger::blocktree::Blocktree;
use solana_sdk::pubkey::Pubkey;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

pub struct BlockstreamService {
    t_blockstream: JoinHandle<()>,
}

impl BlockstreamService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        slot_full_receiver: Receiver<(u64, Pubkey)>,
        blocktree: Arc<Blocktree>,
        unix_socket: &Path,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let mut blockstream = Blockstream::new(unix_socket);
        let exit = exit.clone();
        let t_blockstream = Builder::new()
            .name("solana-blockstream".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) =
                    Self::process_entries(&slot_full_receiver, &blocktree, &mut blockstream)
                {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => info!("Error from process_entries: {:?}", e),
                    }
                }
            })
            .unwrap();
        Self { t_blockstream }
    }
    fn process_entries(
        slot_full_receiver: &Receiver<(u64, Pubkey)>,
        blocktree: &Arc<Blocktree>,
        blockstream: &mut Blockstream,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let (slot, slot_leader) = slot_full_receiver.recv_timeout(timeout)?;

        let entries = blocktree.get_slot_entries(slot, 0, None).unwrap();
        let blocktree_meta = blocktree.meta(slot).unwrap().unwrap();
        let _parent_slot = if slot == 0 {
            None
        } else {
            Some(blocktree_meta.parent_slot)
        };
        let ticks_per_slot = entries.iter().filter(|entry| entry.is_tick()).count() as u64;
        let mut tick_height = ticks_per_slot * slot;

        for (i, entry) in entries.iter().enumerate() {
            if entry.is_tick() {
                tick_height += 1;
            }
            blockstream
                .emit_entry_event(slot, tick_height, &slot_leader, &entry)
                .unwrap_or_else(|e| {
                    debug!("Blockstream error: {:?}, {:?}", e, blockstream.output);
                });
            if i == entries.len() - 1 {
                blockstream
                    .emit_block_event(slot, tick_height, &slot_leader, entry.hash)
                    .unwrap_or_else(|e| {
                        debug!("Blockstream error: {:?}, {:?}", e, blockstream.output);
                    });
            }
        }
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_blockstream.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::genesis_utils::{create_genesis_config, GenesisConfigInfo};
    use bincode::{deserialize, serialize};
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_ledger::create_new_tmp_ledger;
    use solana_ledger::entry::{create_ticks, Entry};
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use std::path::PathBuf;
    use std::sync::mpsc::channel;

    #[test]
    fn test_blockstream_service_process_entries() {
        let ticks_per_slot = 5;
        let leader_pubkey = Pubkey::new_rand();

        // Set up genesis config and blocktree
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(1000);
        genesis_config.ticks_per_slot = ticks_per_slot;

        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blocktree = Blocktree::open(&ledger_path).unwrap();

        // Set up blockstream
        let mut blockstream = Blockstream::new(&PathBuf::from("test_stream"));

        // Set up dummy channel to receive a full-slot notification
        let (slot_full_sender, slot_full_receiver) = channel();

        // Create entries - 4 ticks + 1 populated entry + 1 tick
        let mut entries = create_ticks(4, 0, Hash::default());

        let keypair = Keypair::new();
        let mut blockhash = entries[3].hash;
        let tx = system_transaction::transfer(&keypair, &keypair.pubkey(), 1, Hash::default());
        let entry = Entry::new(&mut blockhash, 1, vec![tx]);
        blockhash = entry.hash;
        entries.push(entry);
        let final_tick = create_ticks(1, 0, blockhash);
        entries.extend_from_slice(&final_tick);

        let expected_entries = entries.clone();
        let expected_tick_heights = [6, 7, 8, 9, 9, 10];

        blocktree
            .write_entries(
                1,
                0,
                0,
                ticks_per_slot,
                None,
                true,
                &Arc::new(Keypair::new()),
                entries,
                0,
            )
            .unwrap();

        slot_full_sender.send((1, leader_pubkey)).unwrap();
        BlockstreamService::process_entries(
            &slot_full_receiver,
            &Arc::new(blocktree),
            &mut blockstream,
        )
        .unwrap();
        assert_eq!(blockstream.entries().len(), 7);

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
            let entry: Entry;
            if tx.len() == 0 {
                entry = serde_json::from_value(entry_obj).unwrap();
            } else {
                let entry_json = entry_obj.as_object().unwrap();
                entry = Entry {
                    num_hashes: entry_json.get("num_hashes").unwrap().as_u64().unwrap(),
                    hash: serde_json::from_value(entry_json.get("hash").unwrap().clone()).unwrap(),
                    transactions: entry_json
                        .get("transactions")
                        .unwrap()
                        .as_array()
                        .unwrap()
                        .into_iter()
                        .enumerate()
                        .map(|(j, tx)| {
                            let tx_vec: Vec<u8> = serde_json::from_value(tx.clone()).unwrap();
                            // Check explicitly that transaction matches bincode-serialized format
                            assert_eq!(
                                tx_vec,
                                serialize(&expected_entries[i].transactions[j]).unwrap()
                            );
                            deserialize(&tx_vec).unwrap()
                        })
                        .collect(),
                };
            }
            assert_eq!(entry, expected_entries[i]);
        }
        for json in block_events {
            let slot = json["s"].as_u64().unwrap();
            assert_eq!(1, slot);
            let height = json["h"].as_u64().unwrap();
            assert_eq!(2 * ticks_per_slot, height);
        }
    }
}
