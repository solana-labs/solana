//! The `replay_stage` replays transactions broadcast by the leader.

use crate::bank::Bank;
use crate::cluster_info::ClusterInfo;
use crate::counter::Counter;
use crate::db_ledger::DbLedger;
use crate::entry::{Entry, EntryReceiver, EntrySender, EntrySlice};
#[cfg(not(test))]
use crate::entry_stream::EntryStream;
use crate::entry_stream::EntryStreamHandler;
#[cfg(test)]
use crate::entry_stream::MockEntryStream as EntryStream;
use crate::fullnode::TvuRotationSender;
use crate::leader_scheduler::DEFAULT_TICKS_PER_SLOT;
use crate::packet::BlobError;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::tvu::TvuReturnType;
use crate::voting_keypair::VotingKeypair;
use log::Level;
use solana_metrics::{influxdb, submit};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::duration_as_ms;
use solana_sdk::vote_transaction::VoteTransaction;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Instant;

pub const MAX_ENTRY_RECV_PER_ITER: usize = 512;

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
    t_replay: JoinHandle<()>,
    exit: Arc<AtomicBool>,
    db_ledger: Arc<DbLedger>,
}

impl ReplayStage {
    /// Process entry blobs, already in order
    #[allow(clippy::too_many_arguments)]
    fn process_entries(
        mut entries: Vec<Entry>,
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        my_id: Pubkey,
        voting_keypair: Option<&Arc<VotingKeypair>>,
        ledger_entry_sender: &EntrySender,
        entry_height: &Arc<RwLock<u64>>,
        last_entry_id: &Arc<RwLock<Hash>>,
        entry_stream: Option<&mut EntryStream>,
    ) -> Result<()> {
        if let Some(stream) = entry_stream {
            stream.stream_entries(&entries).unwrap_or_else(|e| {
                error!("Entry Stream error: {:?}, {:?}", e, stream.socket);
            });
        }
        //coalesce all the available entries into a single vote
        submit(
            influxdb::Point::new("replicate-stage")
                .add_field("count", influxdb::Value::Integer(entries.len() as i64))
                .to_owned(),
        );

        let mut res = Ok(());
        let mut num_entries_to_write = entries.len();
        let now = Instant::now();
        if !entries.as_slice().verify(&last_entry_id.read().unwrap()) {
            inc_new_counter_info!("replicate_stage-verify-fail", entries.len());
            return Err(Error::BlobError(BlobError::VerificationFailed));
        }
        inc_new_counter_info!(
            "replicate_stage-verify-duration",
            duration_as_ms(&now.elapsed()) as usize
        );

        let (mut current_leader, _) = bank
            .get_current_leader()
            .expect("Scheduled leader should be calculated by this point");
        let already_leader = my_id == current_leader;
        let mut did_rotate = false;

        // Next vote tick is ceiling of (current tick/ticks per block)
        let mut num_ticks_to_next_vote =
            DEFAULT_TICKS_PER_SLOT - (bank.tick_height() % DEFAULT_TICKS_PER_SLOT);
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
                    if let Some(voting_keypair) = voting_keypair {
                        let keypair = voting_keypair.as_ref();
                        let vote = VoteTransaction::new_vote(
                            keypair,
                            bank.tick_height(),
                            bank.last_id(),
                            0,
                        );
                        cluster_info.write().unwrap().push_vote(vote);
                    }
                }
                let (scheduled_leader, _) = bank
                    .get_current_leader()
                    .expect("Scheduled leader should be calculated by this point");

                // TODO: Remove this soon once we boot the leader from ClusterInfo
                if scheduled_leader != current_leader {
                    did_rotate = true;
                    cluster_info.write().unwrap().set_leader(scheduled_leader);
                    current_leader = scheduled_leader
                }

