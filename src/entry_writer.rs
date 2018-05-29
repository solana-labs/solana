//! The `entry_writer` module helps implement the TPU's write stage.

use bank::Bank;
use entry::Entry;
use ledger::Block;
use packet;
use result::Result;
use serde_json;
use std::collections::VecDeque;
use std::io::Write;
use std::io::sink;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use streamer;

pub struct EntryWriter<'a> {
    bank: &'a Bank,
}

impl<'a> EntryWriter<'a> {
    /// Create a new Tpu that wraps the given Bank.
    pub fn new(bank: &'a Bank) -> Self {
        EntryWriter { bank }
    }

    fn write_entry<W: Write>(&self, writer: &Mutex<W>, entry: &Entry) {
        trace!("write_entry entry");
        self.bank.register_entry_id(&entry.id);
        writeln!(
            writer.lock().expect("'writer' lock in fn fn write_entry"),
            "{}",
            serde_json::to_string(&entry).expect("'entry' to_strong in fn write_entry")
        ).expect("writeln! in fn write_entry");
    }

    fn write_entries<W: Write>(
        &self,
        writer: &Mutex<W>,
        entry_receiver: &Receiver<Entry>,
    ) -> Result<Vec<Entry>> {
        //TODO implement a serialize for channel that does this without allocations
        let mut l = vec![];
        let entry = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        self.write_entry(writer, &entry);
        l.push(entry);
        while let Ok(entry) = entry_receiver.try_recv() {
            self.write_entry(writer, &entry);
            l.push(entry);
        }
        Ok(l)
    }

    /// Process any Entry items that have been published by the Historian.
    /// continuosly broadcast blobs of entries out
    pub fn write_and_send_entries<W: Write>(
        &self,
        broadcast: &streamer::BlobSender,
        blob_recycler: &packet::BlobRecycler,
        writer: &Mutex<W>,
        entry_receiver: &Receiver<Entry>,
    ) -> Result<()> {
        let mut q = VecDeque::new();
        let list = self.write_entries(writer, entry_receiver)?;
        trace!("New blobs? {}", list.len());
        list.to_blobs(blob_recycler, &mut q);
        if !q.is_empty() {
            trace!("broadcasting {}", q.len());
            broadcast.send(q)?;
        }
        Ok(())
    }

    /// Process any Entry items that have been published by the Historian.
    /// continuosly broadcast blobs of entries out
    pub fn drain_entries(&self, entry_receiver: &Receiver<Entry>) -> Result<()> {
        self.write_entries(&Arc::new(Mutex::new(sink())), entry_receiver)?;
        Ok(())
    }
}
