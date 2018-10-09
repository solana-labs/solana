//! The `replicate_stage` replicates transactions broadcast by the leader.

use bank::Bank;
use cluster_info::ClusterInfo;
use counter::Counter;
use entry::EntryReceiver;
use ledger::{Block, LedgerWriter};
use log::Level;
use result::{Error, Result};
use service::Service;
use signature::Keypair;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use streamer::{responder, BlobSender};
use vote_stage::send_validator_vote;

// Implement a destructor for the ReplicateStage thread to signal it exited
// even on panics
struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}

impl Finalizer {
    fn new(exit_sender: Arc<AtomicBool>) -> Self {
        Finalizer { exit_sender }
    }
}
// Implement a destructor for Finalizer.
impl Drop for Finalizer {
    fn drop(&mut self) {
        self.exit_sender.clone().store(true, Ordering::Relaxed);
    }
}

pub struct ReplicateStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ReplicateStage {
    /// Process entry blobs, already in order
    fn replicate_requests(
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        window_receiver: &EntryReceiver,
        ledger_writer: Option<&mut LedgerWriter>,
        keypair: &Arc<Keypair>,
        vote_blob_sender: Option<&BlobSender>,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        //coalesce all the available entries into a single vote
        let mut entries = window_receiver.recv_timeout(timer)?;
        while let Ok(mut more) = window_receiver.try_recv() {
            entries.append(&mut more);
        }

        let res = bank.process_entries(&entries);

        if let Some(sender) = vote_blob_sender {
            send_validator_vote(bank, keypair, cluster_info, sender)?;
        }

        {
            let mut wcluster_info = cluster_info.write().unwrap();
            wcluster_info.insert_votes(&entries.votes());
        }

        inc_new_counter_info!(
            "replicate-transactions",
            entries.iter().map(|x| x.transactions.len()).sum()
        );

        // TODO: move this to another stage?
        if let Some(ledger_writer) = ledger_writer {
            ledger_writer.write_entries(entries)?;
        }

        res?;
        Ok(())
    }

    pub fn new(
        keypair: Arc<Keypair>,
        bank: Arc<Bank>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        window_receiver: EntryReceiver,
        ledger_path: Option<&str>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let (vote_blob_sender, vote_blob_receiver) = channel();
        let send = UdpSocket::bind("0.0.0.0:0").expect("bind");
        let t_responder = responder("replicate_stage", Arc::new(send), vote_blob_receiver);

        let mut ledger_writer = ledger_path.map(|p| LedgerWriter::open(p, false).unwrap());
        let keypair = Arc::new(keypair);

        let t_replicate = Builder::new()
            .name("solana-replicate-stage".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit);
                let now = Instant::now();
                let mut next_vote_secs = 1;
                loop {
                    // Only vote once a second.
                    let vote_sender = if now.elapsed().as_secs() > next_vote_secs {
                        next_vote_secs += 1;
                        Some(&vote_blob_sender)
                    } else {
                        None
                    };

                    if let Err(e) = Self::replicate_requests(
                        &bank,
                        &cluster_info,
                        &window_receiver,
                        ledger_writer.as_mut(),
                        &keypair,
                        vote_sender,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            _ => error!("{:?}", e),
                        }
                    }
                }
            }).unwrap();

        let thread_hdls = vec![t_responder, t_replicate];

        ReplicateStage { thread_hdls }
    }
}

impl Service for ReplicateStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
