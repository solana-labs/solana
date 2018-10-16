//! The `replicate_stage` replicates transactions broadcast by the leader.

use bank::Bank;
use cluster_info::ClusterInfo;
use counter::Counter;
use entry::EntryReceiver;
use hash::Hash;
use influx_db_client as influxdb;
use leader_scheduler::LeaderScheduler;
use ledger::{Block, LedgerWriter};
use log::Level;
use metrics;
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
use sys_info::hostname;
use vote_stage::send_validator_vote;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ReplicateStageReturnType {
    LeaderRotation(u64, u64, Hash),
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
        poh_height: &mut u64,
        entry_height: &mut u64,
        leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
    ) -> Result<Hash> {
        let timer = Duration::new(1, 0);
        //coalesce all the available entries into a single vote
        let mut entries = window_receiver.recv_timeout(timer)?;
        while let Ok(mut more) = window_receiver.try_recv() {
            entries.append(&mut more);
        }

        metrics::submit(
            influxdb::Point::new("replicate-stage")
                .add_field(
                    "host",
                    influxdb::Value::String(hostname().unwrap_or_else(|_| "?".to_string())),
                ).add_field("count", influxdb::Value::Integer(entries.len() as i64))
                .to_owned(),
        );

        let mut res = Ok(());
        let last_entry_id = {
            let mut num_entries_to_write = entries.len();
            for (i, entry) in entries.iter().enumerate() {
                // max_poh_height is the PoH height at which the next leader rotation will
                // happen. The leader should send an entry such that the total PoH is equal
                // to max_poh_height - guard.
                // TODO: Introduce a "guard" for the end of transmission periods, the guard
                // is assumed to be zero for now.
                let max_poh_height = {
                    let ls_lock = leader_scheduler.read().unwrap();
                    ls_lock.max_height_for_leader(*poh_height)
                };

                let current_poh_height = *poh_height + entry.num_hashes;

                res = bank.process_entry(&entry);
                Bank::process_entry_votes(
                    &bank,
                    &entry,
                    current_poh_height,
                    &mut *leader_scheduler.write().unwrap(),
                );

                {
                    let ls_lock = leader_scheduler.read().unwrap();
                    if current_poh_height > max_poh_height {
                        // TODO: Handle the case where the leader acts badly and sends
                        // an entry such that the total poh_height > max_poh_height - guard
                    } else if current_poh_height == max_poh_height {
                        let my_id = keypair.pubkey();
                        let scheduled_leader = ls_lock
                            .get_scheduled_leader(current_poh_height)
                            .expect(
                            "Scheduled leader id should never be unknown while processing entries",
                        );
                        cluster_info.write().unwrap().set_leader(scheduled_leader);
                        if my_id == scheduled_leader {
                            *poh_height = current_poh_height;
                            num_entries_to_write = i + 1;
                            break;
                        }
                    }
                }

                if res.is_err() {
                    // TODO: This will return early from the first entry that has an erroneous
                    // transaction, instad of processing the rest of the entries in the vector
                    // of received entries. This is in line with previous behavior when
                    // bank.process_entries() was used to process the entries, but doesn't solve the
                    // issue that the bank state was still changed, leading to inconsistencies with the
                    // leader as the leader currently should not be publishing erroneous transactions
                    break;
                }

                // Commit the poh height if everything went smoothly
                *poh_height = current_poh_height;
            }

            // If leader rotation happened, only write the entries up to leader rotation.
            entries.truncate(num_entries_to_write);
            entries
                .last()
                .expect("Entries cannot be empty at this point")
                .id
        };

        if let Some(sender) = vote_blob_sender {
            send_validator_vote(bank, keypair, cluster_info, sender)?;
        }

        cluster_info.write().unwrap().insert_votes(&entries.votes());

        inc_new_counter_info!(
            "replicate-transactions",
            entries.iter().map(|x| x.transactions.len()).sum()
        );

        let entries_len = entries.len() as u64;
        // TODO: move this to another stage?
        // TODO: In line with previous behavior, this will write all the entries even if
        // an error occurred processing one of the entries (causing the rest of the entries to
        // not be processed).
        if let Some(ledger_writer) = ledger_writer {
            ledger_writer.write_entries(entries)?;
        }

        *entry_height += entries_len;
        res?;
        Ok(last_entry_id)
    }

    pub fn new(
        keypair: Arc<Keypair>,
        bank: Arc<Bank>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        window_receiver: EntryReceiver,
        ledger_path: Option<&str>,
        exit: Arc<AtomicBool>,
        poh_height: u64,
        entry_height: u64,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
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
                let mut poh_height_ = poh_height;
                let mut last_entry_id = None;
                loop {
                    let leader_id = leader_scheduler
                        .read()
                        .unwrap()
                        .get_scheduled_leader(poh_height_)
                        .expect("Scheduled leader id should never be unknown at this point");
                    if leader_id == keypair.pubkey() {
                        return Some(ReplicateStageReturnType::LeaderRotation(
                            poh_height_,
                            entry_height_,
                            // We should never start the TPU / this stage on an exact entry that causes leader
                            // rotation (Fullnode should automatically transition on startup if it detects
                            // are no longer a validator. Hence we can assume that some entry must have
                            // triggered leader rotation
                            last_entry_id.expect("Must exist an entry that triggered rotation"),
                        ));
                    }

                    // Only vote once a second.
                    let vote_sender = if now.elapsed().as_secs() > next_vote_secs {
                        next_vote_secs += 1;
                        Some(&vote_blob_sender)
                    } else {
                        None
                    };

                    match Self::replicate_requests(
                        &bank,
                        &cluster_info,
                        &window_receiver,
                        ledger_writer.as_mut(),
                        &keypair,
                        vote_sender,
                        &mut poh_height_,
                        &mut entry_height_,
                        &leader_scheduler,
                    ) {
                        Err(Error::RecvTimeoutError(RecvTimeoutError::Disconnected)) => break,
                        Err(Error::RecvTimeoutError(RecvTimeoutError::Timeout)) => (),
                        Err(e) => error!("{:?}", e),
                        Ok(last_entry_id_) => {
                            last_entry_id = Some(last_entry_id_);
                        }
                    }
                }

                None
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

#[cfg(test)]
mod test {
    use cluster_info::{ClusterInfo, Node};
    use entry::Entry;
    use fullnode::Fullnode;
    use leader_scheduler::{make_active_set_entries, LeaderScheduler, LeaderSchedulerConfig};
    use ledger::{create_tmp_sample_ledger, LedgerWriter};
    use logger;
    use replicate_stage::{ReplicateStage, ReplicateStageReturnType};
    use service::Service;
    use signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};

    #[test]
    pub fn test_replicate_stage_leader_rotation_exit() {
        logger::setup();

        // Set up dummy node to host a ReplicateStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);
        let cluster_info_me = ClusterInfo::new(my_node.info.clone()).expect("ClusterInfo::new");

        // Create a ledger
        let num_ending_ticks = 1;
        let (mint, my_ledger_path, genesis_entries) = create_tmp_sample_ledger(
            "test_replicate_stage_leader_rotation_exit",
            10_000,
            num_ending_ticks,
        );
        let mut last_id = genesis_entries
            .last()
            .expect("expected at least one genesis entry")
            .id;

        // Write two entries to the ledger so that the validator is in the active set:
        // 1) Give the validator a nonzero number of tokens 2) A vote from the validator .
        // This will cause leader rotation after the bootstrap height
        let mut ledger_writer = LedgerWriter::open(&my_ledger_path, false).unwrap();
        let bootstrap_entries =
            make_active_set_entries(&my_keypair, &mint.keypair(), &last_id, &last_id);
        last_id = bootstrap_entries.last().unwrap().id;
        let initial_poh_height = num_ending_ticks + bootstrap_entries.len() as u64;
        let initial_entry_len = (genesis_entries.len() + bootstrap_entries.len()) as u64;
        ledger_writer.write_entries(bootstrap_entries).unwrap();

        // Set up the LeaderScheduler so that this this node becomes the leader at
        // bootstrap_height = num_bootstrap_slots * leader_rotation_interval
        let old_leader_id = Keypair::new().pubkey();
        let leader_rotation_interval = 10;
        let num_bootstrap_slots = 2;
        let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            old_leader_id,
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(leader_rotation_interval * 2),
            Some(bootstrap_height),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        // Set up the bank
        let (bank, _, _, _) =
            Fullnode::new_bank_from_ledger(&my_ledger_path, &mut leader_scheduler);

        let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));

        // Set up the replicate stage
        let (entry_sender, entry_receiver) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        let replicate_stage = ReplicateStage::new(
            Arc::new(my_keypair),
            Arc::new(bank),
            Arc::new(RwLock::new(cluster_info_me)),
            entry_receiver,
            Some(&my_ledger_path),
            exit.clone(),
            initial_poh_height,
            initial_entry_len,
            leader_scheduler.clone(),
        );

        // Send enough entries to trigger leader rotation
        let extra_entries = leader_rotation_interval;
        let total_entries_to_send = (bootstrap_height + extra_entries) as usize;
        let num_hashes = 1;
        let mut entries_to_send = vec![];

        while entries_to_send.len() < total_entries_to_send {
            let entry = Entry::new(&mut last_id, num_hashes, vec![]);
            last_id = entry.id;
            entries_to_send.push(entry);
        }

        assert!(num_ending_ticks < bootstrap_height);
        let expected_entry_length = bootstrap_height + mint.create_entries().len() as u64;
        let expected_last_id =
            entries_to_send[(bootstrap_height - initial_poh_height - 1) as usize].id;
        entry_sender.send(entries_to_send).unwrap();

        // Wait for replicate_stage to exit and check return value is correct
        assert_eq!(
            Some(ReplicateStageReturnType::LeaderRotation(
                bootstrap_height,
                expected_entry_length,
                expected_last_id,
            )),
            replicate_stage.join().expect("replicate stage join")
        );

        assert_eq!(exit.load(Ordering::Relaxed), true);

        //Check ledger height is correct
        let mut leader_scheduler = Arc::try_unwrap(leader_scheduler)
            .expect("Multiple references to this RwLock still exist")
            .into_inner()
            .expect("RwLock for LeaderScheduler is still locked");

        leader_scheduler.reset();
        let (_, entry_height, _, _) =
            Fullnode::new_bank_from_ledger(&my_ledger_path, &mut leader_scheduler);

        assert_eq!(entry_height, expected_entry_length);
        let _ignored = remove_dir_all(&my_ledger_path);
    }
}
