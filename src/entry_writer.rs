//! The `entry_writer` module helps implement the TPU's write stage.

use entry::Entry;
use event_processor::EventProcessor;
use ledger;
use packet;
use request_stage::RequestProcessor;
use result::Result;
use serde_json;
use std::collections::VecDeque;
use std::io::Write;
use std::io::sink;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use streamer;

pub struct EntryWriter<'a> {
    event_processor: &'a EventProcessor,
    request_processor: &'a RequestProcessor,
}

impl<'a> EntryWriter<'a> {
    /// Create a new Tpu that wraps the given Accountant.
    pub fn new(
        event_processor: &'a EventProcessor,
        request_processor: &'a RequestProcessor,
    ) -> Self {
        EntryWriter {
            event_processor,
            request_processor,
        }
    }

    fn write_entry<W: Write>(&self, writer: &Mutex<W>, entry: &Entry) {
        trace!("write_entry entry");
        self.event_processor.accountant.register_entry_id(&entry.id);
        writeln!(
            writer.lock().expect("'writer' lock in fn fn write_entry"),
            "{}",
            serde_json::to_string(&entry).expect("'entry' to_strong in fn write_entry")
        ).expect("writeln! in fn write_entry");
        self.request_processor.notify_entry_info_subscribers(&entry);
    }

    fn write_entries<W: Write>(&self, writer: &Mutex<W>) -> Result<Vec<Entry>> {
        //TODO implement a serialize for channel that does this without allocations
        let mut l = vec![];
        let entry = self.event_processor
            .output
            .lock()
            .expect("'ouput' lock in fn receive_all")
            .recv_timeout(Duration::new(1, 0))?;
        self.write_entry(writer, &entry);
        l.push(entry);
        while let Ok(entry) = self.event_processor
            .output
            .lock()
            .expect("'output' lock in fn write_entries")
            .try_recv()
        {
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
    ) -> Result<()> {
        let mut q = VecDeque::new();
        let list = self.write_entries(writer)?;
        trace!("New blobs? {}", list.len());
        ledger::process_entry_list_into_blobs(&list, blob_recycler, &mut q);
        if !q.is_empty() {
            broadcast.send(q)?;
        }
        Ok(())
    }

    /// Process any Entry items that have been published by the Historian.
    /// continuosly broadcast blobs of entries out
    pub fn drain_entries(&self) -> Result<()> {
        self.write_entries(&Arc::new(Mutex::new(sink())))?;
        Ok(())
    }
}
