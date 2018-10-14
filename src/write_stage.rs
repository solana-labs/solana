//! The `write_stage` module implements the TPU's write stage. It
//! writes entries to the given writer, which is typically a file or
//! stdout, and then sends the Entry to its output channel.

use bank::Bank;
use cluster_info::ClusterInfo;
use counter::Counter;
use entry::Entry;
use ledger::{Block, LedgerWriter};
use log::Level;
use result::{Error, Result};
use service::Service;
use signature::Keypair;
use std::net::UdpSocket;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};
use streamer::responder;
use timing::{duration_as_ms, duration_as_s};
use vote_stage::send_leader_vote;

pub struct WriteStage {
    thread_hdls: Vec<JoinHandle<()>>,
    write_thread: JoinHandle<()>,
}

impl WriteStage {
    /// Process any Entry items that have been published by the RecordStage.
    /// continuosly send entries out
    pub fn write_and_send_entries(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        ledger_writer: &mut LedgerWriter,
        entry_sender: &Sender<Vec<Entry>>,
        entry_receiver: &Receiver<Vec<Entry>>,
    ) -> Result<()> {
        let mut ventries = Vec::new();
        let mut received_entries = entry_receiver.recv_timeout(Duration::new(1, 0))?;
        let now = Instant::now();
        let mut num_new_entries = 0;
        let mut num_txs = 0;

        loop {
            num_new_entries += received_entries.len();
            ventries.push(received_entries);

            if let Ok(n) = entry_receiver.try_recv() {
                received_entries = n;
            } else {
                break;
            }
        }
        inc_new_counter_info!("write_stage-entries_received", num_new_entries);

        debug!("write_stage entries: {}", num_new_entries);

        let mut entries_send_total = 0;
        let mut cluster_info_votes_total = 0;

        let start = Instant::now();
        for entries in ventries {
            let cluster_info_votes_start = Instant::now();
            let votes = &entries.votes();
            cluster_info.write().unwrap().insert_votes(&votes);
            cluster_info_votes_total += duration_as_ms(&cluster_info_votes_start.elapsed());

            for e in &entries {
                num_txs += e.transactions.len();
                ledger_writer.write_entry_noflush(&e)?;
            }

            inc_new_counter_info!("write_stage-write_entries", entries.len());

            //TODO(anatoly): real stake based voting needs to change this
            //leader simply votes if the current set of validators have voted
            //on a valid last id

            trace!("New entries? {}", entries.len());
            let entries_send_start = Instant::now();
            if !entries.is_empty() {
                inc_new_counter_info!("write_stage-recv_vote", votes.len());
                inc_new_counter_info!("write_stage-entries_sent", entries.len());
                trace!("broadcasting {}", entries.len());
                entry_sender.send(entries)?;
            }

            entries_send_total += duration_as_ms(&entries_send_start.elapsed());
        }
        ledger_writer.flush()?;
        inc_new_counter_info!(
            "write_stage-time_ms",
            duration_as_ms(&now.elapsed()) as usize
        );
        debug!("done write_stage txs: {} time {} ms txs/s: {} entries_send_total: {} cluster_info_votes_total: {}",
              num_txs, duration_as_ms(&start.elapsed()),
              num_txs as f32 / duration_as_s(&start.elapsed()),
              entries_send_total,
              cluster_info_votes_total);

        Ok(())
    }

    /// Create a new WriteStage for writing and broadcasting entries.
    pub fn new(
        keypair: Arc<Keypair>,
        bank: Arc<Bank>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        ledger_path: &str,
        entry_receiver: Receiver<Vec<Entry>>,
    ) -> (Self, Receiver<Vec<Entry>>) {
        let (vote_blob_sender, vote_blob_receiver) = channel();
        let send = UdpSocket::bind("0.0.0.0:0").expect("bind");
        let t_responder = responder(
            "write_stage_vote_sender",
            Arc::new(send),
            vote_blob_receiver,
        );
        let (entry_sender, entry_receiver_forward) = channel();
        let mut ledger_writer = LedgerWriter::recover(ledger_path).unwrap();

        let write_thread = Builder::new()
            .name("solana-writer".to_string())
            .spawn(move || {
                let mut last_vote = 0;
                let mut last_valid_validator_timestamp = 0;
                let id = cluster_info.read().unwrap().id;
                loop {
                    if let Err(e) = Self::write_and_send_entries(
                        &cluster_info,
                        &mut ledger_writer,
                        &entry_sender,
                        &entry_receiver,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                                break;
                            }
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            _ => {
                                inc_new_counter_info!(
                                    "write_stage-write_and_send_entries-error",
                                    1
                                );
                                error!("{:?}", e);
                            }
                        }
                    };
                    if let Err(e) = send_leader_vote(
                        &id,
                        &keypair,
                        &bank,
                        &cluster_info,
                        &vote_blob_sender,
                        &mut last_vote,
                        &mut last_valid_validator_timestamp,
                    ) {
                        inc_new_counter_info!("write_stage-leader_vote-error", 1);
                        error!("{:?}", e);
                    }
                }
            }).unwrap();

        let thread_hdls = vec![t_responder];
        (
            WriteStage {
                write_thread,
                thread_hdls,
            },
            entry_receiver_forward,
        )
    }
}

impl Service for WriteStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }

        self.write_thread.join()
    }
}