                if !already_leader && my_id == scheduled_leader && did_rotate {
                    num_entries_to_write = i + 1;
                    break;
                }
                start_entry_index = i + 1;
                num_ticks_to_next_vote = DEFAULT_TICKS_PER_SLOT;
            }
        }

        // If leader rotation happened, only write the entries up to leader rotation.
        entries.truncate(num_entries_to_write);
        *last_entry_id.write().unwrap() = entries
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
            println!("{:?}, Sending {} entries", my_id, entries.len());
            ledger_entry_sender.send(entries)?;
        }

        *entry_height.write().unwrap() += entries_len;

        println!("returning from process_entries()");
        res?;
        inc_new_counter_info!(
            "replicate_stage-duration",
            duration_as_ms(&now.elapsed()) as usize
        );
        Ok(())
    }

    #[allow(clippy::new_ret_no_self, clippy::too_many_arguments)]
    pub fn new(
        my_id: Pubkey,
        voting_keypair: Option<Arc<VotingKeypair>>,
        db_ledger: Arc<DbLedger>,
        bank: Arc<Bank>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        exit: Arc<AtomicBool>,
        entry_height: Arc<RwLock<u64>>,
        last_entry_id: Arc<RwLock<Hash>>,
        to_leader_sender: TvuRotationSender,
        entry_stream: Option<&String>,
    ) -> (Self, EntryReceiver) {
        let (ledger_entry_sender, ledger_entry_receiver) = channel();
        let mut entry_stream = entry_stream.cloned().map(EntryStream::new);

        let (_, mut current_slot) = bank
            .get_current_leader()
            .expect("Scheduled leader should be calculated by this point");

        let mut max_tick_height_for_slot = bank
            .leader_scheduler
            .read()
            .unwrap()
            .max_tick_height_for_slot(current_slot);

        println!(
            "mthfs: {}, current_slot: {}",
            max_tick_height_for_slot, current_slot
        );

        let db_ledger_ = db_ledger.clone();
        let exit_ = exit.clone();
        let t_replay = Builder::new()
            .name("solana-replay-stage".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_.clone());
                let (mut last_leader_id, _) = bank
                    .get_current_leader()
                    .expect("Scheduled leader should be calculated by this point");
                'outer: loop {
                    // Loop through db_ledger MAX_ENTRY_RECV_PER_ITER entries at a time for each
                    // relevant slot to see if there are any available updates
                    loop {
                        // Stop getting entries if we get exit signal
                        if exit_.load(Ordering::Relaxed) {
                            break 'outer;
                        }

                        let current_entry_height = *entry_height.read().unwrap();
                        println!(
                            "{:?}, Fetching entries from db for slot: {:?}, {}",
                            my_id, current_slot, current_entry_height,
                        );
                        // Fetch the next entries from the database
                        if let Ok(entries) = db_ledger.get_slot_entries(
                            current_slot,
                            current_entry_height,
                            Some(MAX_ENTRY_RECV_PER_ITER as u64),
                        ) {
                            if entries.is_empty() {
                                println!("Fetched zero entries");
                                break;
                            }
                            if let Err(e) = Self::process_entries(
                                entries,
                                &bank,
                                &cluster_info,
                                my_id,
                                voting_keypair.as_ref(),
                                &ledger_entry_sender,
                                &entry_height,
                                &last_entry_id,
                                entry_stream.as_mut(),
                            ) {
                                error!("{:?}", e);
                            }
                        } else {
                            break;
                        }

                        let current_tick_height = bank.tick_height();

                        println!(
                            "{:?}, current_slot: {:?} mthfs: {}, cth: {}",
                            my_id, current_slot, max_tick_height_for_slot, current_tick_height
                        );
                        // We've reached the end of a slot, reset our state and check
                        // for leader rotation
                        if max_tick_height_for_slot == current_tick_height {
                            // Check for leader rotation
                            let leader_id = Self::get_leader(&bank, &cluster_info);
                            println!("{:?}, next_leader: {}", my_id, leader_id);
                            if leader_id != last_leader_id && my_id == leader_id {
                                to_leader_sender
                                    .send(TvuReturnType::LeaderRotation(
                                        bank.tick_height(),
                                        *entry_height.read().unwrap(),
                                        *last_entry_id.read().unwrap(),
                                    ))
                                    .unwrap();
                            }

                            current_slot += 1;
                            max_tick_height_for_slot = bank
                                .leader_scheduler
                                .read()
                                .unwrap()
                                .max_tick_height_for_slot(current_slot);
                            last_leader_id = leader_id;
                            continue;
                        }

                        println!(
                            "{:?}, current_slot: {}, bi: {}",
                            my_id,
                            current_slot,
                            *entry_height.read().unwrap()
                        );
                    }

                    // Block until there are updates again
                    {
                        let (cvar, lock) = &db_ledger.new_blobs_signal;
                        let mut has_updates = lock.lock().unwrap();
                        loop {
                            // Check for exit signal
                            if exit_.load(Ordering::Relaxed) {
                                break 'outer;
                            }

                            // Check boolean predicate to protect against spurious wakeups
                            if !*has_updates {
                                has_updates = cvar.wait(has_updates).unwrap();
                            } else {
                                break;
                            }
                        }

                        *has_updates = false;
                    }
                }
            })
            .unwrap();

        (
            Self {
                t_replay,
                exit,
                db_ledger: db_ledger_,
            },
            ledger_entry_receiver,
        )
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
    }

    pub fn exit(&self) {
        let (cvar, lock) = &self.db_ledger.new_blobs_signal;
        self.exit.store(true, Ordering::Relaxed);
        let _ = lock.lock().unwrap();
        cvar.notify_all();
    }

    fn get_leader(bank: &Bank, cluster_info: &Arc<RwLock<ClusterInfo>>) -> Pubkey {
        let (scheduled_leader, _) = bank
            .get_current_leader()
            .expect("Scheduled leader should be calculated by this point");

        // TODO: Remove this soon once we boot the leader from ClusterInfo
        cluster_info.write().unwrap().set_leader(scheduled_leader);

        scheduled_leader
    }
}

