//! The `write_stage` module implements the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bank::Bank;
use entry::Entry;
use entry_writer::EntryWriter;
use packet::BlobRecycler;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{Builder, JoinHandle};
use streamer::BlobReceiver;

pub struct WriteStage {
    pub thread_hdl: JoinHandle<()>,
    pub blob_receiver: BlobReceiver,
}

impl WriteStage {
    /// Create a new Rpu that wraps the given Bank.
    pub fn new<W: Write + Send + 'static>(
        bank: Arc<Bank>,
        exit: Arc<AtomicBool>,
        blob_recycler: BlobRecycler,
        writer: Mutex<W>,
        entry_receiver: Receiver<Vec<Entry>>,
    ) -> Self {
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-writer".to_string())
            .spawn(move || loop {
                let entry_writer = EntryWriter::new(&bank);
                let _ = entry_writer.write_and_send_entries(
                    &blob_sender,
                    &blob_recycler,
                    &writer,
                    &entry_receiver,
                );
                if exit.load(Ordering::Relaxed) {
                    info!("broadcat_service exiting");
                    break;
                }
            })
            .unwrap();

        WriteStage {
            thread_hdl,
            blob_receiver,
        }
    }

    pub fn new_drain(
        bank: Arc<Bank>,
        exit: Arc<AtomicBool>,
        entry_receiver: Receiver<Vec<Entry>>,
    ) -> Self {
        let (_blob_sender, blob_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-drain".to_string())
            .spawn(move || {
                let entry_writer = EntryWriter::new(&bank);
                loop {
                    let _ = entry_writer.drain_entries(&entry_receiver);
                    if exit.load(Ordering::Relaxed) {
                        info!("drain_service exiting");
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
