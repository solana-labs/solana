//! The `replay_stage` replays transactions broadcast by the leader.

use crate::bank::Bank;
use crate::blocktree::Blocktree;
use crate::cluster_info::ClusterInfo;
use crate::counter::Counter;
use crate::entry::{Entry, EntryReceiver, EntrySender, EntrySlice};
use crate::packet::BlobError;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::tvu::TvuRotationSender;
use crate::voting_keypair::VotingKeypair;
use log::Level;
use solana_metrics::{influxdb, submit};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::duration_as_ms;
use solana_sdk::vote_transaction::VoteTransaction;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, SyncSender};
use std::sync::{Arc, RwLock};
#[cfg(test)]
use std::thread::sleep;
use std::thread::{self, Builder, JoinHandle};
#[cfg(test)]
use std::time::Duration;
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
    ledger_signal_sender: SyncSender<bool>,
    #[cfg(test)]
    pause: Arc<AtomicBool>,
}

impl ReplayStage {
    /// Process entry blobs, already in order
    #[allow(clippy::too_many_arguments)]
    fn process_entries(
        mut entries: Vec<Entry>,
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        voting_keypair: Option<&Arc<VotingKeypair>>,
        ledger_entry_sender: &EntrySender,
        current_blob_index: &mut u64,
        last_entry_id: &Arc<RwLock<Hash>>,
    ) -> Result<()> {
        // Coalesce all the available entries into a single vote
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

        let num_ticks = bank.tick_height();
        let mut num_ticks_to_next_vote = bank
            .leader_scheduler
            .read()
            .unwrap()
            .num_ticks_left_in_slot(num_ticks);

        for (i, entry) in entries.iter().enumerate() {
            inc_new_counter_info!("replicate-stage_bank-tick", bank.tick_height() as usize);
            if entry.is_tick() {
                if num_ticks_to_next_vote == 0 {
                    num_ticks_to_next_vote = bank.leader_scheduler.read().unwrap().ticks_per_slot;
                }
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
                res = bank.process_entries(&entries[0..=i]);

                if res.is_err() {
                    // TODO: This will return early from the first entry that has an erroneous
                    // transaction, instead of processing the rest of the entries in the vector
                    // of received entries. This is in line with previous behavior when
                    // bank.process_entries() was used to process the entries, but doesn't solve the
                    // issue that the bank state was still changed, leading to inconsistencies with the
                    // leader as the leader currently should not be publishing erroneous transactions
                    inc_new_counter_info!("replicate-stage_failed_process_entries", i);
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
                num_entries_to_write = i + 1;
                break;
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
            ledger_entry_sender.send(entries)?;
        }

        *current_blob_index += entries_len;
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
        blocktree: Arc<Blocktree>,
        bank: Arc<Bank>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        exit: Arc<AtomicBool>,
        last_entry_id: Arc<RwLock<Hash>>,
        to_leader_sender: &TvuRotationSender,
        ledger_signal_sender: SyncSender<bool>,
        ledger_signal_receiver: Receiver<bool>,
    ) -> (Self, EntryReceiver) {
        let (ledger_entry_sender, ledger_entry_receiver) = channel();
        #[cfg(test)]
        let (pause, pause_) = {
            let pause = Arc::new(AtomicBool::new(false));
            let pause_ = pause.clone();
            (pause, pause_)
        };
        let exit_ = exit.clone();
        let to_leader_sender = to_leader_sender.clone();
        let t_replay = Builder::new()
            .name("solana-replay-stage".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_.clone());
                let mut last_leader_id = Self::get_leader_for_next_tick(&bank);
                let mut prev_slot = None;
                let (mut current_slot, mut max_tick_height_for_slot, mut current_blob_index) = {
                    let tick_height = bank.tick_height();
                    let leader_scheduler = bank.leader_scheduler.read().unwrap();
                    let current_slot = leader_scheduler.tick_height_to_slot(tick_height + 1);
                    let first_tick_in_current_slot = current_slot * leader_scheduler.ticks_per_slot;
                    (
                        Some(current_slot),
                        first_tick_in_current_slot
                            + leader_scheduler.num_ticks_left_in_slot(first_tick_in_current_slot),
                        {
                            let meta = blocktree.meta(current_slot).expect("Database error");
                            if let Some(meta) = meta {
                                meta.consumed
                            } else {
                                0
                            }
                        },
                    )
                };

                // Loop through blocktree MAX_ENTRY_RECV_PER_ITER entries at a time for each
                // relevant slot to see if there are any available updates
                loop {
                    // Stop getting entries if we get exit signal
                    if exit_.load(Ordering::Relaxed) {
                        break;
                    }
                    #[cfg(test)]
                    while pause_.load(Ordering::Relaxed) {
                        sleep(Duration::from_millis(200));
                    }

                    if current_slot.is_none() {
                        let new_slot = Self::get_next_slot(
                            &blocktree,
                            prev_slot.expect("prev_slot must exist"),
                        );
                        if new_slot.is_some() {
                            // Reset the state
                            current_slot = new_slot;
                            current_blob_index = 0;
                            let leader_scheduler = bank.leader_scheduler.read().unwrap();
                            let first_tick_in_current_slot =
                                current_slot.unwrap() * leader_scheduler.ticks_per_slot;
                            max_tick_height_for_slot = first_tick_in_current_slot
                                + leader_scheduler
                                    .num_ticks_left_in_slot(first_tick_in_current_slot);
                        }
                    }

                    let entries = {
                        if let Some(slot) = current_slot {
                            if let Ok(entries) = blocktree.get_slot_entries(
                                slot,
                                current_blob_index,
                                Some(MAX_ENTRY_RECV_PER_ITER as u64),
                            ) {
                                entries
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        }
                    };

                    let entry_len = entries.len();
                    // Fetch the next entries from the database
                    if !entries.is_empty() {
                        if let Err(e) = Self::process_entries(
                            entries,
                            &bank,
                            &cluster_info,
                            voting_keypair.as_ref(),
                            &ledger_entry_sender,
                            &mut current_blob_index,
                            &last_entry_id,
                        ) {
                            error!("process_entries failed: {:?}", e);
                        }

                        let current_tick_height = bank.tick_height();

                        // We've reached the end of a slot, reset our state and check
                        // for leader rotation
                        if max_tick_height_for_slot == current_tick_height {
                            // Check for leader rotation
                            let leader_id = Self::get_leader_for_next_tick(&bank);

                            // TODO: Remove this soon once we boot the leader from ClusterInfo
                            cluster_info.write().unwrap().set_leader(leader_id);

                            if leader_id != last_leader_id && my_id == leader_id {
                                to_leader_sender.send(current_tick_height).unwrap();
                            }

                            // Check for any slots that chain to this one
                            prev_slot = current_slot;
                            current_slot = None;
                            last_leader_id = leader_id;
                            continue;
                        }
                    }

                    // Block until there are updates again
                    if entry_len < MAX_ENTRY_RECV_PER_ITER && ledger_signal_receiver.recv().is_err()
                    {
                        // Update disconnected, exit
                        break;
                    }
                }
            })
            .unwrap();

        (
            Self {
                t_replay,
                exit,
                ledger_signal_sender,
                #[cfg(test)]
                pause,
            },
            ledger_entry_receiver,
        )
    }

    #[cfg(test)]
    pub fn get_pause(&self) -> Arc<AtomicBool> {
        self.pause.clone()
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
        let _ = self.ledger_signal_sender.send(true);
    }

    fn get_leader_for_next_tick(bank: &Bank) -> Pubkey {
        let tick_height = bank.tick_height();
        let leader_scheduler = bank.leader_scheduler.read().unwrap();
        let slot = leader_scheduler.tick_height_to_slot(tick_height + 1);
        leader_scheduler
            .get_leader_for_slot(slot)
            .expect("Scheduled leader should be calculated by this point")
    }

    fn get_next_slot(blocktree: &Blocktree, slot_index: u64) -> Option<u64> {
        // Find the next slot that chains to the old slot
        let next_slots = blocktree.get_slots_since(&[slot_index]).expect("Db error");
        next_slots.first().cloned()
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
    use crate::blocktree::{
        create_tmp_sample_ledger, Blocktree, BlocktreeConfig, DEFAULT_SLOT_HEIGHT,
    };
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::entry::create_ticks;
    use crate::entry::Entry;
    use crate::fullnode::new_bank_from_ledger;
    use crate::genesis_block::GenesisBlock;
    use crate::leader_scheduler::{make_active_set_entries, LeaderSchedulerConfig};
    use crate::replay_stage::ReplayStage;
    use crate::voting_keypair::VotingKeypair;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};

    #[test]
    #[ignore] // TODO: Fix this test to not send all entries in slot 0
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
        let (mint_keypair, my_ledger_path, genesis_entry_height, mut last_id, last_entry_id) =
            create_tmp_sample_ledger(
                "test_replay_stage_leader_rotation_exit",
                10_000,
                0,
                old_leader_id,
                500,
            );

        info!("my_id: {:?}", my_id);
        info!("old_leader_id: {:?}", old_leader_id);

        // Set up the LeaderScheduler so that my_id becomes the leader for epoch 1
        let ticks_per_slot = 16;
        let leader_scheduler_config = LeaderSchedulerConfig::new(ticks_per_slot, 1, ticks_per_slot);

        let my_keypair = Arc::new(my_keypair);
        let (active_set_entries, voting_keypair) = make_active_set_entries(
            &my_keypair,
            &mint_keypair,
            100,
            ticks_per_slot, // add a vote for tick_height = ticks_per_slot
            &last_entry_id,
            &last_id,
            0,
        );
        last_id = active_set_entries.last().unwrap().id;

        {
            let blocktree = Blocktree::open(&my_ledger_path).unwrap();
            blocktree
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    genesis_entry_height,
                    &active_set_entries,
                )
                .unwrap();
        }