impl Service for ReplayStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_replay.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bank::Bank;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::db_ledger::create_tmp_sample_ledger;
    use crate::db_ledger::{DbLedger, DEFAULT_SLOT_HEIGHT};
    use crate::entry::create_ticks;
    use crate::entry::Entry;
    use crate::fullnode::Fullnode;
    use crate::genesis_block::GenesisBlock;
    use crate::leader_scheduler::{
        make_active_set_entries, LeaderScheduler, LeaderSchedulerConfig,
    };
    use crate::replay_stage::ReplayStage;
    use crate::tvu::TvuReturnType;
    use crate::voting_keypair::VotingKeypair;
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};

    #[test]
    #[ignore]
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
        let num_ending_ticks = 3;
        let (mint_keypair, my_ledger_path, genesis_entry_height, mut last_id) =
            create_tmp_sample_ledger(
                "test_replay_stage_leader_rotation_exit",
                10_000,
                num_ending_ticks,
                old_leader_id,
                500,
            );

        let my_keypair = Arc::new(my_keypair);
        // Write two entries to the ledger so that the validator is in the active set:
        // 1) Give the validator a nonzero number of tokens 2) A vote from the validator.
        // This will cause leader rotation after the bootstrap height
        let (active_set_entries, voting_keypair) =
            make_active_set_entries(&my_keypair, &mint_keypair, &last_id, &last_id, 0);
        last_id = active_set_entries.last().unwrap().id;
        let initial_tick_height = genesis_entry_height;
        let active_set_entries_len = active_set_entries.len() as u64;
        let initial_non_tick_height = genesis_entry_height - initial_tick_height;

        {
            // Set up the LeaderScheduler so that this this node becomes the leader at
            // bootstrap_height = num_bootstrap_slots * leader_rotation_interval
            let leader_rotation_interval = 16;
            let bootstrap_height = 2 * leader_rotation_interval;
            assert!((num_ending_ticks as u64) < bootstrap_height);
            let leader_scheduler_config = LeaderSchedulerConfig::new(
                bootstrap_height,
                leader_rotation_interval,
                leader_rotation_interval * 2,
                bootstrap_height,
            );

            let leader_scheduler =
                Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config)));

            let db_ledger = Arc::new(DbLedger::open(&my_ledger_path).unwrap());
            db_ledger
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    genesis_entry_height,
                    &active_set_entries,
                )
                .unwrap();

            let genesis_block = GenesisBlock::load(&my_ledger_path)
                .expect("Expected to successfully open genesis block");

            // Set up the bank
            let (bank, _, last_entry_id) =
                Fullnode::new_bank_from_db_ledger(&genesis_block, &db_ledger, leader_scheduler);

            // Set up the replay stage
            let (rotation_sender, rotation_receiver) = channel();
            let meta = db_ledger.meta().unwrap().unwrap();
            let exit = Arc::new(AtomicBool::new(false));
            let (replay_stage, ledger_writer_recv) = ReplayStage::new(
                my_id,
                Some(Arc::new(voting_keypair)),
                db_ledger.clone(),
                Arc::new(bank),
                Arc::new(RwLock::new(cluster_info_me)),
                exit.clone(),
                Arc::new(RwLock::new(meta.consumed)),
                Arc::new(RwLock::new(last_entry_id)),
                rotation_sender,
                None,
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
            let leader_rotation_index = (bootstrap_height - initial_tick_height) as usize;
            let expected_entry_height =
                bootstrap_height + initial_non_tick_height + active_set_entries_len;
            let expected_last_id = entries_to_send[leader_rotation_index - 1].id;

            // Write the entries to the ledger, replay_stage should get notified of changes
            db_ledger
                .write_entries(DEFAULT_SLOT_HEIGHT, meta.consumed, &entries_to_send)
                .unwrap();

            // Wait for replay_stage to exit and check return value is correct
            assert_eq!(
                Some(TvuReturnType::LeaderRotation(
                    bootstrap_height,
                    expected_entry_height,
                    expected_last_id,
                )),
                {
                    Some(
                        rotation_receiver
                            .recv()
                            .expect("should have signaled leader rotation"),
                    )
                }
            );

            // Check that the entries on the ledger writer channel are correct

            let mut received_ticks = ledger_writer_recv
                .recv()
                .expect("Expected to recieve an entry on the ledger writer receiver");

            while let Ok(entries) = ledger_writer_recv.try_recv() {
                received_ticks.extend(entries);
            }

            assert_eq!(
                &received_ticks[..],
                &entries_to_send[..leader_rotation_index]
            );

            //replay stage should continue running even after rotation has happened (tvu never goes down)
            assert_eq!(exit.load(Ordering::Relaxed), false);
            //force exit
            replay_stage
                .close()
                .expect("Expect successful ReplayStage exit");
        }
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

        let num_ending_ticks = 1;
        let (_, my_ledger_path, _, _) = create_tmp_sample_ledger(
            "test_vote_error_replay_stage_correctness",
            10_000,
            num_ending_ticks,
            leader_id,
            500,
        );

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));

        // Set up the replay stage
        let exit = Arc::new(AtomicBool::new(false));
        let my_keypair = Arc::new(my_keypair);
        let voting_keypair = Arc::new(VotingKeypair::new_local(&my_keypair));
        let (to_leader_sender, _) = channel();
        {
            let db_ledger = Arc::new(DbLedger::open(&my_ledger_path).unwrap());
            // Set up the bank
            let genesis_block = GenesisBlock::load(&my_ledger_path)
                .expect("Expected to successfully open genesis block");
            let (bank, entry_height, last_entry_id) =
                Fullnode::new_bank_from_db_ledger(&genesis_block, &db_ledger, leader_scheduler);
            let bank = Arc::new(bank);
            let (replay_stage, ledger_writer_recv) = ReplayStage::new(
                my_keypair.pubkey(),
                Some(voting_keypair.clone()),
                db_ledger.clone(),
                bank.clone(),
                cluster_info_me.clone(),
                exit.clone(),
                Arc::new(RwLock::new(entry_height)),
                Arc::new(RwLock::new(last_entry_id)),
                to_leader_sender,
                None,
            );

            let keypair = voting_keypair.as_ref();
            let vote = VoteTransaction::new_vote(keypair, bank.tick_height(), bank.last_id(), 0);
            cluster_info_me.write().unwrap().push_vote(vote);

            // Send ReplayStage an entry, should see it on the ledger writer receiver
            let next_tick = create_ticks(1, last_entry_id);

            db_ledger
                .write_entries(DEFAULT_SLOT_HEIGHT, entry_height, next_tick.clone())
                .unwrap();

            let received_tick = ledger_writer_recv
                .recv()
                .expect("Expected to recieve an entry on the ledger writer receiver");

            assert_eq!(next_tick, received_tick);

            replay_stage
                .close()
                .expect("Expect successful ReplayStage exit");
        }
        let _ignored = remove_dir_all(&my_ledger_path);
    }

    #[test]
    #[ignore]
    fn test_vote_error_replay_stage_leader_rotation() {
        solana_logger::setup();

        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);

        // Create keypair for the leader
        let leader_id = Keypair::new().pubkey();

        // Create the ledger
        let (mint_keypair, my_ledger_path, genesis_entry_height, last_id) =
            create_tmp_sample_ledger(
                "test_vote_error_replay_stage_leader_rotation",
                10_000,
                1,
                leader_id,
                500,
            );

        let my_keypair = Arc::new(my_keypair);
        // Write two entries to the ledger so that the validator is in the active set:
        // 1) Give the validator a nonzero number of tokens 2) A vote from the validator.
        // This will cause leader rotation after the bootstrap height
        let (active_set_entries, voting_keypair) =
            make_active_set_entries(&my_keypair, &mint_keypair, &last_id, &last_id, 0);
        let mut last_id = active_set_entries.last().unwrap().id;
        let initial_tick_height = genesis_entry_height;
        let active_set_entries_len = active_set_entries.len() as u64;
        let initial_non_tick_height = genesis_entry_height - initial_tick_height;

        // Set up the LeaderScheduler so that this this node becomes the leader at
        // bootstrap_height = num_bootstrap_slots * leader_rotation_interval
        // Set up the LeaderScheduler so that this this node becomes the leader at
        // bootstrap_height = num_bootstrap_slots * leader_rotation_interval
        let leader_rotation_interval = 10;
        let num_bootstrap_slots = 2;
        let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            bootstrap_height,
            leader_rotation_interval,
            leader_rotation_interval * 2,
            bootstrap_height,
        );

        let leader_scheduler =
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config)));

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));

        // Set up the replay stage
        let (rotation_tx, rotation_rx) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        {
            let db_ledger = Arc::new(DbLedger::open(&my_ledger_path).unwrap());
            db_ledger
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    genesis_entry_height,
                    &active_set_entries,
                )
                .unwrap();
            let meta = db_ledger
                .meta()
                .unwrap()
                .expect("First slot metadata must exist");

            // Set up the bank
            let genesis_block = GenesisBlock::load(&my_ledger_path)
                .expect("Expected to successfully open genesis block");
            let (bank, _, last_entry_id) =
                Fullnode::new_bank_from_db_ledger(&genesis_block, &db_ledger, leader_scheduler);

            let voting_keypair = Arc::new(voting_keypair);
            let bank = Arc::new(bank);
            let (replay_stage, ledger_writer_recv) = ReplayStage::new(
                my_keypair.pubkey(),
                Some(voting_keypair.clone()),
                db_ledger.clone(),
                bank.clone(),
                cluster_info_me.clone(),
                exit.clone(),
                Arc::new(RwLock::new(meta.consumed)),
                Arc::new(RwLock::new(last_entry_id)),
                rotation_tx,
                None,
            );

            let keypair = voting_keypair.as_ref();
            let vote = VoteTransaction::new_vote(keypair, bank.tick_height(), bank.last_id(), 0);
            cluster_info_me.write().unwrap().push_vote(vote);

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
                db_ledger
                    .write_entries(
                        DEFAULT_SLOT_HEIGHT,
                        meta.consumed + i as u64,
                        vec![entry.clone()],
                    )
                    .expect("Expected successful database write");
                // Check that the entries on the ledger writer channel are correct
                let received_entry = ledger_writer_recv
                    .recv()
                    .expect("Expected to recieve an entry on the ledger writer receiver");
                assert_eq!(received_entry[0], entry);

                if i == leader_rotation_index {
                    expected_last_id = entry.id;
                }
            }

            // Wait for replay_stage to exit and check return value is correct
            assert_eq!(
                Some(TvuReturnType::LeaderRotation(
                    bootstrap_height,
                    expected_entry_height,
                    expected_last_id,
                )),
                {
                    Some(
                        rotation_rx
                            .recv()
                            .expect("should have signaled leader rotation"),
                    )
                }
            );
            assert_ne!(expected_last_id, Hash::default());
            //replay stage should continue running even after rotation has happened (tvu never goes down)
            replay_stage
                .close()
                .expect("Expect successful ReplayStage exit");
        }
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
        let (ledger_entry_sender, _ledger_entry_receiver) = channel();
        let last_entry_id = Hash::default();

        let entry_height = 0;
        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        for _ in 0..5 {
            let entry = Entry::new(&mut last_id, 0, 1, vec![]); //just ticks
            last_id = entry.id;
            entries.push(entry);
        }

        let my_keypair = Arc::new(my_keypair);
        let voting_keypair = Arc::new(VotingKeypair::new_local(&my_keypair));
        let res = ReplayStage::process_entries(
            entries.clone(),
            &Arc::new(Bank::default()),
            &cluster_info_me,
            my_id,
            Some(&voting_keypair),
            &ledger_entry_sender,
            &Arc::new(RwLock::new(entry_height)),
            &Arc::new(RwLock::new(last_entry_id)),
            None,
        );

        match res {
            Ok(_) => (),
            Err(e) => assert!(false, "Entries were not sent correctly {:?}", e),
        }

        entries.clear();
        for _ in 0..5 {
            let entry = Entry::new(&mut Hash::default(), 0, 1, vec![]); //just broken entries
            entries.push(entry);
        }

        let res = ReplayStage::process_entries(
            entries.clone(),
            &Arc::new(Bank::default()),
            &cluster_info_me,
            Keypair::new().pubkey(),
            Some(&voting_keypair),
            &ledger_entry_sender,
            &Arc::new(RwLock::new(entry_height)),
            &Arc::new(RwLock::new(last_entry_id)),
            None,
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
    }

    #[test]
    fn test_replay_stage_stream_entries() {
        // Set up entry stream
        let mut entry_stream = EntryStream::new("test_stream".to_string());

        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);
        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));
        let (ledger_entry_sender, _ledger_entry_receiver) = channel();
        let last_entry_id = Hash::default();

        let entry_height = 0;
        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        let mut expected_entries = Vec::new();
        for _ in 0..5 {
            let entry = Entry::new(&mut last_id, 0, 1, vec![]); //just ticks
            last_id = entry.id;
            expected_entries.push(entry.clone());
            entries.push(entry);
        }

        let my_keypair = Arc::new(my_keypair);
        let voting_keypair = Arc::new(VotingKeypair::new_local(&my_keypair));
        ReplayStage::process_entries(
            entries.clone(),
            &Arc::new(Bank::default()),
            &cluster_info_me,
            my_id,
            Some(&voting_keypair),
            &ledger_entry_sender,
            &Arc::new(RwLock::new(entry_height)),
            &Arc::new(RwLock::new(last_entry_id)),
            Some(&mut entry_stream),
        )
        .unwrap();

        assert_eq!(entry_stream.socket.len(), 5);

        for (i, item) in entry_stream.socket.iter().enumerate() {
            let json: Value = serde_json::from_str(&item).unwrap();
            let dt_str = json["dt"].as_str().unwrap();

            // Ensure `ts` field parses as valid DateTime
            let _dt: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(dt_str).unwrap();

            let entry_obj = json["entry"].clone();
            let entry: Entry = serde_json::from_value(entry_obj).unwrap();
            assert_eq!(entry, expected_entries[i]);
        }
    }
}
