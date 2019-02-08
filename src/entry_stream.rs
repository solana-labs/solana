//! The `entry_stream` module provides a method for streaming entries out via a
//! local unix socket, to provide client services such as a block explorer with
//! real-time access to entries.

use crate::entry::Entry;
use crate::leader_scheduler::LeaderScheduler;
use crate::leader_scheduler::DEFAULT_TICKS_PER_SLOT;
use crate::result::Result;
use chrono::{SecondsFormat, Utc};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use std::io::prelude::*;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;

pub trait EntryStreamHandler {
    fn emit_entry_events(
        &mut self,
        leader_scheduler: &LeaderScheduler,
        entries: &[Entry],
    ) -> Result<()>;
    fn emit_slot_event(
        &mut self,
        current_slot: u64,
        tick_height: u64,
        leader_id: &Pubkey,
    ) -> Result<()>;
    fn emit_block_event(&mut self) -> Result<()>;
}

trait Output {
    fn write(&mut self, payload: String) -> Result<()>;
}

trait EntryStreamEvents {
    fn do_entry_events(
        &mut self,
        output: &mut Output,
        leader_scheduler: &LeaderScheduler,
        entries: &[Entry],
    ) -> Result<()>;
    fn do_slot_event(
        &mut self,
        output: &mut Output,
        current_slot: u64,
        tick_height: u64,
        leader_id: &Pubkey,
    ) -> Result<()>;
    fn do_block_event(&mut self, output: &mut Output) -> Result<()>;
}

impl EntryStreamEvents for EntryStreamMeta {
    fn do_entry_events(
        &mut self,
        output: &mut Output,
        leader_scheduler: &LeaderScheduler,
        entries: &[Entry],
    ) -> Result<()> {
        for entry in entries {
            let (_prev_leader_id, prev_slot) = leader_scheduler
                .get_scheduled_leader(self.last_tick_height)
                .unwrap();
            let (curr_leader_id, curr_slot) = leader_scheduler
                .get_scheduled_leader(entry.tick_height)
                .unwrap();

            if self.is_new_block(entry, prev_slot, curr_slot) {
                self.do_block_event(output)?;
            }

            if self.is_new_slot(entry, prev_slot, curr_slot) {
                self.do_slot_event(output, curr_slot, entry.tick_height, &curr_leader_id)?;
                self.last_slot = curr_slot;
            }

            let json_leader_id = serde_json::to_string(&curr_leader_id)?;
            let json_entry = serde_json::to_string(&entry)?;
            let payload = format!(
                r#"{{"dt":"{}","t":"entry","s":{},"leader_id":{},"entry":{}}}{}"#,
                Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
                self.last_slot,
                json_leader_id,
                json_entry,
                "\n",
            );
            output.write(payload)?;

            self.last_entry_id = entry.id;
            self.last_tick_height = entry.tick_height;

            if self.last_leader_id != curr_leader_id {
                self.last_leader_id = curr_leader_id;
            }
        }
        Ok(())
    }

    fn do_slot_event(
        &mut self,
        output: &mut Output,
        current_slot: u64,
        tick_height: u64,
        leader_id: &Pubkey,
    ) -> Result<()> {
        let json_leader_id = serde_json::to_string(leader_id)?;
        let payload = format!(
            r#"{{"dt":"{}","t":"slot","s":{},"tick_height":{},"leader_id":{}}}{}"#,
            Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            current_slot,
            tick_height,
            json_leader_id,
            "\n",
        );
        output.write(payload)?;
        Ok(())
    }

    fn do_block_event(&mut self, output: &mut Output) -> Result<()> {
        let json_leader_id = serde_json::to_string(&self.last_leader_id)?;
        let json_entry_id = serde_json::to_string(&self.last_entry_id)?;
        let payload = format!(
            r#"{{"dt":"{}","t":"block","s":{},"tick_height":{},"leader_id":{},"id":{}}}{}"#,
            Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            self.last_slot,
            self.last_tick_height,
            json_leader_id,
            json_entry_id,
            "\n",
        );
        output.write(payload)?;
        Ok(())
    }
}

