//! The `replay_stage` replays transactions broadcast by the leader.

use crate::bank::Bank;
use crate::cluster_info::ClusterInfo;
use crate::counter::Counter;
use crate::entry::{EntryReceiver, EntrySender};
use solana_sdk::hash::Hash;

use crate::entry::EntrySlice;
use crate::leader_scheduler::TICKS_PER_BLOCK;
use crate::packet::BlobError;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::streamer::{responder, BlobSender};
use crate::vote_signer_proxy::VoteSignerProxy;
use log::Level;
use solana_metrics::{influxdb, submit};
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;

pub const MAX_ENTRY_RECV_PER_ITER: usize = 512;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ReplayStageReturnType {
    LeaderRotation(u64, u64, Hash),
}

// Implement a destructor for the ReplayStage thread to signal it exited
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

pub struct ReplayStage {
    t_responder: JoinHandle<()>,
    t_replay: JoinHandle<Option<ReplayStageReturnType>>,
}

impl ReplayStage {
    /// Process entry blobs, already in order
    #[allow(clippy::too_many_arguments)]
    fn process_entries(
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        window_receiver: &EntryReceiver,
        keypair: &Arc<Keypair>,
        vote_signer: &Arc<VoteSignerProxy>,
        vote_blob_sender: Option<&BlobSender>,
        ledger_entry_sender: &EntrySender,
        entry_height: &mut u64,
        last_entry_id: &mut Hash,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        //coalesce all the available entries into a single vote
        let mut entries = window_receiver.recv_timeout(timer)?;
        while let Ok(mut more) = window_receiver.try_recv() {
            entries.append(&mut more);
            if entries.len() >= MAX_ENTRY_RECV_PER_ITER {
                break;
            }
        }

        submit(
            influxdb::Point::new("replicate-stage")
                .add_field("count", influxdb::Value::Integer(entries.len() as i64))
                .to_owned(),
        );

        let mut res = Ok(());
        let mut num_entries_to_write = entries.len();
        let now = Instant::now();
        if !entries.as_slice().verify(last_entry_id) {
            inc_new_counter_info!("replicate_stage-verify-fail", entries.len());
            return Err(Error::BlobError(BlobError::VerificationFailed));
        }
        inc_new_counter_info!(
            "replicate_stage-verify-duration",
            duration_as_ms(&now.elapsed()) as usize
        );

        let (current_leader, _) = bank
            .get_current_leader()
            .expect("Scheduled leader should be calculated by this point");
        let my_id = keypair.pubkey();

        // Next vote tick is ceiling of (current tick/ticks per block)
        let mut num_ticks_to_next_vote = TICKS_PER_BLOCK - (bank.tick_height() % TICKS_PER_BLOCK);
        let mut start_entry_index = 0;
        for (i, entry) in entries.iter().enumerate() {
            inc_new_counter_info!("replicate-stage_bank-tick", bank.tick_height() as usize);
            if entry.is_tick() {
                num_ticks_to_next_vote -= 1;
            }
            inc_new_counter_info!(
                "replicate-stage_tick-to-vote",
                num_ticks_to_next_vote as usize
            );
            // If it's the last entry in the vector, i will be vec len - 1.
            // If we don't process the entry now, the for loop will exit and the entry
            // will be dropped.
            if 0 == num_ticks_to_next_vote || (i + 1) == entries.len() {
                res = bank.process_entries(&entries[start_entry_index..=i]);

                if res.is_err() {
                    // TODO: This will return early from the first entry that has an erroneous
                    // transaction, instead of processing the rest of the entries in the vector
                    // of received entries. This is in line with previous behavior when
                    // bank.process_entries() was used to process the entries, but doesn't solve the
                    // issue that the bank state was still changed, leading to inconsistencies with the
                    // leader as the leader currently should not be publishing erroneous transactions
                    inc_new_counter_info!(
                        "replicate-stage_failed_process_entries",
                        (i - start_entry_index)
                    );

                    break;
                }

                if 0 == num_ticks_to_next_vote {
                    if let Some(sender) = vote_blob_sender {
                        vote_signer
                            .send_validator_vote(bank, &cluster_info, sender)
                            .unwrap();
                    }
                }
                let (scheduled_leader, _) = bank
                    .get_current_leader()
                    .expect("Scheduled leader should be calculated by this point");

                // TODO: Remove this soon once we boot the leader from ClusterInfo
                if scheduled_leader != current_leader {
                    cluster_info.write().unwrap().set_leader(scheduled_leader);
                }

                if my_id == scheduled_leader {
                    num_entries_to_write = i + 1;
                    break;
                }
                start_entry_index = i + 1;
                num_ticks_to_next_vote = TICKS_PER_BLOCK;
            }
        }

        // If leader rotation happened, only write the entries up to leader rotation.
        entries.truncate(num_entries_to_write);
        *last_entry_id = entries
            .last()
            .expect("Entries cannot be empty at this point")
            .id;

        inc_new_counter_info!(
            "replicate-transactions",
            entries.iter().map(|x| x.transactions.len()).sum()
        );

        let entries_len = entries.len() as u64;
        // TODO: In line with previous behavior, this will write all the entries even if
        // an error occurred processing one of the entries (causing the rest of the entries to
        // not be processed).
        if entries_len != 0 {
            ledger_entry_sender.send(entries)?;
        }

        *entry_height += entries_len;
        res?;
        inc_new_counter_info!(
            "replicate_stage-duration",
            duration_as_ms(&now.elapsed()) as usize
        );

        Ok(())
    }

    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        keypair: Arc<Keypair>,
        vote_signer: Arc<VoteSignerProxy>,
        bank: Arc<Bank>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        window_receiver: EntryReceiver,
        exit: Arc<AtomicBool>,
        entry_height: u64,
        last_entry_id: Hash,
    ) -> (Self, EntryReceiver) {
        let (vote_blob_sender, vote_blob_receiver) = channel();
        let (ledger_entry_sender, ledger_entry_receiver) = channel();
        let send = UdpSocket::bind("0.0.0.0:0").expect("bind");
        let t_responder = responder("replay_stage", Arc::new(send), vote_blob_receiver);

        let keypair = Arc::new(keypair);
        let t_replay = Builder::new()
            .name("solana-replay-stage".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit);
                let mut entry_height_ = entry_height;
                let mut last_entry_id = last_entry_id;
                loop {
                    let (leader_id, _) = bank
                        .get_current_leader()
                        .expect("Scheduled leader should be calculated by this point");

                    if leader_id == keypair.pubkey() {
                        inc_new_counter_info!(
                            "replay_stage-new_leader",
                            bank.tick_height() as usize
                        );
                        return Some(ReplayStageReturnType::LeaderRotation(
                            bank.tick_height(),
                            entry_height_,
                            // We should never start the TPU / this stage on an exact entry that causes leader
                            // rotation (Fullnode should automatically transition on startup if it detects
                            // are no longer a validator. Hence we can assume that some entry must have
                            // triggered leader rotation
                            last_entry_id,
                        ));
                    }

                    match Self::process_entries(
                        &bank,
                        &cluster_info,
                        &window_receiver,
                        &keypair,
                        &vote_signer,
                        Some(&vote_blob_sender),
                        &ledger_entry_sender,
                        &mut entry_height_,
                        &mut last_entry_id,
                    ) {
                        Err(Error::RecvTimeoutError(RecvTimeoutError::Disconnected)) => break,
                        Err(Error::RecvTimeoutError(RecvTimeoutError::Timeout)) => (),
                        Err(e) => error!("{:?}", e),
                        Ok(()) => (),
                    }
                }

                None
            })
            .unwrap();

        (
            Self {
                t_responder,
                t_replay,
            },
            ledger_entry_receiver,
        )
    }
}

