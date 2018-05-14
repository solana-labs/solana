//! The `write_stage` module implements write stage of the RPU.

use bank::Bank;
use entry::Entry;
use entry_writer::EntryWriter;
use packet;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{spawn, JoinHandle};
use streamer;

pub struct WriteStage {
    pub thread_hdl: JoinHandle<()>,
    pub blob_receiver: streamer::BlobReceiver,
}

impl WriteStage {
    /// Create a new Rpu that wraps the given Bank.
    pub fn new<W: Write + Send + 'static>(
        bank: Arc<Bank>,
        exit: Arc<AtomicBool>,
        blob_recycler: packet::BlobRecycler,
        writer: Mutex<W>,
        entry_receiver: Receiver<Entry>,
    ) -> Self {
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = spawn(move || loop {
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
        });

        WriteStage {
            thread_hdl,
            blob_receiver,
        }
    }

    pub fn new_drain(
        bank: Arc<Bank>,
        exit: Arc<AtomicBool>,
        entry_receiver: Receiver<Entry>,
    ) -> Self {
        let (_blob_sender, blob_receiver) = channel();
        let thread_hdl = spawn(move || {
            let entry_writer = EntryWriter::new(&bank);
            loop {
                let _ = entry_writer.drain_entries(&entry_receiver);
                if exit.load(Ordering::Relaxed) {
                    info!("drain_service exiting");
                    break;
                }
            }
        });

        WriteStage {
            thread_hdl,
            blob_receiver,
        }
    }
}