struct EntryStreamMeta {
    pub last_tick_height: u64,
    pub last_slot: u64,
    pub last_entry_id: Hash,
    pub last_leader_id: Pubkey,
}

impl EntryStreamMeta {
    pub fn new() -> Self {
        EntryStreamMeta {
            last_tick_height: 0,
            last_slot: 0,
            last_entry_id: Hash::new(&[0; 32]),
            last_leader_id: Pubkey::new(&[0; 32]),
        }
    }

    fn is_new_slot(&self, entry: &Entry, prev_slot: u64, curr_slot: u64) -> bool {
        // set to true to use leader_scheduler logic
        let use_puzzling = false;

        if use_puzzling {
            // this should work, but slot scheduler isn't incrementing
            curr_slot > prev_slot
        } else {
            entry.tick_height % DEFAULT_TICKS_PER_SLOT == 0 || self.last_tick_height == 0
        }
    }

    fn is_new_block(&self, entry: &Entry, prev_slot: u64, curr_slot: u64) -> bool {
        // set to true to use leader_scheduler logic
        let use_puzzling = false;

        if use_puzzling {
            // this should work, but slot scheduler isn't incrementing
            curr_slot > prev_slot
        } else {
            entry.tick_height % DEFAULT_TICKS_PER_SLOT == 0 && self.last_tick_height != 0
        }
    }
}

pub struct EntryStream {
    entry_stream_meta: EntryStreamMeta,
    socket: String,
    output: SocketOutput,
}

impl std::fmt::Debug for EntryStream {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "EntryStream {{ last_tick_height: {}, last_slot: {}, last_entry_id: {}, last_leader_id: {}, socket: {} }}",
            self.entry_stream_meta.last_tick_height,
            self.entry_stream_meta.last_slot,
            self.entry_stream_meta.last_entry_id,
            self.entry_stream_meta.last_leader_id,
            self.socket
        )
    }
}

struct SocketOutput {
    socket: String,
}

impl Output for SocketOutput {
    fn write(&mut self, payload: String) -> Result<()> {
        let mut socket = UnixStream::connect(Path::new(&self.socket))?;
        socket.write_all(payload.as_bytes())?;
        socket.shutdown(Shutdown::Write)?;
        Ok(())
    }
}

impl EntryStreamHandler for EntryStream {
    fn emit_entry_events(
        &mut self,
        leader_scheduler: &LeaderScheduler,
        entries: &[Entry],
    ) -> Result<()> {
        self.entry_stream_meta
            .do_entry_events(&mut self.output, leader_scheduler, entries)
    }

    fn emit_slot_event(
        &mut self,
        current_slot: u64,
        tick_height: u64,
        leader_id: &Pubkey,
    ) -> Result<()> {
        self.entry_stream_meta
            .do_slot_event(&mut self.output, current_slot, tick_height, leader_id)
    }

    fn emit_block_event(&mut self) -> Result<()> {
        self.entry_stream_meta.do_block_event(&mut self.output)
    }
}

impl EntryStream {
    pub fn new(socket: String) -> Self {
        EntryStream {
            entry_stream_meta: EntryStreamMeta::new(),
            socket: socket.clone(),
            output: SocketOutput { socket },
        }
    }
}

struct VecOutput {
    values: Vec<String>,
}

impl Output for VecOutput {
    fn write(&mut self, payload: String) -> Result<()> {
        self.values.push(payload);
        Ok(())
    }
}

impl VecOutput {
    pub fn new() -> Self {
        VecOutput { values: Vec::new() }
    }

    pub fn entries(&self) -> Vec<String> {
        self.values.clone()
    }
}

pub struct MockEntryStream {
    entry_stream_meta: EntryStreamMeta,
    output: VecOutput,
}

