//! The `replicate_stage` replicates transactions broadcast by the leader.

use bank::Bank;
use cluster_info::ClusterInfo;
use counter::Counter;
use entry::EntryReceiver;
use leader_scheduler::LeaderScheduler;
use ledger::{Block, LedgerWriter};
use log::Level;
use result::{Error, Result};
use service::Service;
use signature::{Keypair, KeypairUtil};
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ReplicateStageReturnType {
    LeaderRotation(u64),
}

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
    t_responder: JoinHandle<()>,
    t_replicate: JoinHandle<Option<ReplicateStageReturnType>>,
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
        entry_height: &mut u64,
        leader_scheduler_option: &mut Option<Arc<RwLock<LeaderScheduler>>>,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        //coalesce all the available entries into a single vote
        let mut entries = window_receiver.recv_timeout(timer)?;
        while let Ok(mut more) = window_receiver.try_recv() {
            entries.append(&mut more);
        }

        let errs = vec![];
        let mut leader_rotation_occurred = false;
        {
            let mut leader_scheduler_lock_option = None;
            if let Some(leader_scheduler_lock) = leader_scheduler_option {
                let wlock = leader_scheduler_lock.write().unwrap();
                leader_scheduler_lock_option = Some(wlock);
            }

            let mut num_entries_to_write = entries.len();
            for (i, entry) in entries.iter().enumerate() {
                let res = bank.process_entry(
                    &entry,
                    Some(*entry_height + i as u64 + 1),
                    &mut leader_scheduler_lock_option
                        .as_mut()
                        .map(|wlock| &mut (**wlock)),
                );

                if let Some(leader_scheduler) = leader_scheduler_lock_option {
                    let my_id = keypair.pubkey();
                    match leader_scheduler.get_scheduled_leader(*entry_height + i as u64) {
                        // If we are the next leader, exit
                        Some(next_leader_id) if my_id == next_leader_id => {
                            leader_rotation_occurred = true;
                            num_entries_to_write = i + 1;
                            break;
                        }
                        // TODO: Figure out where to set the new leader in the crdt for
                        // validator -> validator transition (once we have real leader scheduling,
                        // this decision will be clearer). Also make sure new blobs to window actually
                        // originate from new leader
                        _ => (),
                    }
                }

                if res.is_err() {
                    errs.push(res);
                }
            }
            entries.truncate(num_entries_to_write);
        }

        if let Some(sender) = vote_blob_sender {
            send_validator_vote(bank, keypair, cluster_info, sender)?;
        }
        let votes = &entries.votes(*entry_height);
        wcluster_info.write().unwrap().insert_votes(votes);

        inc_new_counter_info!(
            "replicate-transactions",
            entries.iter().map(|x| x.transactions.len()).sum()
        );

        // TODO: move this to another stage?
        if let Some(ledger_writer) = ledger_writer {
            ledger_writer.write_entries(entries)?;
        }

        *entry_height += entries.len() as u64;

        if errs.len() > 0 {
            errs[0]?;
        }

        Ok(())
    }

    pub fn new(
        keypair: Arc<Keypair>,
        bank: Arc<Bank>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        window_receiver: EntryReceiver,
        ledger_path: Option<&str>,
        exit: Arc<AtomicBool>,
        entry_height: u64,
        leader_scheduler_option: Option<Arc<RwLock<LeaderScheduler>>>,
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
                let mut entry_height_ = entry_height;
                let mut leader_scheduler_option_ = leader_scheduler_option;
                loop {
                    if let Some(leader_scheduler_lock) = leader_scheduler_option {
                        let leader_id = leader_scheduler_lock
                            .read()
                            .unwrap()
                            .get_scheduled_leader(entry_height_)
                            .expect("Scheduled leader id should never be unknown at this point");
                        if leader_id != keypair.pubkey() {
                            return Some(ReplicateStageReturnType::LeaderRotation(entry_height));
                        }
                    }
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
                        &mut entry_height_,
                        &mut leader_scheduler_option_,
                    ) {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            _ => error!("{:?}", e),
                        }
                    }
                }

                return None;
            }).unwrap();

        ReplicateStage {
            t_responder,
            t_replicate,
        }
    }
}

impl Service for ReplicateStage {
    type JoinReturnType = Option<ReplicateStageReturnType>;

    fn join(self) -> thread::Result<Option<ReplicateStageReturnType>> {
        self.t_responder.join()?;
        self.t_replicate.join()
    }
}
