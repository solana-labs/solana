//! The `replay_stage` replays transactions broadcast by the leader.

use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::blocktree_processor::{self, BankForksInfo};
use crate::cluster_info::ClusterInfo;
use crate::entry::{Entry, EntryReceiver, EntrySender, EntrySlice};
use crate::leader_scheduler::LeaderScheduler;
use crate::packet::BlobError;
use crate::result::{Error, Result};
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::tvu::{TvuRotationInfo, TvuRotationSender};
use log::Level;
use solana_metrics::counter::Counter;
use solana_metrics::{influxdb, submit};
use solana_runtime::bank::Bank;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::KeypairUtil;
use solana_sdk::timing::duration_as_ms;
use solana_sdk::vote_transaction::VoteTransaction;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
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
}

impl ReplayStage {
    /// Process entry blobs, already in order
    #[allow(clippy::too_many_arguments)]
    fn process_entries<T: KeypairUtil>(
        mut entries: Vec<Entry>,
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        voting_keypair: &Option<Arc<T>>,
        ledger_entry_sender: &EntrySender,
        current_blob_index: &mut u64,
        last_entry_id: &Arc<RwLock<Hash>>,
        leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
        subscriptions: &Arc<RpcSubscriptions>,
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
        let (mut num_ticks_to_next_vote, slot_height) = {
            let rl = leader_scheduler.read().unwrap();
            (
                rl.num_ticks_left_in_slot(num_ticks),
                rl.tick_height_to_slot(num_ticks),
            )
        };

        for (i, entry) in entries.iter().enumerate() {
            inc_new_counter_info!("replicate-stage_bank-tick", bank.tick_height() as usize);
            if entry.is_tick() {
                if num_ticks_to_next_vote == 0 {
                    num_ticks_to_next_vote = bank.ticks_per_slot();
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
                res = blocktree_processor::process_entries(bank, &entries[0..=i], leader_scheduler);

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
                    subscriptions.notify_subscribers(&bank);
                    if let Some(voting_keypair) = voting_keypair {
                        let keypair = voting_keypair.as_ref();
                        let vote =
                            VoteTransaction::new_vote(keypair, slot_height, bank.last_id(), 0);
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
    pub fn new<T>(
        my_id: Pubkey,
        voting_keypair: Option<Arc<T>>,
        blocktree: Arc<Blocktree>,
        bank_forks: &Arc<RwLock<BankForks>>,
        bank_forks_info: &[BankForksInfo],
        cluster_info: Arc<RwLock<ClusterInfo>>,
        exit: Arc<AtomicBool>,
        to_leader_sender: &TvuRotationSender,
        ledger_signal_receiver: Receiver<bool>,
        leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
        subscriptions: &Arc<RpcSubscriptions>,
    ) -> (Self, EntryReceiver)
    where
        T: 'static + KeypairUtil + Send + Sync,
    {
        let (ledger_entry_sender, ledger_entry_receiver) = channel();
        let exit_ = exit.clone();
        let leader_scheduler_ = leader_scheduler.clone();
        let to_leader_sender = to_leader_sender.clone();
        let subscriptions_ = subscriptions.clone();

        let (bank, last_entry_id) = {
            let mut bank_forks = bank_forks.write().unwrap();
            bank_forks.set_working_bank_id(bank_forks_info[0].bank_id);
            (bank_forks.working_bank(), bank_forks_info[0].last_entry_id)
        };
        let last_entry_id = Arc::new(RwLock::new(last_entry_id));

        let mut current_blob_index = {
            let leader_scheduler = leader_scheduler.read().unwrap();
            let slot = leader_scheduler.tick_height_to_slot(bank.tick_height() + 1);

            let leader_id = leader_scheduler
                .get_leader_for_slot(slot)
                .expect("Leader not known after processing bank");
            trace!("node {:?} scheduled as leader for slot {}", leader_id, slot,);

            // Send a rotation notification back to Fullnode to initialize the TPU to the right
            // state
            to_leader_sender
                .send(TvuRotationInfo {
                    bank: Bank::new_from_parent(&bank, &leader_id),
                    last_entry_id: *last_entry_id.read().unwrap(),
                    slot,
                    leader_id,
                })
                .unwrap();

            blocktree
                .meta(slot)
                .expect("Database error")
                .map(|meta| meta.consumed)
                .unwrap_or(0)
        };

        let t_replay = Builder::new()
            .name("solana-replay-stage".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_.clone());
                let mut last_leader_id = leader_scheduler_
                    .read()
                    .unwrap()
                    .get_leader_for_tick(bank.tick_height() + 1)
                    .unwrap();
                let mut prev_slot = None;
                let (mut current_slot, mut max_tick_height_for_slot) = {
                    let tick_height = bank.tick_height();
                    let leader_scheduler = leader_scheduler_.read().unwrap();
                    let current_slot = leader_scheduler.tick_height_to_slot(tick_height + 1);
                    let first_tick_in_current_slot = current_slot * bank.ticks_per_slot();
                    (
                        Some(current_slot),
                        first_tick_in_current_slot
                            + leader_scheduler.num_ticks_left_in_slot(first_tick_in_current_slot),
                    )
                };

                // Loop through blocktree MAX_ENTRY_RECV_PER_ITER entries at a time for each
                // relevant slot to see if there are any available updates
                loop {
                    // Stop getting entries if we get exit signal
                    if exit_.load(Ordering::Relaxed) {
                        break;
                    }
                    let timer = Duration::from_millis(100);
                    let e = ledger_signal_receiver.recv_timeout(timer);
                    match e {
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                        Ok(_) => (),
                    };

                    if current_slot.is_none() {
                        let new_slot = Self::get_next_slot(
                            &blocktree,
                            prev_slot.expect("prev_slot must exist"),
                        );
                        if new_slot.is_some() {
                            // Reset the state
                            current_slot = new_slot;
                            current_blob_index = 0;
                            let leader_scheduler = leader_scheduler_.read().unwrap();
                            let first_tick_in_current_slot =
                                current_slot.unwrap() * bank.ticks_per_slot();
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

                    // Fetch the next entries from the database
                    if !entries.is_empty() {
                        if let Err(e) = Self::process_entries(
                            entries,
                            &bank,
                            &cluster_info,
                            &voting_keypair,
                            &ledger_entry_sender,
                            &mut current_blob_index,
                            &last_entry_id,
                            &leader_scheduler_,
                            &subscriptions_,
                        ) {
                            error!("process_entries failed: {:?}", e);
                        }

                        let current_tick_height = bank.tick_height();

                        // We've reached the end of a slot, reset our state and check
                        // for leader rotation
                        if max_tick_height_for_slot == current_tick_height {
                            // Check for leader rotation
                            let (leader_id, next_slot) = {
                                let leader_scheduler = leader_scheduler_.read().unwrap();
                                (
                                    leader_scheduler
                                        .get_leader_for_tick(current_tick_height + 1)
                                        .unwrap(),
                                    leader_scheduler.tick_height_to_slot(current_tick_height + 1),
                                )
                            };

                            if my_id == leader_id || my_id == last_leader_id {
                                to_leader_sender
                                    .send(TvuRotationInfo {
                                        bank: Bank::new_from_parent(&bank, &leader_id),
                                        last_entry_id: *last_entry_id.read().unwrap(),
                                        slot: next_slot,
                                        leader_id,
                                    })
                                    .unwrap();
                            } else if leader_id != last_leader_id {
                                // TODO: Remove this soon once we boot the leader from ClusterInfo
                                cluster_info.write().unwrap().set_leader(leader_id);
                            }

                            // Check for any slots that chain to this one
                            prev_slot = current_slot;
                            current_slot = None;
                            last_leader_id = leader_id;
                            continue;
                        }
                    }
                }
            })
            .unwrap();

        (Self { t_replay, exit }, ledger_entry_receiver)
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
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
    use crate::blocktree::{create_tmp_sample_blocktree, Blocktree, DEFAULT_SLOT_HEIGHT};
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::entry::create_ticks;
    use crate::entry::{next_entry_mut, Entry};
    use crate::fullnode::new_banks_from_blocktree;
    use crate::leader_scheduler::{
        make_active_set_entries, LeaderScheduler, LeaderSchedulerConfig,
    };
    use crate::replay_stage::ReplayStage;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::timing::DEFAULT_TICKS_PER_SLOT;
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
        let leader_scheduler =
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config)));

        let (mut genesis_block, mint_keypair) =
            GenesisBlock::new_with_leader(10_000, old_leader_id, 500);
        genesis_block.ticks_per_slot = ticks_per_slot;

        // Create a ledger
        let (my_ledger_path, mut tick_height, entry_height, mut last_id, last_entry_id) =
            create_tmp_sample_blocktree(
                "test_replay_stage_leader_rotation_exit",
                &genesis_block,
                0,
            );

        info!("my_id: {:?}", my_id);
        info!("old_leader_id: {:?}", old_leader_id);

        let my_keypair = Arc::new(my_keypair);
        let num_ending_ticks = 0;
        let (active_set_entries, voting_keypair) = make_active_set_entries(
            &my_keypair,
            &mint_keypair,
            100,
            1, // add a vote for tick_height = ticks_per_slot
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
            let (bank_forks, bank_forks_info, blocktree, ledger_signal_receiver) =
                new_banks_from_blocktree(&my_ledger_path, ticks_per_slot, &leader_scheduler);

            // Set up the replay stage
            let (rotation_sender, rotation_receiver) = channel();
            let exit = Arc::new(AtomicBool::new(false));
            let blocktree = Arc::new(blocktree);
            let (replay_stage, ledger_writer_recv) = ReplayStage::new(
                my_id,
                Some(Arc::new(voting_keypair)),
                blocktree.clone(),
                &Arc::new(RwLock::new(bank_forks)),
                &bank_forks_info,
                Arc::new(RwLock::new(cluster_info_me)),
                exit.clone(),
                &rotation_sender,
                ledger_signal_receiver,
                &leader_scheduler,
                &Arc::new(RpcSubscriptions::default()),
            );

            let total_entries_to_send = 2 * ticks_per_slot as usize - 2;
            let mut entries_to_send = vec![];
            while entries_to_send.len() < total_entries_to_send {
                let entry = next_entry_mut(&mut last_id, 1, vec![]);
                entries_to_send.push(entry);
            }

            // Write the entries to the ledger, replay_stage should get notified of changes
            let meta = blocktree.meta(0).unwrap().unwrap();
            blocktree
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    tick_height,
                    meta.consumed,
                    &entries_to_send,
                )
                .unwrap();

            info!("Wait for replay_stage to exit and check return value is correct");
            let rotation_info = rotation_receiver
                .recv()
                .expect("should have signaled leader rotation");
            assert_eq!(
                rotation_info.last_entry_id,
                bank_forks_info[0].last_entry_id
            );
            assert_eq!(rotation_info.slot, 2);
            assert_eq!(rotation_info.leader_id, my_keypair.pubkey());

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

        let (genesis_block, _mint_keypair) = GenesisBlock::new_with_leader(10_000, leader_id, 500);

        let (my_ledger_path, tick_height, _last_entry_height, _last_id, _last_entry_id) =
            create_tmp_sample_blocktree(
                "test_vote_error_replay_stage_correctness",
                &genesis_block,
                1,
            );

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));

        // Set up the replay stage
        let exit = Arc::new(AtomicBool::new(false));
        let voting_keypair = Arc::new(Keypair::new());
        let (to_leader_sender, _to_leader_receiver) = channel();
        {
            let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::default()));
            let (bank_forks, bank_forks_info, blocktree, l_receiver) = new_banks_from_blocktree(
                &my_ledger_path,
                DEFAULT_TICKS_PER_SLOT,
                &leader_scheduler,
            );
            let bank = bank_forks.working_bank();
            let entry_height = bank_forks_info[0].entry_height;
            let last_entry_id = bank_forks_info[0].last_entry_id;

