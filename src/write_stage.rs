//! The `write_stage` module implements the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bank::Bank;
use bincode::serialize;
use counter::Counter;
use crdt::Crdt;
use entry::Entry;
use entry_writer::EntryWriter;
use ledger::Block;
use packet::BlobRecycler;
use result::{Error, Result};
use service::Service;
use signature::KeyPair;
use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use streamer::{BlobReceiver, BlobSender};
use timing;
use transaction::Transaction;
use voting::entries_to_votes;

pub struct WriteStage {
    thread_hdl: JoinHandle<()>,
}

const VOTE_TIMEOUT_MS: u64 = 1000;

impl WriteStage {
    /// Process any Entry items that have been published by the RecordStage.
    /// continuosly broadcast blobs of entries out
    pub fn write_and_send_entries<W: Write>(
        keypair: &KeyPair,
        bank: &Arc<Bank>,
        crdt: &Arc<RwLock<Crdt>>,
        entry_writer: &mut EntryWriter<W>,
        blob_sender: &BlobSender,
        blob_recycler: &BlobRecycler,
        entry_receiver: &Receiver<Vec<Entry>>,
        last_vote: &mut u64,
    ) -> Result<()> {
        let entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        let votes = entries_to_votes(&entries);
        crdt.write().unwrap().insert_votes(&votes);

        //TODO(anatoly): real stake based voting needs to change this
        //leader simply votes if the current set of validators have voted
        //on a valid last id
        entry_writer.write_and_register_entries(&entries)?;
        trace!("New blobs? {}", entries.len());
        let mut blobs = VecDeque::new();
        entries.to_blobs(blob_recycler, &mut blobs);

        let now = timing::timestamp();
        if now - *last_vote > VOTE_TIMEOUT_MS {
            //TODO(anatoly): vote if the last id set is mostly valid
            let ids: Vec<_> = crdt.read()
                .unwrap()
                .table
                .values()
                .map(|x| x.ledger_state.last_id)
                .collect();
            let total = bank.count_valid_ids(&ids);
            if total > 2 * ids.len() / 3 {
                *last_vote = now;
                let last_id = bank.last_id();
                let shared_blob = blob_recycler.allocate();
                let (vote, addr) = crdt.write().unwrap().new_vote(last_id)?;
                let tx = Transaction::new_vote(&keypair, vote, last_id, 0);
                let bytes = serialize(&tx)?;
                let len = bytes.len();
                {
                    let mut blob = shared_blob.write().unwrap();
                    blob.data[..len].copy_from_slice(&bytes);
                    blob.meta.set_addr(&addr);
                    blob.meta.size = len;
                }
            }
        }
        if !blobs.is_empty() {
            inc_new_counter!("write_stage-broadcast_vote-count", votes.len());
            inc_new_counter!("write_stage-broadcast_blobs-count", blobs.len());
            trace!("broadcasting {}", blobs.len());
            blob_sender.send(blobs)?;
        }
        Ok(())
    }

    /// Create a new WriteStage for writing and broadcasting entries.
    pub fn new<W: Write + Send + 'static>(
        keypair: KeyPair,
        bank: Arc<Bank>,
        crdt: Arc<RwLock<Crdt>>,
        blob_recycler: BlobRecycler,
        writer: W,
        entry_receiver: Receiver<Vec<Entry>>,
    ) -> (Self, BlobReceiver) {
        let (blob_sender, blob_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-writer".to_string())
            .spawn(move || {
                let mut entry_writer = EntryWriter::new(&bank, writer);
                let mut last_vote = 0;
                loop {
                    if let Err(e) = Self::write_and_send_entries(
                        &keypair,
                        &bank,
                        &crdt,
                        &mut entry_writer,
                        &blob_sender,
                        &blob_recycler,
                        &entry_receiver,
                        &mut last_vote,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            _ => {
                                inc_new_counter!("write_stage-error", 1);
                                error!("{:?}", e);
                            }
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
