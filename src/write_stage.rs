//! The `write_stage` module implements the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bank::Bank;
use entry::Entry;
use entry_writer::EntryWriter;
use ledger::Block;
use packet::BlobRecycler;
use result::{Error, Result};
use service::Service;
use std::collections::VecDeque;
use std::io::Write;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use streamer::{BlobReceiver, BlobSender};

pub struct WriteStage {
    thread_hdl: JoinHandle<()>,
}

impl WriteStage {
    /// Process any Entry items that have been published by the RecordStage.
    /// continuosly broadcast blobs of entries out
    pub fn write_and_send_entries<W: Write>(
        entry_writer: &mut EntryWriter<W>,
        blob_sender: &BlobSender,
        blob_recycler: &BlobRecycler,
        entry_receiver: &Receiver<Vec<Entry>>,
    ) -> Result<()> {
        let entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        entry_writer.write_and_register_entries(&entries)?;
        trace!("New blobs? {}", entries.len());
        let mut blobs = VecDeque::new();
        entries.to_blobs(blob_recycler, &mut blobs);
        if !blobs.is_empty() {
            trace!("broadcasting {}", blobs.len());
            blob_sender.send(blobs)?;
        }
        Ok(())
    }

    /// Create a new WriteStage for writing and broadcasting entries.
    pub fn new<W: Write + Send + 'static>(
        bank: Arc<Bank>,
        blob_recycler: BlobRecycler,
        writer: W,
        entry_receiver: Receiver<Vec<Entry>>,
    ) -> (Self, BlobReceiver) {
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-writer".to_string())
            .spawn(move || {
                let mut entry_writer = EntryWriter::new(&bank, writer);
                loop {
                    if let Err(e) = Self::write_and_send_entries(
                        &mut entry_writer,
                        &blob_sender,
                        &blob_recycler,
                        &entry_receiver,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            Error::SendError => (), // Ignore when downstream stage exits prematurely.
                            _ => error!("{:?}", e),
                        }
                    };
                }
            })
            .unwrap();

        (WriteStage { thread_hdl }, blob_receiver)
    }
}

impl Service for WriteStage {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        vec![self.thread_hdl]
    }

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
