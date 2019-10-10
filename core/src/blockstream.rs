//! The `blockstream` module provides a method for streaming entries out via a
//! local unix socket, to provide client services such as a block explorer with
//! real-time access to entries.

use crate::entry::Entry;
use crate::result::Result;
use bincode::serialize;
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

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
    unix_socket: PathBuf,
}

impl EntryWriter for EntrySocket {
    #[cfg(not(windows))]
    fn write(&self, payload: String) -> Result<()> {
        use std::io::prelude::*;
        use std::net::Shutdown;
        use std::os::unix::net::UnixStream;

        const MESSAGE_TERMINATOR: &str = "\n";

        let mut socket = UnixStream::connect(&self.unix_socket)?;
        socket.write_all(payload.as_bytes())?;
        socket.write_all(MESSAGE_TERMINATOR.as_bytes())?;
        socket.shutdown(Shutdown::Write)?;
        Ok(())
    }
    #[cfg(windows)]
    fn write(&self, _payload: String) -> Result<()> {
        Err(crate::result::Error::from(std::io::Error::new(
            std::io::ErrorKind::Other,
            "EntryWriter::write() not implemented for windows",
        )))
    }
}

pub trait BlockstreamEvents {
    fn emit_entry_event(
        &self,
        slot: u64,
        tick_height: u64,
        leader_pubkey: &Pubkey,
        entries: &Entry,
    ) -> Result<()>;
    fn emit_block_event(
        &self,
        slot: u64,
        tick_height: u64,
        leader_pubkey: &Pubkey,
        blockhash: Hash,
    ) -> Result<()>;
}

#[derive(Debug)]
pub struct Blockstream<T: EntryWriter> {
    pub output: T,
}

impl<T> BlockstreamEvents for Blockstream<T>
where
    T: EntryWriter,
{
    fn emit_entry_event(
        &self,
        slot: u64,
        tick_height: u64,
        leader_pubkey: &Pubkey,
        entry: &Entry,
    ) -> Result<()> {
        let transactions: Vec<Vec<u8>> = serialize_transactions(entry);
        let stream_entry = json!({
            "num_hashes": entry.num_hashes,
            "hash": entry.hash,
            "transactions": transactions
        });
        let json_entry = serde_json::to_string(&stream_entry)?;
        let payload = format!(
            r#"{{"dt":"{}","t":"entry","s":{},"h":{},"l":"{:?}","entry":{}}}"#,
            Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            slot,
            tick_height,
            leader_pubkey,
            json_entry,
        );
        self.output.write(payload)?;
        Ok(())
    }

    fn emit_block_event(
        &self,
        slot: u64,
        tick_height: u64,
        leader_pubkey: &Pubkey,
        blockhash: Hash,
    ) -> Result<()> {
        let payload = format!(
            r#"{{"dt":"{}","t":"block","s":{},"h":{},"l":"{:?}","hash":"{:?}"}}"#,
            Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            slot,
            tick_height,
            leader_pubkey,
            blockhash,
        );
        self.output.write(payload)?;
        Ok(())
    }
}

pub type SocketBlockstream = Blockstream<EntrySocket>;

impl SocketBlockstream {
    pub fn new(unix_socket: &Path) -> Self {
        Blockstream {
            output: EntrySocket {
                unix_socket: unix_socket.to_path_buf(),
            },
        }
    }
}

pub type MockBlockstream = Blockstream<EntryVec>;

impl MockBlockstream {
    pub fn new(_: &Path) -> Self {
        Blockstream {
            output: EntryVec::new(),
        }
    }

    pub fn entries(&self) -> Vec<String> {
        self.output.entries()
    }
}

fn serialize_transactions(entry: &Entry) -> Vec<Vec<u8>> {
    entry
        .transactions
        .iter()
        .map(|tx| serialize(&tx).unwrap())
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::entry::Entry;
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use std::collections::HashSet;
    use std::path::PathBuf;

    #[test]
    fn test_serialize_transactions() {
        let entry = Entry::new(&Hash::default(), 1, vec![]);
        let empty_vec: Vec<Vec<u8>> = vec![];
        assert_eq!(serialize_transactions(&entry), empty_vec);

        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let tx0 = system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
        let tx1 = system_transaction::transfer(&keypair1, &keypair0.pubkey(), 2, Hash::default());
        let serialized_tx0 = serialize(&tx0).unwrap();
        let serialized_tx1 = serialize(&tx1).unwrap();
        let entry = Entry::new(&Hash::default(), 1, vec![tx0, tx1]);
        assert_eq!(
            serialize_transactions(&entry),
            vec![serialized_tx0, serialized_tx1]
        );
    }

    #[test]
    fn test_blockstream() -> () {
        let blockstream = MockBlockstream::new(&PathBuf::from("test_stream"));
        let ticks_per_slot = 5;

        let mut blockhash = Hash::default();
        let mut entries = Vec::new();
        let mut expected_entries = Vec::new();

        let tick_height_initial = 1;
        let tick_height_final = tick_height_initial + ticks_per_slot + 2;
        let mut curr_slot = 0;
        let leader_pubkey = Pubkey::new_rand();

        for tick_height in tick_height_initial..=tick_height_final {
            if tick_height == 5 {
                blockstream
                    .emit_block_event(curr_slot, tick_height, &leader_pubkey, blockhash)
                    .unwrap();
                curr_slot += 1;
            }
            let entry = Entry::new(&mut blockhash, 1, vec![]); // just ticks
            blockhash = entry.hash;
            blockstream
                .emit_entry_event(curr_slot, tick_height, &leader_pubkey, &entry)
                .unwrap();
            expected_entries.push(entry.clone());
            entries.push(entry);
        }

        assert_eq!(
            blockstream.entries().len() as u64,
            // one entry per tick (1..=N+2) is +3, plus one block
            ticks_per_slot + 3 + 1
        );

        let mut j = 0;
        let mut matched_entries = 0;
        let mut matched_slots = HashSet::new();
        let mut matched_blocks = HashSet::new();

        for item in blockstream.entries() {
            let json: Value = serde_json::from_str(&item).unwrap();
            let dt_str = json["dt"].as_str().unwrap();

            // Ensure `ts` field parses as valid DateTime
            let _dt: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(dt_str).unwrap();

            let item_type = json["t"].as_str().unwrap();
            match item_type {
                "block" => {
                    let hash = json["hash"].to_string();
                    matched_blocks.insert(hash);
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
