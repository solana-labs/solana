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
use std::sync::atomic::{AtomicBool, Ordering};
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
        current_slot: u64,
        base_slot: u64,
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

        // if zero, time to vote ;)
        let mut num_ticks_left_in_slot = bank
            .leader_scheduler
            .read()
            .unwrap()
            .num_ticks_left_in_block(current_slot, bank.active_fork().tick_height());

        info!(
            "entries.len(): {}, bank.tick_height(): {} num_ticks_left_in_slot: {}",
            entries.len(),
            bank.active_fork().tick_height(),
            num_ticks_left_in_slot,
        );

        // this code to guard against consuming more ticks in a slot than are actually
        //  allowed by protocol.  entries beyond max_tick_height are silently discarded
        // TODO: slash somebody?
        //  this code also counts down to a vote
        entries.retain(|e| {
            let retain = num_ticks_left_in_slot > 0;
            if num_ticks_left_in_slot > 0 && e.is_tick() {
                num_ticks_left_in_slot -= 1;
            }
            retain
        });
        info!(
            "entries.len(): {}, num_ticks_left_in_slot: {}",
            entries.len(),
            num_ticks_left_in_slot,
        );

        let now = Instant::now();

        if !entries.as_slice().verify(&last_entry_id.read().unwrap()) {
            inc_new_counter_info!("replicate_stage-verify-fail", entries.len());
            return Err(Error::BlobError(BlobError::VerificationFailed));
        }
        inc_new_counter_info!(
            "replicate_stage-verify-duration",
            duration_as_ms(&now.elapsed()) as usize
        );

        inc_new_counter_info!(
            "replicate-stage_bank-tick",
            bank.active_fork().tick_height() as usize
        );
        if bank.fork(current_slot).is_none() {
            bank.init_fork(current_slot, &entries[0].id, base_slot)
                .expect("init fork");
        }
        let res = bank.fork(current_slot).unwrap().process_entries(&entries);

        if res.is_err() {
            // TODO: This will return early from the first entry that has an erroneous
            // transaction, instead of processing the rest of the entries in the vector
            // of received entries. This is in line with previous behavior when
            // bank.process_entries() was used to process the entries, but doesn't solve the
            // issue that the bank state was still changed, leading to inconsistencies with the
            // leader as the leader currently should not be publishing erroneous transactions
            inc_new_counter_info!(
                "replicate-stage_failed_process_entries",
                current_slot as usize
            );

            res?;
        }

        if num_ticks_left_in_slot == 0 {
            let fork = bank.fork(current_slot).expect("current bank state");

            trace!("freezing {} from replay_stage", current_slot);
            fork.head().freeze();
            bank.merge_into_root(current_slot);
            if let Some(voting_keypair) = voting_keypair {
                let keypair = voting_keypair.as_ref();
                let vote =
                    VoteTransaction::new_vote(keypair, fork.tick_height(), fork.last_id(), 0);
                cluster_info.write().unwrap().push_vote(vote);
            }
        }

        // If leader rotation happened, only write the entries up to leader rotation.
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
        mut current_blob_index: u64,
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
                let mut prev_slot = None;
                let (mut current_slot, mut max_tick_height, mut leader_id) = {
                    let current_slot = bank.active_fork().head().fork_id();
                    let leader_scheduler = bank.leader_scheduler.read().unwrap();
                    (
                        Some(current_slot),
                        leader_scheduler.max_tick_height_for_slot(current_slot),
                        leader_scheduler
                            .get_leader_for_slot(current_slot)
                            .expect("leader must be known"),
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
                        let new_slot = get_next_slot(
                            &blocktree,
                            prev_slot.expect("prev_slot must exist"),
                        );
                        if let Some(new_slot) = new_slot {
                            // Reset the state
                            current_slot = Some(new_slot);
                            current_blob_index = 0;
                            max_tick_height = bank
                                .leader_scheduler
                                .read()
                                .unwrap()
                                .max_tick_height_for_slot(new_slot);
                        }
                        info!(
                            "updated to current_slot: {:?} leader_id: {} max_tick_height: {}",
                            current_slot,
                            leader_id,
                            max_tick_height
                        );
                    }

                    if current_slot.is_some() && my_id == leader_id {
                        info!("skip validating current_slot: {:?}", current_slot);
                        // skip validating this slot
                        prev_slot = current_slot;
                        current_slot = None;
                    }

                    let entries = {
                        if let Some(slot) = current_slot {
                            info!("replay getting slot_entries: {} {}", slot, current_blob_index);
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
                    info!("entries: {:?}", entries);

                    let entry_len = entries.len();
                    // Fetch the next entries from the database
                    if !entries.is_empty() {
                        let slot = current_slot.expect("current_slot must exist");

                        // TODO: ledger provides from get_slot_entries()
                        let base_slot = match slot {
                            0 => 0,
                            x => x - 1,
                        };

                        if let Err(e) = Self::process_entries(
                            entries,
                            slot,
                            base_slot,
                            &bank,
                            &cluster_info,
                            voting_keypair.as_ref(),
                            &ledger_entry_sender,
                            &mut current_blob_index,
                            &last_entry_id,
                        ) {
                            error!("process_entries failed: {:?}", e);
                        }

                        let current_tick_height = bank
                            .fork(slot)
                            .expect("fork for current slot must exist")
                            .tick_height();

                        // we've reached the end of a slot, reset our state and check
                        // for leader rotation
                        info!(
                            "max_tick_height: {} current_tick_height: {}",
                            max_tick_height,
                            current_tick_height
                        );

                        if max_tick_height == current_tick_height {
                            // Check for leader rotation
                            let next_leader_id = bank
                                .leader_scheduler
                                .read()
                                .unwrap()
                                .get_leader_for_slot(slot + 1)
                                .expect("Scheduled leader should be calculated by this point");

                            info!(
                                "next_leader_id: {} leader_id_for_slot: {} my_id: {}",
                                next_leader_id,
                                leader_id,
                                my_id
                            );

                            if leader_id != next_leader_id {
                                if my_id == leader_id {
                                    // construct the leader's bank_state for it
                                    bank.init_fork(slot + 1, &last_entry_id.read().unwrap(), slot)
                                        .expect("init fork");

                                    to_leader_sender.send(current_tick_height).unwrap();
                                } else {
                                    // TODO: Remove this soon once we boot the leader from ClusterInfo
                                    cluster_info.write().unwrap().set_leader(leader_id);
                                }
                            }

                            // update slot enumeration state
                            prev_slot = current_slot;
                            current_slot = None;
                            leader_id = next_leader_id;
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
}

pub fn get_next_slot(blocktree: &Blocktree, slot_index: u64) -> Option<u64> {
    // Find the next slot that chains to the old slot
    let next_slots = blocktree.get_slots_since(&[slot_index]).expect("Db error");
    next_slots.first().cloned()
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
    use crate::entry::{next_entry_mut, Entry};
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

        // Set up the LeaderScheduler so that my_id becomes the leader for epoch 1
        let ticks_per_slot = 16;
        let leader_scheduler_config = LeaderSchedulerConfig::new(ticks_per_slot, 1, ticks_per_slot);

        // Create a ledger
        let (
            mint_keypair,
            my_ledger_path,
            mut tick_height,
            entry_height,
            mut last_id,
            last_entry_id,
        ) = create_tmp_sample_ledger(
            "test_replay_stage_leader_rotation_exit",
            10_000,
            0,
            old_leader_id,
            500,
            &BlocktreeConfig::new(ticks_per_slot),
        );

        info!("my_id: {:?}", my_id);
        info!("old_leader_id: {:?}", old_leader_id);

        let my_keypair = Arc::new(my_keypair);
        let num_ending_ticks = 0;
        let (active_set_entries, voting_keypair) = make_active_set_entries(
            &my_keypair,
            &mint_keypair,
            100,
            ticks_per_slot, // add a vote for tick_height = ticks_per_slot
            &last_entry_id,
            &last_id,
            num_ending_ticks,
        );
        last_id = active_set_entries.last().unwrap().id;

        {
            let blocktree = Blocktree::open(&my_ledger_path).unwrap();
            blocktree
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    tick_height,
                    entry_height,
                    active_set_entries,
                )
                .unwrap();
            tick_height += num_ending_ticks;
        }

        {
            // Set up the bank
            let blocktree_config = BlocktreeConfig::new(ticks_per_slot);
            let (bank, _entry_height, last_entry_id, blocktree, l_sender, l_receiver) =
                new_bank_from_ledger(&my_ledger_path, &blocktree_config, &leader_scheduler_config);

            // Set up the replay stage
            let (to_leader_sender, rotation_receiver) = channel();
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
                meta.consumed,
                Arc::new(RwLock::new(last_entry_id)),
                &to_leader_sender,
                l_sender,
                l_receiver,
            );

            let total_entries_to_send = 2 * ticks_per_slot as usize - 2;
            let mut entries_to_send = vec![];
            while entries_to_send.len() < total_entries_to_send {
                let entry = next_entry_mut(&mut last_id, 1, vec![]);
                entries_to_send.push(entry);
            }

            // Write the entries to the ledger, replay_stage should get notified of changes
            blocktree
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    tick_height,
                    meta.consumed,
                    &entries_to_send,
                )
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
        solana_logger::setup();
        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);

        // Create keypair for the leader
        let leader_id = Keypair::new().pubkey();

        let (
            _mint_keypair,
            my_ledger_path,
            tick_height,
            _last_entry_height,
            _last_id,
            _last_entry_id,
        ) = create_tmp_sample_ledger(
            "test_vote_error_replay_stage_correctness",
            10_000,
            1,
            leader_id,
            500,
            &BlocktreeConfig::default(),
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
                    &BlocktreeConfig::default(),
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
                entry_height,
                Arc::new(RwLock::new(last_entry_id)),
                &to_leader_sender,
                l_sender,
                l_receiver,
            );

            let keypair = voting_keypair.as_ref();
            let vote = VoteTransaction::new_vote(
                keypair,
                bank.active_fork().tick_height(),
                bank.active_fork().last_id(),
                0,
            );

            cluster_info_me.write().unwrap().push_vote(vote);

            info!("Send ReplayStage an entry, should see it on the ledger writer receiver");
            let next_tick = create_ticks(1, last_entry_id);
            info!("tick: {:?}", next_tick);
            blocktree
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    tick_height,
                    entry_height,
                    next_tick.clone(),
                )
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

        let ticks_per_slot = 10;
        let slots_per_epoch = 2;
        let active_window_tick_length = ticks_per_slot * slots_per_epoch;

        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);

        // Create keypair for the leader
        let leader_id = Keypair::new().pubkey();

        // Create the ledger
        let blocktree_config = BlocktreeConfig::new(ticks_per_slot);
        let (
            mint_keypair,
            my_ledger_path,
            tick_height,
            genesis_entry_height,
            last_id,
            last_entry_id,
        ) = create_tmp_sample_ledger(
            "test_vote_error_replay_stage_leader_rotation",
            10_000,
            1,
            leader_id,
            500,
            &blocktree_config,
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
        {
            let blocktree = Blocktree::open_config(&my_ledger_path, &blocktree_config).unwrap();
            blocktree
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    tick_height,
                    genesis_entry_height,
                    &active_set_entries,
                )
                .unwrap();
        }

        let leader_scheduler_config =
            LeaderSchedulerConfig::new(ticks_per_slot, slots_per_epoch, active_window_tick_length);

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));

        // Set up the replay stage
        let (rotation_tx, rotation_rx) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        {
            let (bank, _entry_height, last_entry_id, blocktree, l_sender, l_receiver) =
                new_bank_from_ledger(&my_ledger_path, &blocktree_config, &leader_scheduler_config);

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
                meta.consumed,
                Arc::new(RwLock::new(last_entry_id)),
                &rotation_tx,
                l_sender,
                l_receiver,
            );

            let keypair = voting_keypair.as_ref();
            let vote = VoteTransaction::new_vote(
                keypair,
                bank.active_fork().tick_height(),
                bank.active_fork().last_id(),
                0,
            );
            cluster_info_me.write().unwrap().push_vote(vote);

            // Send enough ticks to trigger leader rotation
            let total_entries_to_send = (active_window_tick_length - tick_height) as usize;
            let num_hashes = 1;

            let leader_rotation_index = (active_window_tick_length - tick_height - 1) as usize;
            let mut expected_last_id = Hash::default();
            for i in 0..total_entries_to_send {
                let entry = next_entry_mut(&mut last_id, num_hashes, vec![]);
                blocktree
                    .write_entries(
                        DEFAULT_SLOT_HEIGHT,
                        tick_height + i as u64,
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
        solana_logger::setup();
        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);
        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));
        let (ledger_entry_sender, _ledger_entry_receiver) = channel();
        let genesis_block = GenesisBlock::new(10_000).0;
        let last_entry_id = genesis_block.last_id();

        let mut current_blob_index = 0;
        let bank = Arc::new(Bank::new(&genesis_block));

        let mut entries = Vec::new();
        {
            let mut last_id = last_entry_id;
            for _ in 0..5 {
                let entry = next_entry_mut(&mut last_id, 1, vec![]); //just ticks
                entries.push(entry);
            }
        }

        let my_keypair = Arc::new(my_keypair);
        let voting_keypair = Arc::new(VotingKeypair::new_local(&my_keypair));
        let res = ReplayStage::process_entries(
            entries.clone(),
            0,
            0,
            &bank,
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
            0,
            0,
            &bank,
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
