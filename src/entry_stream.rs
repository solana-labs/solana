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

pub trait EntryWriter: std::fmt::Debug {
    fn write(&self, payload: String) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct EntryVec {
    values: RefCell<Vec<String>>,
}

impl EntryWriter for EntryVec {
    fn write(&self, payload: String) -> Result<()> {
        self.values.borrow_mut().push(payload);
        Ok(())
    }
}

impl EntryVec {
    pub fn new() -> Self {
        EntryVec {
            values: RefCell::new(Vec::new()),
        }
    }

    pub fn entries(&self) -> Vec<String> {
        self.values.borrow().clone()
    }
}

#[derive(Debug)]
pub struct EntrySocket {
    socket: String,
}

const MESSAGE_TERMINATOR: &str = "\n";

impl EntryWriter for EntrySocket {
    fn write(&self, payload: String) -> Result<()> {
        let mut socket = UnixStream::connect(Path::new(&self.socket))?;
        socket.write_all(payload.as_bytes())?;
        socket.write_all(MESSAGE_TERMINATOR.as_bytes())?;
        socket.shutdown(Shutdown::Write)?;
        Ok(())
    }
}

pub trait EntryStreamHandler {
    fn emit_entry_event(&self, slot: u64, leader_id: &str, entries: &Entry) -> Result<()>;
    fn emit_block_event(
        &self,
        slot: u64,
        leader_id: &str,
        tick_height: u64,
        last_id: Hash,
    ) -> Result<()>;
}

#[derive(Debug)]
pub struct EntryStream<T: EntryWriter> {
    pub output: T,
    pub leader_scheduler: Arc<RwLock<LeaderScheduler>>,
    pub queued_block: Option<EntryStreamBlock>,
}

impl<T> EntryStreamHandler for EntryStream<T>
where
    T: EntryWriter,
{
    fn emit_entry_event(&self, slot: u64, leader_id: &str, entry: &Entry) -> Result<()> {
        let json_entry = serde_json::to_string(&entry)?;
        let payload = format!(
            r#"{{"dt":"{}","t":"entry","s":{},"l":{:?},"entry":{}}}"#,
            Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            slot,
            leader_id,
            json_entry,
        );
        self.output.write(payload)?;
        Ok(())
    }

    fn emit_block_event(
        &self,
        slot: u64,
        leader_id: &str,
        tick_height: u64,
        last_id: Hash,
    ) -> Result<()> {
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

pub type SocketEntryStream = EntryStream<EntrySocket>;

impl SocketEntryStream {
    pub fn new(socket: String, leader_scheduler: Arc<RwLock<LeaderScheduler>>) -> Self {
        EntryStream {
            output: EntrySocket { socket },
            leader_scheduler,
            queued_block: None,
        }
    }
}

pub type MockEntryStream = EntryStream<EntryVec>;

impl MockEntryStream {
    pub fn new(_: String, leader_scheduler: Arc<RwLock<LeaderScheduler>>) -> Self {
        EntryStream {
            output: EntryVec::new(),
            leader_scheduler,
            queued_block: None,
        }
    }

    pub fn entries(&self) -> Vec<String> {
        self.output.entries()
    }
}

#[derive(Debug)]
pub struct EntryStreamBlock {
    pub slot: u64,
    pub tick_height: u64,
    pub id: Hash,
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
        let leader_scheduler =
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config)));
        let bank = Bank::new_with_leader_scheduler(&genesis_block, leader_scheduler.clone());
        // Set up entry stream
        let entry_stream =
            MockEntryStream::new("test_stream".to_string(), leader_scheduler.clone());
        let ticks_per_slot = leader_scheduler.read().unwrap().ticks_per_slot;

        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        let mut expected_entries = Vec::new();

        let tick_height_initial = 0;
        let tick_height_final = tick_height_initial + ticks_per_slot + 2;
        let mut previous_slot = leader_scheduler
            .read()
            .unwrap()
            .tick_height_to_slot(tick_height_initial);
        let leader_id = leader_scheduler
            .read()
            .unwrap()
            .get_leader_for_slot(previous_slot)
            .map(|leader| leader.to_string())
            .unwrap_or_else(|| "None".to_string());

        for tick_height in tick_height_initial..=tick_height_final {
            leader_scheduler
                .write()
                .unwrap()
                .update_tick_height(tick_height, &bank);
            let curr_slot = leader_scheduler
                .read()
                .unwrap()
                .tick_height_to_slot(tick_height);
            if curr_slot != previous_slot {
                entry_stream
                    .emit_block_event(previous_slot, &leader_id, tick_height - 1, last_id)
                    .unwrap();
            }
            let entry = Entry::new(&mut last_id, tick_height, 1, vec![]); // just ticks
            last_id = entry.id;
            previous_slot = curr_slot;
            entry_stream
                .emit_entry_event(curr_slot, &leader_id, &entry)
                .unwrap();
            expected_entries.push(entry.clone());
            entries.push(entry);
        }

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