impl Service for ReplayStage {
    type JoinReturnType = Option<ReplayStageReturnType>;

    fn join(self) -> thread::Result<Option<ReplayStageReturnType>> {
        self.t_responder.join()?;
        self.t_replay.join()
    }
}

#[cfg(test)]
mod test {
    use crate::bank::Bank;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::db_ledger::create_tmp_sample_ledger;
    use crate::db_ledger::{DbLedger, DEFAULT_SLOT_HEIGHT};
    use crate::entry::create_ticks;
    use crate::entry::Entry;
    use crate::fullnode::Fullnode;
    use crate::leader_scheduler::{
        make_active_set_entries, LeaderScheduler, LeaderSchedulerConfig,
    };

    use crate::packet::BlobError;
    use crate::replay_stage::{ReplayStage, ReplayStageReturnType};
    use crate::result::Error;
    use crate::service::Service;
    use crate::vote_signer_proxy::VoteSignerProxy;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_vote_signer::rpc::LocalVoteSigner;
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};

    #[test]
    pub fn test_replay_stage_leader_rotation_exit() {
        solana_logger::setup();

        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);
        let cluster_info_me = ClusterInfo::new(my_node.info.clone());

        // Create keypair for the old leader
        let old_leader_id = Keypair::new().pubkey();

        // Create a ledger
        let num_ending_ticks = 1;
        let (mint, my_ledger_path, genesis_entries) = create_tmp_sample_ledger(
            "test_replay_stage_leader_rotation_exit",
            10_000,
            num_ending_ticks,
            old_leader_id,
            500,
        );
        let mut last_id = genesis_entries
            .last()
            .expect("expected at least one genesis entry")
            .id;

        let my_keypair = Arc::new(my_keypair);
        // Write two entries to the ledger so that the validator is in the active set:
        // 1) Give the validator a nonzero number of tokens 2) A vote from the validator .
        // This will cause leader rotation after the bootstrap height
        let (active_set_entries, vote_account_id) =
            make_active_set_entries(&my_keypair, &mint.keypair(), &last_id, &last_id, 0);
        last_id = active_set_entries.last().unwrap().id;
        let initial_tick_height = genesis_entries
            .iter()
            .skip(2)
            .fold(0, |tick_count, entry| tick_count + entry.is_tick() as u64);
        let active_set_entries_len = active_set_entries.len() as u64;
        let initial_non_tick_height = genesis_entries.len() as u64 - initial_tick_height;
        let initial_entry_len = genesis_entries.len() as u64 + active_set_entries_len;

        {
            let db_ledger = DbLedger::open(&my_ledger_path).unwrap();
            db_ledger
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    genesis_entries.len() as u64,
                    &active_set_entries,
                )
                .unwrap();
        }

        // Set up the LeaderScheduler so that this this node becomes the leader at
        // bootstrap_height = num_bootstrap_slots * leader_rotation_interval
        let leader_rotation_interval = 16;
        let num_bootstrap_slots = 2;
        let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(leader_rotation_interval * 2),
            Some(bootstrap_height),
        );

        let leader_scheduler =
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config)));

        // Set up the bank
        let (bank, _, last_entry_id) =
            Fullnode::new_bank_from_ledger(&my_ledger_path, leader_scheduler);

        // Set up the replay stage
        let (entry_sender, entry_receiver) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        let (replay_stage, ledger_writer_recv) = ReplayStage::new(
            my_keypair,
            Arc::new(vote_account_id),
            Arc::new(bank),
            Arc::new(RwLock::new(cluster_info_me)),
            entry_receiver,
            exit.clone(),
            initial_entry_len,
            last_entry_id,
        );

        // Send enough ticks to trigger leader rotation
        let extra_entries = leader_rotation_interval;
        let total_entries_to_send = (bootstrap_height + extra_entries) as usize;
        let num_hashes = 1;
        let mut entries_to_send = vec![];

        while entries_to_send.len() < total_entries_to_send {
            let entry = Entry::new(&mut last_id, 0, num_hashes, vec![]);
            last_id = entry.id;
            entries_to_send.push(entry);
        }

        assert!((num_ending_ticks as u64) < bootstrap_height);

        // Add on the only entries that weren't ticks to the bootstrap height to get the
        // total expected entry length
        let leader_rotation_index = (bootstrap_height - initial_tick_height - 1) as usize;
        let expected_entry_height =
            bootstrap_height + initial_non_tick_height + active_set_entries_len;
        let expected_last_id = entries_to_send[leader_rotation_index].id;
        entry_sender.send(entries_to_send.clone()).unwrap();

        // Wait for replay_stage to exit and check return value is correct
        assert_eq!(
            Some(ReplayStageReturnType::LeaderRotation(
                bootstrap_height,
                expected_entry_height,
                expected_last_id,
            )),
            replay_stage.join().expect("replay stage join")
        );

        // Check that the entries on the ledger writer channel are correct
        let received_ticks = ledger_writer_recv
            .recv()
            .expect("Expected to recieve an entry on the ledger writer receiver");

        assert_eq!(
            &received_ticks[..],
            &entries_to_send[..leader_rotation_index + 1]
        );

        assert_eq!(exit.load(Ordering::Relaxed), true);

        let _ignored = remove_dir_all(&my_ledger_path);
    }

    #[test]
    fn test_vote_error_replay_stage_correctness() {
        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);

        // Create keypair for the leader
        let leader_id = Keypair::new().pubkey();
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::default()));

        let num_ending_ticks = 0;
        let (_, my_ledger_path, genesis_entries) = create_tmp_sample_ledger(
            "test_vote_error_replay_stage_correctness",
            10_000,
            num_ending_ticks,
            leader_id,
            500,
        );

        let initial_entry_len = genesis_entries.len();

        // Set up the bank
        let (bank, _, last_entry_id) =
            Fullnode::new_bank_from_ledger(&my_ledger_path, leader_scheduler);

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));

        // Set up the replay stage
        let bank = Arc::new(bank);
        let (entry_sender, entry_receiver) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        let my_keypair = Arc::new(my_keypair);
        let vote_signer = Arc::new(VoteSignerProxy::new(
            &my_keypair,
            Box::new(LocalVoteSigner::default()),
        ));
        let (replay_stage, ledger_writer_recv) = ReplayStage::new(
            my_keypair.clone(),
            vote_signer.clone(),
            bank.clone(),
            cluster_info_me.clone(),
            entry_receiver,
            exit.clone(),
            initial_entry_len as u64,
            last_entry_id,
        );

        // Vote sender should error because no leader contact info is found in the
        // ClusterInfo
        let (mock_sender, _mock_receiver) = channel();
        let _vote_err = vote_signer.send_validator_vote(&bank, &cluster_info_me, &mock_sender);

        // Send ReplayStage an entry, should see it on the ledger writer receiver
        let next_tick = create_ticks(
            1,
            genesis_entries
                .last()
                .expect("Expected nonzero number of entries in genesis")
                .id,
        );
        entry_sender
            .send(next_tick.clone())
            .expect("Error sending entry to ReplayStage");
        let received_tick = ledger_writer_recv
            .recv()
            .expect("Expected to recieve an entry on the ledger writer receiver");

        assert_eq!(next_tick, received_tick);
        drop(entry_sender);
        replay_stage
            .join()
            .expect("Expect successful ReplayStage exit");
        let _ignored = remove_dir_all(&my_ledger_path);
    }

    #[test]
    fn test_vote_error_replay_stage_leader_rotation() {
        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);

        // Create keypair for the leader
        let leader_id = Keypair::new().pubkey();

        // Create the ledger
        let (mint, my_ledger_path, genesis_entries) = create_tmp_sample_ledger(
            "test_vote_error_replay_stage_leader_rotation",
            10_000,
            0,
            leader_id,
            500,
        );

        let mut last_id = genesis_entries
            .last()
            .expect("expected at least one genesis entry")
            .id;

        let my_keypair = Arc::new(my_keypair);
        // Write two entries to the ledger so that the validator is in the active set:
        // 1) Give the validator a nonzero number of tokens 2) A vote from the validator.
        // This will cause leader rotation after the bootstrap height
        let (active_set_entries, vote_account_id) =
            make_active_set_entries(&my_keypair, &mint.keypair(), &last_id, &last_id, 0);
        last_id = active_set_entries.last().unwrap().id;
        let initial_tick_height = genesis_entries
            .iter()
            .skip(2)
            .fold(0, |tick_count, entry| tick_count + entry.is_tick() as u64);
        let active_set_entries_len = active_set_entries.len() as u64;
        let initial_non_tick_height = genesis_entries.len() as u64 - initial_tick_height;
        let initial_entry_len = genesis_entries.len() as u64 + active_set_entries_len;

        {
            let db_ledger = DbLedger::open(&my_ledger_path).unwrap();
            db_ledger
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    genesis_entries.len() as u64,
                    &active_set_entries,
                )
                .unwrap();
        }

        // Set up the LeaderScheduler so that this this node becomes the leader at
        // bootstrap_height = num_bootstrap_slots * leader_rotation_interval
        let leader_rotation_interval = 10;
        let num_bootstrap_slots = 2;
        let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(leader_rotation_interval * 2),
            Some(bootstrap_height),
        );

        let leader_scheduler =
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config)));

        // Set up the bank
        let (bank, _, last_entry_id) =
            Fullnode::new_bank_from_ledger(&my_ledger_path, leader_scheduler);

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));

        // Set up the replay stage
        let signer_proxy = Arc::new(vote_account_id);
        let bank = Arc::new(bank);
        let (entry_sender, entry_receiver) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        let (replay_stage, ledger_writer_recv) = ReplayStage::new(
            my_keypair.clone(),
            signer_proxy.clone(),
            bank.clone(),
            cluster_info_me.clone(),
            entry_receiver,
            exit.clone(),
            initial_entry_len as u64,
            last_entry_id,
        );

        // Vote sender should error because no leader contact info is found in the
        // ClusterInfo
        let (mock_sender, _mock_receiver) = channel();
        let _vote_err = signer_proxy.send_validator_vote(&bank, &cluster_info_me, &mock_sender);

        // Send enough ticks to trigger leader rotation
        let total_entries_to_send = (bootstrap_height - initial_tick_height) as usize;
        let num_hashes = 1;

        // Add on the only entries that weren't ticks to the bootstrap height to get the
        // total expected entry length
        let expected_entry_height =
            bootstrap_height + initial_non_tick_height + active_set_entries_len;
        let leader_rotation_index = (bootstrap_height - initial_tick_height - 1) as usize;
        let mut expected_last_id = Hash::default();
        for i in 0..total_entries_to_send {
            let entry = Entry::new(&mut last_id, 0, num_hashes, vec![]);
            last_id = entry.id;
            entry_sender
                .send(vec![entry.clone()])
                .expect("Expected to be able to send entry to ReplayStage");
            // Check that the entries on the ledger writer channel are correct
            let received_entry = ledger_writer_recv
                .recv()
                .expect("Expected to recieve an entry on the ledger writer receiver");
            assert_eq!(received_entry[0], entry);

            if i == leader_rotation_index {
                expected_last_id = entry.id;
            }
        }

        assert_ne!(expected_last_id, Hash::default());

        // Wait for replay_stage to exit and check return value is correct
        assert_eq!(
            Some(ReplayStageReturnType::LeaderRotation(
                bootstrap_height,
                expected_entry_height,
                expected_last_id,
            )),
            replay_stage.join().expect("replay stage join")
        );
        assert_eq!(exit.load(Ordering::Relaxed), true);
        let _ignored = remove_dir_all(&my_ledger_path);
    }

    #[test]
    fn test_replay_stage_poh_error_entry_receiver() {
        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);
        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));
        let (entry_sender, entry_receiver) = channel();
        let (ledger_entry_sender, _ledger_entry_receiver) = channel();
        let mut last_entry_id = Hash::default();
        // Create keypair for the old leader
        let old_leader_id = Keypair::new().pubkey();

        let (_, my_ledger_path, _) = create_tmp_sample_ledger(
            "test_replay_stage_leader_rotation_exit",
            10_000,
            0,
            old_leader_id,
            500,
        );

        let mut entry_height = 0;
        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        for _ in 0..5 {
            let entry = Entry::new(&mut last_id, 0, 1, vec![]); //just ticks
            last_id = entry.id;
            entries.push(entry);
        }
        entry_sender
            .send(entries.clone())
            .expect("Expected to err out");

        let my_keypair = Arc::new(my_keypair);
        let vote_signer = Arc::new(VoteSignerProxy::new(
            &my_keypair,
            Box::new(LocalVoteSigner::default()),
        ));
        let res = ReplayStage::process_entries(
            &Arc::new(Bank::default()),
            &cluster_info_me,
            &entry_receiver,
            &my_keypair,
            &vote_signer,
            None,
            &ledger_entry_sender,
            &mut entry_height,
            &mut last_entry_id,
        );

        match res {
            Ok(_) => (),
            Err(e) => assert!(false, "Entries were not sent correctly {:?}", e),
        }

        entries.clear();
        for _ in 0..5 {
            let entry = Entry::new(&mut Hash::default(), 0, 0, vec![]); //just broken entries
            entries.push(entry);
        }
        entry_sender
            .send(entries.clone())
            .expect("Expected to err out");

        let res = ReplayStage::process_entries(
            &Arc::new(Bank::default()),
            &cluster_info_me,
            &entry_receiver,
            &Arc::new(Keypair::new()),
            &vote_signer,
            None,
            &ledger_entry_sender,
            &mut entry_height,
            &mut last_entry_id,
        );

        match res {
            Ok(_) => assert!(false, "Should have failed because entries are broken"),
            Err(Error::BlobError(BlobError::VerificationFailed)) => (),
            Err(e) => assert!(
                false,
                "Should have failed because with blob error, instead, got {:?}",
                e
            ),
        }
        let _ignored = remove_dir_all(&my_ledger_path);
    }
}