            let blocktree = Arc::new(blocktree);
            let (replay_stage, ledger_writer_recv) = ReplayStage::new(
                my_keypair.pubkey(),
                Some(voting_keypair.clone()),
                blocktree.clone(),
                &Arc::new(RwLock::new(bank_forks)),
                &bank_forks_info,
                cluster_info_me.clone(),
                exit.clone(),
                &to_leader_sender,
                l_receiver,
                &leader_scheduler,
                &Arc::new(RpcSubscriptions::default()),
            );

            let keypair = voting_keypair.as_ref();
            let vote = VoteTransaction::new_vote(keypair, 0, bank.last_id(), 0);
            cluster_info_me.write().unwrap().push_vote(vote);

            info!("Send ReplayStage an entry, should see it on the ledger writer receiver");
            let next_tick = create_ticks(1, last_entry_id);
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

        let (mut genesis_block, mint_keypair) =
            GenesisBlock::new_with_leader(10_000, leader_id, 500);
        genesis_block.ticks_per_slot = ticks_per_slot;

        // Create the ledger
        let (my_ledger_path, tick_height, genesis_entry_height, last_id, last_entry_id) =
            create_tmp_sample_blocktree(
                "test_vote_error_replay_stage_leader_rotation",
                &genesis_block,
                1,
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
            let blocktree = Blocktree::open_config(&my_ledger_path, ticks_per_slot).unwrap();
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
        let leader_scheduler =
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config)));

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));

        // Set up the replay stage
        let (rotation_sender, rotation_receiver) = channel();
        let exit = Arc::new(AtomicBool::new(false));
        {
            let (bank_forks, bank_forks_info, blocktree, l_receiver) =
                new_banks_from_blocktree(&my_ledger_path, ticks_per_slot, &leader_scheduler);
            let bank = bank_forks.working_bank();
            let meta = blocktree
                .meta(0)
                .unwrap()
                .expect("First slot metadata must exist");

            let voting_keypair = Arc::new(voting_keypair);
            let blocktree = Arc::new(blocktree);
            let (replay_stage, ledger_writer_recv) = ReplayStage::new(
                my_keypair.pubkey(),
                Some(voting_keypair.clone()),
                blocktree.clone(),
                &Arc::new(RwLock::new(bank_forks)),
                &bank_forks_info,
                cluster_info_me.clone(),
                exit.clone(),
                &rotation_sender,
                l_receiver,
                &leader_scheduler,
                &Arc::new(RpcSubscriptions::default()),
            );

            let keypair = voting_keypair.as_ref();
            let vote = VoteTransaction::new_vote(keypair, 0, bank.last_id(), 0);
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
            let rotation_info = rotation_receiver
                .recv()
                .expect("should have signaled leader rotation");
            assert_eq!(
                rotation_info.last_entry_id,
                bank_forks_info[0].last_entry_id
            );
            assert_eq!(rotation_info.slot, 1);
            assert_eq!(rotation_info.leader_id, my_keypair.pubkey());

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
            let entry = next_entry_mut(&mut last_id, 1, vec![]); //just ticks
            entries.push(entry);
        }

        let genesis_block = GenesisBlock::new(10_000).0;
        let bank = Arc::new(Bank::new(&genesis_block));
        let leader_scheduler_config = LeaderSchedulerConfig::default();
        let leader_scheduler = LeaderScheduler::new_with_bank(&leader_scheduler_config, &bank);
        let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));
        let voting_keypair = Some(Arc::new(Keypair::new()));
        let res = ReplayStage::process_entries(
            entries.clone(),
            &bank,
            &cluster_info_me,
            &voting_keypair,
            &ledger_entry_sender,
            &mut current_blob_index,
            &Arc::new(RwLock::new(last_entry_id)),
            &leader_scheduler,
            &Arc::new(RpcSubscriptions::default()),
        );

        match res {
            Ok(_) => (),
            Err(e) => assert!(false, "Entries were not sent correctly {:?}", e),
        }

        entries.clear();
        for _ in 0..5 {
            let entry = Entry::new(&mut Hash::default(), 1, vec![]); //just broken entries
            entries.push(entry);
        }

        let bank = Arc::new(Bank::new(&genesis_block));
        let leader_scheduler = LeaderScheduler::new_with_bank(&leader_scheduler_config, &bank);
        let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));
        let res = ReplayStage::process_entries(
            entries.clone(),
            &bank,
            &cluster_info_me,
            &voting_keypair,
            &ledger_entry_sender,
            &mut current_blob_index,
            &Arc::new(RwLock::new(last_entry_id)),
            &leader_scheduler,
            &Arc::new(RpcSubscriptions::default()),
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
