//! The `write_stage` module implements the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bank::Bank;
use entry::Entry;
use entry_writer::EntryWriter;
use ledger::Block;
use packet::BlobRecycler;
use result::Result;
use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{Builder, JoinHandle};
use std::time::Duration;
use streamer::{BlobReceiver, BlobSender};

pub struct WriteStage {
    pub thread_hdl: JoinHandle<()>,
    pub blob_receiver: BlobReceiver,
}

impl WriteStage {
    /// Process any Entry items that have been published by the Historian.
    /// continuosly broadcast blobs of entries out
    pub fn write_and_send_entries<W: Write>(
        entry_writer: &EntryWriter,
        blob_sender: &BlobSender,
        blob_recycler: &BlobRecycler,
        writer: &Mutex<W>,
        entry_receiver: &Receiver<Vec<Entry>>,
    ) -> Result<()> {
        let entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        entry_writer.write_and_register_entries(writer, &entries)?;
        trace!("New blobs? {}", entries.len());
        let mut blobs = VecDeque::new();
        entries.to_blobs(blob_recycler, &mut blobs);
        if !blobs.is_empty() {
            trace!("broadcasting {}", blobs.len());
            blob_sender.send(blobs)?;
        }
        Ok(())
    }

    /// Create a new Rpu that wraps the given Bank.
    pub fn new<W: Write + Send + 'static>(
        bank: Arc<Bank>,
        exit: Arc<AtomicBool>,
        blob_recycler: BlobRecycler,
        writer: W,
        entry_receiver: Receiver<Vec<Entry>>,
    ) -> Self {
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-writer".to_string())
            .spawn(move || {
                let entry_writer = EntryWriter::new(&bank);
                let writer = Mutex::new(writer);
                loop {
                    let _ = Self::write_and_send_entries(
                        &entry_writer,
                        &blob_sender,
                        &blob_recycler,
                        &writer,
                        &entry_receiver,
                    );
                    if exit.load(Ordering::Relaxed) {
                        info!("broadcat_service exiting");
                        break;
                    }
                }
            })
            .unwrap();

        WriteStage {
            thread_hdl,
            blob_receiver,
        }
    }
}
