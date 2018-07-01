//! The `entry_writer` module helps implement the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bank::Bank;
use entry::Entry;
use ledger::Block;
use packet::BlobRecycler;
use result::Result;
use serde_json;
use std::collections::VecDeque;
use std::io::{self, sink, Write};
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use std::time::Duration;
use streamer::BlobSender;

pub struct EntryWriter<'a> {
    bank: &'a Bank,
}

impl<'a> EntryWriter<'a> {
    /// Create a new Tpu that wraps the given Bank.
    pub fn new(bank: &'a Bank) -> Self {
        EntryWriter { bank }
    }

    fn write_entry<W: Write>(writer: &Mutex<W>, entry: &Entry) -> io::Result<()> {
        let serialized = serde_json::to_string(&entry).unwrap();
        writeln!(writer.lock().unwrap(), "{}", serialized)
    }

    pub fn write_entries<W: Write>(writer: &Mutex<W>, entries: &[Entry]) -> io::Result<()> {
        for entry in entries {
            Self::write_entry(writer, entry)?;
        }
        Ok(())
    }

    fn write_and_register_entry<W: Write>(
        &self,
        writer: &Mutex<W>,
        entry: &Entry,
    ) -> io::Result<()> {
        trace!("write_and_register_entry entry");
        if !entry.has_more {
            self.bank.register_entry_id(&entry.id);
        }
        Self::write_entry(&writer, entry)
    }

    fn write_and_register_entries<W: Write>(
        &self,
        writer: &Mutex<W>,
        entries: &[Entry],
    ) -> io::Result<()> {
        for entry in entries {
            self.write_and_register_entry(writer, &entry)?;
        }
        Ok(())
    }

    /// Process any Entry items that have been published by the Historian.
    /// continuosly broadcast blobs of entries out
    pub fn write_and_send_entries<W: Write>(
        &self,
        blob_sender: &BlobSender,
        blob_recycler: &BlobRecycler,
        writer: &Mutex<W>,
        entry_receiver: &Receiver<Vec<Entry>>,
    ) -> Result<()> {
        let entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        self.write_and_register_entries(writer, &entries)?;
        trace!("New blobs? {}", entries.len());
        let mut blobs = VecDeque::new();
        entries.to_blobs(blob_recycler, &mut blobs);
        if !blobs.is_empty() {
            trace!("broadcasting {}", blobs.len());
            blob_sender.send(blobs)?;
        }
        Ok(())
    }

    /// Process any Entry items that have been published by the Historian.
    /// continuosly broadcast blobs of entries out
    pub fn drain_entries(&self, entry_receiver: &Receiver<Vec<Entry>>) -> Result<()> {
        let entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        self.write_and_register_entries(&Mutex::new(sink()), &entries)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger;
    use mint::Mint;
    use packet::BLOB_DATA_SIZE;
    use signature::{KeyPair, KeyPairUtil};
    use transaction::Transaction;

    #[test]
    fn test_dont_register_partial_entries() {
        let mint = Mint::new(1);
        let bank = Bank::new(&mint);

        let entry_writer = EntryWriter::new(&bank);
        let keypair = KeyPair::new();
        let tx = Transaction::new(&mint.keypair(), keypair.pubkey(), 1, mint.last_id());

        // NOTE: if Entry grows to larger than a transaction, the code below falls over
        let threshold = (BLOB_DATA_SIZE / 256) - 1; // 256 is transaction size

        // Verify large entries are split up and the first sets has_more.
        let txs = vec![tx.clone(); threshold * 2];
        let entries = ledger::next_entries(&mint.last_id(), 0, txs);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].has_more);
        assert!(!entries[1].has_more);

        // Verify that write_and_register_entry doesn't register the first entries after a split.
        assert_eq!(bank.last_id(), mint.last_id());
        let writer = Mutex::new(sink());
        entry_writer
            .write_and_register_entry(&writer, &entries[0])
            .unwrap();
        assert_eq!(bank.last_id(), mint.last_id());

        // Verify that write_and_register_entry registers the final entry after a split.
        entry_writer
            .write_and_register_entry(&writer, &entries[1])
            .unwrap();
        assert_eq!(bank.last_id(), entries[1].id);
    }
}