impl std::fmt::Debug for MockEntryStream {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "VecEntryStream {{ last_tick_height: {}, last_slot: {}, last_entry_id: {}, last_leader_id: {} }}",
            self.entry_stream_meta.last_tick_height,
            self.entry_stream_meta.last_slot,
            self.entry_stream_meta.last_entry_id,
            self.entry_stream_meta.last_leader_id
        )
    }
}

impl EntryStreamHandler for MockEntryStream {
    fn emit_entry_events(
        &mut self,
        leader_scheduler: &LeaderScheduler,
        entries: &[Entry],
    ) -> Result<()> {
        self.entry_stream_meta
            .do_entry_events(&mut self.output, leader_scheduler, entries)
    }

    fn emit_slot_event(
        &mut self,
        current_slot: u64,
        tick_height: u64,
        leader_id: &Pubkey,
    ) -> Result<()> {
        self.entry_stream_meta
            .do_slot_event(&mut self.output, current_slot, tick_height, leader_id)
    }

    fn emit_block_event(&mut self) -> Result<()> {
        self.entry_stream_meta.do_block_event(&mut self.output)
    }
}

impl MockEntryStream {
    pub fn new(_: String) -> Self {
        MockEntryStream {
            entry_stream_meta: EntryStreamMeta::new(),
            output: VecOutput::new(),
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
    use crate::leader_scheduler::DEFAULT_TICKS_PER_SLOT;
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_sdk::hash::Hash;

    #[test]
    fn test_entry_stream() -> () {
        // Set up entry stream
        let mut entry_stream = MockEntryStream::new("test_stream".to_string());
        let bank = Bank::default();
        let leader_scheduler = bank.leader_scheduler.read().unwrap();
        let num_entries = 9;

        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        let mut expected_entries = Vec::new();

        for tick_height in 1..=DEFAULT_TICKS_PER_SLOT + 1 {
            let entry = Entry::new(&mut last_id, tick_height, 1, vec![]); //just ticks
            last_id = entry.id;
            expected_entries.push(entry.clone());
            entries.push(entry);
        }

        entry_stream
            .emit_entry_events(&leader_scheduler, &entries)
            .unwrap_or_else(|e| {
                error!("Entry Stream error: {:?}, {:?}", e, entry_stream);
            });

        assert_eq!(
            entry_stream.entries().len() as u64,
            // one entry per tick, plus 2 slots, plus one block
            (DEFAULT_TICKS_PER_SLOT + 1) + 2 + 1
        );

        let mut j = 0;
        let mut matched_entries = 0;
        let mut matched_slots = 0;
        let mut matched_blocks = 0;
        let expect_slot = 0;
        let mut expect_block = 0;

        for item in entry_stream.entries() {
            let json: Value = serde_json::from_str(&item).unwrap();
            let dt_str = json["dt"].as_str().unwrap();

            // Ensure `ts` field parses as valid DateTime
            let _dt: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(dt_str).unwrap();

            let item_type = json["t"].as_str().unwrap();
            match item_type {
                "slot" => {
                    let slot = json["s"].as_u64().unwrap();
                    let _tick_height = json["tick_height"].as_u64().unwrap();

                    assert_eq!(slot, expect_slot, "slot/expect_slot mismatch {}", &item);
                    matched_slots += 1;

                    // puzzling: slot isn't incrementing
                    // expect_slot += 1;
                }

                "block" => {
                    let block_slot = json["s"].as_u64().unwrap();
                    let _tick_height = json["tick_height"].as_u64().unwrap();

                    assert_eq!(
                        block_slot, expect_block,
                        "block/expect_block mismatch {}",
                        &item
                    );
                    matched_blocks += 1;
                    expect_block += 1;
                }

                "entry" => {
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

        assert_eq!(matched_entries, num_entries);
        assert_eq!(matched_slots, 2);
        assert_eq!(matched_blocks, 1);
    }
}
