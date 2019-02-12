//! The `entry_stream` module provides a method for streaming entries out via a
//! local unix socket, to provide client services such as a block explorer with
//! real-time access to entries.

use crate::entry::Entry;
use crate::leader_scheduler::LeaderScheduler;
use crate::result::Result;
use chrono::{SecondsFormat, Utc};
use solana_sdk::hash::Hash;
use std::cell::RefCell;
use std::io::prelude::*;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, RwLock};

pub trait Output: std::fmt::Debug {
    fn write(&self, payload: String) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct VecOutput {
    values: RefCell<Vec<String>>,
}

impl Output for VecOutput {
    fn write(&self, payload: String) -> Result<()> {
        self.values.borrow_mut().push(payload);
        Ok(())
    }
}

impl VecOutput {
    pub fn new() -> Self {
        VecOutput {
            values: RefCell::new(Vec::new()),
        }
    }

    pub fn entries(&self) -> Vec<String> {
        self.values.borrow().clone()
    }
}

#[derive(Debug)]
pub struct SocketOutput {
    socket: String,
}

impl Output for SocketOutput {
    fn write(&self, payload: String) -> Result<()> {
        let mut socket = UnixStream::connect(Path::new(&self.socket))?;
        socket.write_all(payload.as_bytes())?;
        socket.shutdown(Shutdown::Write)?;
        Ok(())
    }
}

pub trait EntryStreamHandler {
    fn emit_entry_events(&self, entries: &[Entry]) -> Result<()>;
    fn emit_block_event(&self, tick_height: u64, last_id: Hash) -> Result<()>;
}

#[derive(Debug)]
pub struct EntryStream<T: Output> {
    pub output: T,
    leader_scheduler: Arc<RwLock<LeaderScheduler>>,
}

impl<T> EntryStreamHandler for EntryStream<T>
where
    T: Output,
{
    fn emit_entry_events(&self, entries: &[Entry]) -> Result<()> {
        let leader_scheduler = self.leader_scheduler.read().unwrap();
        for entry in entries {
            let slot = leader_scheduler.tick_height_to_slot(entry.tick_height);
            let leader_id = leader_scheduler
                .get_leader_for_slot(slot)
                .map(|leader| leader.to_string())
                .unwrap_or_else(|| "None".to_string());

            let json_entry = serde_json::to_string(&entry)?;
            let payload = format!(
                r#"{{"dt":"{}","t":"entry","s":{},"l":{:?},"entry":{}}}"#,
                Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
                slot,
                leader_id,
                json_entry,
            );
            self.output.write(payload)?;
        }
        Ok(())
    }

    fn emit_block_event(&self, tick_height: u64, last_id: Hash) -> Result<()> {
        let leader_scheduler = self.leader_scheduler.read().unwrap();
        let slot = leader_scheduler.tick_height_to_slot(tick_height);
        let leader_id = leader_scheduler
            .get_leader_for_slot(slot)
            .map(|leader| leader.to_string())
            .unwrap_or_else(|| "None".to_string());
        let payload = format!(
            r#"{{"dt":"{}","t":"block","s":{},"h":{},"l":{:?},"id":"{:?}"}}"#,
            Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            slot,
            tick_height,
            leader_id,
            last_id,
        );
        self.output.write(payload)?;
        Ok(())
    }
}

pub type SocketEntryStream = EntryStream<SocketOutput>;

impl SocketEntryStream {
    pub fn new(socket: String, leader_scheduler: Arc<RwLock<LeaderScheduler>>) -> Self {
        EntryStream {
            output: SocketOutput { socket },
            leader_scheduler,
        }
    }
}

pub type MockEntryStream = EntryStream<VecOutput>;

impl MockEntryStream {
    pub fn new(_: String, leader_scheduler: Arc<RwLock<LeaderScheduler>>) -> Self {
        EntryStream {
            output: VecOutput::new(),
            leader_scheduler,
        }
    }

    pub fn entries(&self) -> Vec<String> {
        self.output.entries()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bank::Bank;
    use crate::entry::Entry;
    use crate::genesis_block::GenesisBlock;
    use crate::leader_scheduler::LeaderSchedulerConfig;
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_sdk::hash::Hash;
    use std::collections::HashSet;

    #[test]
    fn test_entry_stream() -> () {
        // Set up bank and leader_scheduler
        let leader_scheduler_config = LeaderSchedulerConfig::new(5, 2, 10);
        let (genesis_block, _mint_keypair) = GenesisBlock::new(1_000_000);
        let bank = Bank::new_with_leader_scheduler_config(&genesis_block, &leader_scheduler_config);
        // Set up entry stream
        let entry_stream =
            MockEntryStream::new("test_stream".to_string(), bank.leader_scheduler.clone());
        let ticks_per_slot = bank.leader_scheduler.read().unwrap().ticks_per_slot;

        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        let mut expected_entries = Vec::new();

        let tick_height_initial = 0;
        let tick_height_final = tick_height_initial + ticks_per_slot + 2;
        let mut previous_slot = bank
            .leader_scheduler
            .read()
            .unwrap()
            .tick_height_to_slot(tick_height_initial);

        for tick_height in tick_height_initial..=tick_height_final {
            bank.leader_scheduler
                .write()
                .unwrap()
                .update_tick_height(tick_height, &bank);
            let curr_slot = bank
                .leader_scheduler
                .read()
                .unwrap()
                .tick_height_to_slot(tick_height);
            if curr_slot != previous_slot {
                entry_stream
                    .emit_block_event(tick_height - 1, last_id)
                    .unwrap();
            }
            let entry = Entry::new(&mut last_id, tick_height, 1, vec![]); //just ticks
            last_id = entry.id;
            previous_slot = curr_slot;
            expected_entries.push(entry.clone());
            entries.push(entry);
        }

        entry_stream.emit_entry_events(&entries).unwrap();

        assert_eq!(
            entry_stream.entries().len() as u64,
            // one entry per tick (0..=N+2) is +3, plus one block
            ticks_per_slot + 3 + 1
        );

        let mut j = 0;
        let mut matched_entries = 0;
        let mut matched_slots = HashSet::new();
        let mut matched_blocks = HashSet::new();

        for item in entry_stream.entries() {
            let json: Value = serde_json::from_str(&item).unwrap();
            let dt_str = json["dt"].as_str().unwrap();

            // Ensure `ts` field parses as valid DateTime
            let _dt: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(dt_str).unwrap();

            let item_type = json["t"].as_str().unwrap();
            match item_type {
                "block" => {
                    let id = json["id"].to_string();
                    matched_blocks.insert(id);
                }

                "entry" => {
                    let slot = json["s"].as_u64().unwrap();
                    matched_slots.insert(slot);
                    let entry_obj = json["entry"].clone();
                    let entry: Entry = serde_json::from_value(entry_obj).unwrap();

                    assert_eq!(entry, expected_entries[j]);
                    matched_entries += 1;
                    j += 1;
                }

                _ => {
                    assert!(false, "unknown item type {}", item);
                }
            }
        }

        assert_eq!(matched_entries, expected_entries.len());
        assert_eq!(matched_slots.len(), 2);
        assert_eq!(matched_blocks.len(), 1);
    }
}