        {
            // Set up the bank
            let blocktree_config = BlocktreeConfig::new(ticks_per_slot);
            let (bank, _entry_height, last_entry_id, blocktree, l_sender, l_receiver) =
                new_bank_from_ledger(&my_ledger_path, blocktree_config, &leader_scheduler_config);

            // Set up the replay stage
            let (rotation_sender, rotation_receiver) = channel();
            let meta = blocktree.meta(0).unwrap().unwrap();
            let exit = Arc::new(AtomicBool::new(false));
            let bank = Arc::new(bank);
            let blocktree = Arc::new(blocktree);
            let (replay_stage, ledger_writer_recv) = ReplayStage::new(
                my_id,
                Some(Arc::new(voting_keypair)),
                blocktree.clone(),
                bank.clone(),
                Arc::new(RwLock::new(cluster_info_me)),
                exit.clone(),
                Arc::new(RwLock::new(last_entry_id)),
                &rotation_sender,
                l_sender,
                l_receiver,
            );

            let total_entries_to_send = 2 * ticks_per_slot as usize - 2;
            let mut entries_to_send = vec![];
            while entries_to_send.len() < total_entries_to_send {
                let entry = Entry::new(&mut last_id, 0, 1, vec![]);
                last_id = entry.id;
                entries_to_send.push(entry);
            }

            // Write the entries to the ledger, replay_stage should get notified of changes
            blocktree
                .write_entries(DEFAULT_SLOT_HEIGHT, meta.consumed, &entries_to_send)
                .unwrap();

            info!("Wait for replay_stage to exit and check return value is correct");
            assert_eq!(
                2 * ticks_per_slot - 1,
                rotation_receiver
                    .recv()
                    .expect("should have signaled leader rotation"),
            );

            info!("Check that the entries on the ledger writer channel are correct");
            let mut received_ticks = ledger_writer_recv
                .recv()
                .expect("Expected to receive an entry on the ledger writer receiver");

            while let Ok(entries) = ledger_writer_recv.try_recv() {
                received_ticks.extend(entries);
            }
            assert_eq!(&received_ticks[..], &entries_to_send[..]);

            // Replay stage should continue running even after rotation has happened (tvu never goes down)
            assert_eq!(exit.load(Ordering::Relaxed), false);

            info!("Close replay_stage");
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

        let (_mint_keypair, my_ledger_path, _last_entry_height, _last_id, _last_entry_id) =
            create_tmp_sample_ledger(
                "test_vote_error_replay_stage_correctness",
                10_000,
                1,
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
            let (bank, entry_height, last_entry_id, blocktree, l_sender, l_receiver) =
                new_bank_from_ledger(
                    &my_ledger_path,
                    BlocktreeConfig::default(),
                    &LeaderSchedulerConfig::default(),
                );

            let bank = Arc::new(bank);
            let blocktree = Arc::new(blocktree);
            let (replay_stage, ledger_writer_recv) = ReplayStage::new(
                my_keypair.pubkey(),
                Some(voting_keypair.clone()),
                blocktree.clone(),
                bank.clone(),
                cluster_info_me.clone(),
                exit.clone(),
                Arc::new(RwLock::new(last_entry_id)),
                &to_leader_sender,
                l_sender,
                l_receiver,
            );

            let keypair = voting_keypair.as_ref();
            let vote = VoteTransaction::new_vote(keypair, bank.tick_height(), bank.last_id(), 0);
            cluster_info_me.write().unwrap().push_vote(vote);

            info!("Send ReplayStage an entry, should see it on the ledger writer receiver");
            let next_tick = create_ticks(1, last_entry_id);

            blocktree
                .write_entries(DEFAULT_SLOT_HEIGHT, entry_height, next_tick.clone())
                .unwrap();

            let received_tick = ledger_writer_recv
                .recv()
                .expect("Expected to receive an entry on the ledger writer receiver");

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
        let (mint_keypair, my_ledger_path, genesis_entry_height, last_id, last_entry_id) =
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
        let (active_set_entries, voting_keypair) = make_active_set_entries(
            &my_keypair,
            &mint_keypair,
            100,
            1,
            &last_entry_id,
            &last_id,
            0,
        );
        let mut last_id = active_set_entries.last().unwrap().id;
        let initial_tick_height = genesis_entry_height;

        {
            let blocktree = Blocktree::open(&my_ledger_path).unwrap();
            blocktree
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    genesis_entry_height,
                    &active_set_entries,
                )
                .unwrap();
        }

        let ticks_per_slot = 10;
        let slots_per_epoch = 2;
        let active_window_tick_length = ticks_per_slot * slots_per_epoch;
        let leader_scheduler_config =
            LeaderSchedulerConfig::new(ticks_per_slot, slots_per_epoch, active_window_tick_length);

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));

        // Set up the replay stage
        let (rotation_tx, rotation_rx) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        {
            let blocktree_config = BlocktreeConfig::new(ticks_per_slot);
            let (bank, _entry_height, last_entry_id, blocktree, l_sender, l_receiver) =
                new_bank_from_ledger(&my_ledger_path, blocktree_config, &leader_scheduler_config);

            let meta = blocktree
                .meta(0)
                .unwrap()
                .expect("First slot metadata must exist");

            let voting_keypair = Arc::new(voting_keypair);
            let bank = Arc::new(bank);
            let blocktree = Arc::new(blocktree);
            let (replay_stage, ledger_writer_recv) = ReplayStage::new(
                my_keypair.pubkey(),
                Some(voting_keypair.clone()),
                blocktree.clone(),
                bank.clone(),
                cluster_info_me.clone(),
                exit.clone(),
                Arc::new(RwLock::new(last_entry_id)),
                &rotation_tx,
                l_sender,
                l_receiver,
            );

            let keypair = voting_keypair.as_ref();
            let vote = VoteTransaction::new_vote(keypair, bank.tick_height(), bank.last_id(), 0);
            cluster_info_me.write().unwrap().push_vote(vote);

            // Send enough ticks to trigger leader rotation
            let total_entries_to_send = (active_window_tick_length - initial_tick_height) as usize;
            let num_hashes = 1;

            let leader_rotation_index =
                (active_window_tick_length - initial_tick_height - 1) as usize;
            let mut expected_last_id = Hash::default();
            for i in 0..total_entries_to_send {
                let entry = Entry::new(&mut last_id, 0, num_hashes, vec![]);
                last_id = entry.id;
                blocktree
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
                active_window_tick_length,
                rotation_rx
                    .recv()
                    .expect("should have signaled leader rotation")
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
        let mut current_blob_index = 0;
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
            &Arc::new(Bank::new(&GenesisBlock::new(10_000).0)),
            &cluster_info_me,
            Some(&voting_keypair),
            &ledger_entry_sender,
            &mut current_blob_index,
            &Arc::new(RwLock::new(last_entry_id)),
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
            Some(&voting_keypair),
            &ledger_entry_sender,
            &mut current_blob_index,
            &Arc::new(RwLock::new(last_entry_id)),
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
}
