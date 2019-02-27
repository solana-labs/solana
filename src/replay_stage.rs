//! The `replay_stage` replays transactions broadcast by the leader.

use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::blocktree_processor::{self, BankForksInfo};
use crate::cluster_info::ClusterInfo;
use crate::entry::{Entry, EntryMeta, EntryReceiver, EntrySender, EntrySlice};
use crate::leader_scheduler::LeaderScheduler;
use crate::packet::BlobError;
use crate::result::{Error, Result};
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::tvu::{TvuRotationInfo, TvuRotationSender};
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
        last_entry_id: &mut Hash,
        subscriptions: &Arc<RpcSubscriptions>,
        slot: u64,
        parent_slot: Option<u64>,
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

        if !entries.as_slice().verify(last_entry_id) {
            inc_new_counter_info!("replicate_stage-verify-fail", entries.len());
            return Err(Error::BlobError(BlobError::VerificationFailed));
        }
        inc_new_counter_info!(
            "replicate_stage-verify-duration",
            duration_as_ms(&now.elapsed()) as usize
        );

        let num_ticks = bank.tick_height();
        let slot_height = bank.slot_height();
        let leader_id = LeaderScheduler::default().slot_leader(bank);
        let mut num_ticks_to_next_vote = LeaderScheduler::num_ticks_left_in_slot(bank, num_ticks);

        let mut entry_tick_height = num_ticks;
        let mut entries_with_meta = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            inc_new_counter_info!("replicate-stage_bank-tick", bank.tick_height() as usize);
            if entry.is_tick() {
                entry_tick_height += 1;
                if num_ticks_to_next_vote == 0 {
                    num_ticks_to_next_vote = bank.ticks_per_slot();
                }
                num_ticks_to_next_vote -= 1;
            }
            inc_new_counter_info!(
                "replicate-stage_tick-to-vote",
                num_ticks_to_next_vote as usize
            );
            entries_with_meta.push(EntryMeta {
                tick_height: entry_tick_height,
                slot,
                slot_leader: leader_id,
                num_ticks_left_in_slot: num_ticks_to_next_vote,
                parent_slot,
                entry: entry.clone(),
            });
            // If it's the last entry in the vector, i will be vec len - 1.
            // If we don't process the entry now, the for loop will exit and the entry
            // will be dropped.
            if 0 == num_ticks_to_next_vote || (i + 1) == entries.len() {
                res = blocktree_processor::process_entries(bank, &entries[0..=i]);

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
        entries_with_meta.truncate(num_entries_to_write);
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
            ledger_entry_sender.send(entries_with_meta)?;
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
        subscriptions: &Arc<RpcSubscriptions>,
    ) -> (Self, EntryReceiver)
    where
        T: 'static + KeypairUtil + Send + Sync,
    {
        let (ledger_entry_sender, ledger_entry_receiver) = channel();
        let exit_ = exit.clone();
        let to_leader_sender = to_leader_sender.clone();
        let subscriptions_ = subscriptions.clone();

        // Gather up all the metadata about the current state of the ledger
        let (mut bank, tick_height, mut last_entry_id, mut current_blob_index) = {
            let mut bank_forks = bank_forks.write().unwrap();
            bank_forks.set_working_bank_id(bank_forks_info[0].bank_id);
            let bank = bank_forks.working_bank();
            let tick_height = bank.tick_height();
            (
                bank,
                tick_height,
                bank_forks_info[0].last_entry_id,
                bank_forks_info[0].next_blob_index,
            )
        };

        // Update Tpu and other fullnode components with the current bank
        let (mut current_slot, mut current_leader_id, mut max_tick_height_for_slot) = {
            let slot = (tick_height + 1) / bank.ticks_per_slot();
            let first_tick_in_slot = slot * bank.ticks_per_slot();

            let leader_id = LeaderScheduler::default().slot_leader_at(slot, &bank);
            trace!("node {:?} scheduled as leader for slot {}", leader_id, slot,);

            let old_bank = bank.clone();
            // If the next slot is going to be a new slot and we're the leader for that slot,
            // make a new working bank, set it as the working bank.
            if tick_height + 1 == first_tick_in_slot {
                if leader_id == my_id {
                    bank = Self::create_and_set_working_bank(slot, &bank_forks, &old_bank);
                }
                current_blob_index = 0;
            }

            // Send a rotation notification back to Fullnode to initialize the TPU to the right
            // state. After this point, the bank.tick_height() is live, which it means it can
            // be updated by the TPU
            to_leader_sender
                .send(TvuRotationInfo {
                    bank: old_bank,
                    last_entry_id,
                    slot,
                    leader_id,
                })
                .unwrap();

            let max_tick_height_for_slot = first_tick_in_slot
                + LeaderScheduler::num_ticks_left_in_slot(&bank, first_tick_in_slot);

            (Some(slot), leader_id, max_tick_height_for_slot)
        };

        // Start the replay stage loop
        let bank_forks = bank_forks.clone();
        let t_replay = Builder::new()
            .name("solana-replay-stage".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_.clone());
                let mut prev_slot = None;

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
                            trace!("{} replay_stage: new_slot found: {:?}", my_id, new_slot);
                            // Reset the state
                            bank = Self::create_and_set_working_bank(
                                new_slot.unwrap(),
                                &bank_forks,
                                &bank,
                            );
                            current_slot = new_slot;
                            Self::reset_state(
                                bank.ticks_per_slot(),
                                current_slot.unwrap(),
                                &mut max_tick_height_for_slot,
                                &mut current_blob_index,
                            );
                        } else {
                            continue;
                        }
                    }

                    // current_slot must be Some(x) by this point
                    let slot = current_slot.unwrap();

                    // Fetch the next entries from the database
                    let entries = {
                        if current_leader_id != my_id {
                            info!(
                                "{} replay_stage: asking for entries from slot: {}, bi: {}",
                                my_id, slot, current_blob_index
                            );
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
                    let parent_slot = blocktree.meta(slot).unwrap().map(|meta| meta.parent_slot);

                    if !entries.is_empty() {
                        if let Err(e) = Self::process_entries(
                            entries,
                            &bank,
                            &cluster_info,
                            &voting_keypair,
                            &ledger_entry_sender,
                            &mut current_blob_index,
                            &mut last_entry_id,
                            &subscriptions_,
                            slot,
                            parent_slot,
                        ) {
                            error!("{} process_entries failed: {:?}", my_id, e);
                        }
                    }

                    let current_tick_height = bank.tick_height();

                    // We've reached the end of a slot, reset our state and check
                    // for leader rotation
                    if max_tick_height_for_slot == current_tick_height {
                        // Check for leader rotation
                        let (leader_id, next_slot) = {
                            let slot = (current_tick_height + 1) / bank.ticks_per_slot();

                            (LeaderScheduler::default().slot_leader_at(slot, &bank), slot)
                        };

                        // If we were the leader for the last slot update the last id b/c we
                        // haven't processed any of the entries for the slot for which we were
                        // the leader
                        if current_leader_id == my_id {
                            let meta = blocktree.meta(slot).unwrap().expect("meta has to exist");
                            if meta.last_index == std::u64::MAX {
                                // Ledger hasn't gotten last blob yet, break and wait
                                // for a signal
                                continue;
                            }
                            let last_entry = blocktree
                                .get_slot_entries(slot, meta.last_index, Some(1))
                                .unwrap();
                            last_entry_id = last_entry[0].id;
                        }

                        let old_bank = bank.clone();
                        prev_slot = current_slot;
                        if my_id == leader_id {
                            // Create new bank for next slot if we are the leader for that slot
                            bank = Self::create_and_set_working_bank(
                                next_slot,
                                &bank_forks,
                                &old_bank,
                            );
                            current_slot = Some(next_slot);
                            Self::reset_state(
                                bank.ticks_per_slot(),
                                next_slot,
                                &mut max_tick_height_for_slot,
                                &mut current_blob_index,
                            );
                        } else {
                            current_slot = None;
                        }

                        if leader_id != current_leader_id {
                            // TODO: Remove this soon once we boot the leader from ClusterInfo
                            cluster_info.write().unwrap().set_leader(leader_id);
                        }

                        // Always send rotation signal so that other services like
                        // RPC can be made aware of last slot's bank
                        to_leader_sender
                            .send(TvuRotationInfo {
                                bank: old_bank,
                                last_entry_id,
                                slot: next_slot,
                                leader_id,
                            })
                            .unwrap();

                        // Check for any slots that chain to this one
                        current_leader_id = leader_id;
                        continue;
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

    fn create_and_set_working_bank(
        slot: u64,
        bank_forks: &Arc<RwLock<BankForks>>,
        parent: &Arc<Bank>,
    ) -> Arc<Bank> {
        let new_bank = Bank::new_from_parent(&parent);
        new_bank.squash();
        let mut bank_forks = bank_forks.write().unwrap();
        bank_forks.insert(slot, new_bank);
        bank_forks.set_working_bank_id(slot);
        bank_forks.working_bank()
    }

    fn reset_state(
        ticks_per_slot: u64,
        slot: u64,
        max_tick_height_for_slot: &mut u64,
        current_blob_index: &mut u64,
    ) {
        *current_blob_index = 0;
        *max_tick_height_for_slot = (slot + 1) * ticks_per_slot - 1;
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
    use crate::blocktree::create_new_tmp_ledger;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::entry::create_ticks;
    use crate::entry::{next_entry_mut, Entry};
    use crate::fullnode::make_active_set_entries;
    use crate::fullnode::new_banks_from_blocktree;
    use crate::replay_stage::ReplayStage;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_vote_error_replay_stage_correctness() {
        // Set up dummy node to host a ReplayStage
        let my_keypair = Keypair::new();
        let my_id = my_keypair.pubkey();
        let my_node = Node::new_localhost_with_pubkey(my_id);

        // Create keypair for the leader
        let leader_id = Keypair::new().pubkey();

        let (genesis_block, _mint_keypair) = GenesisBlock::new_with_leader(10_000, leader_id, 500);

        let (my_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block).unwrap();

        // Set up the cluster info
        let cluster_info_me = Arc::new(RwLock::new(ClusterInfo::new(my_node.info.clone())));

        // Set up the replay stage
        let exit = Arc::new(AtomicBool::new(false));
        let voting_keypair = Arc::new(Keypair::new());
        let (to_leader_sender, _to_leader_receiver) = channel();
        {
            let (bank_forks, bank_forks_info, blocktree, l_receiver) =
                new_banks_from_blocktree(&my_ledger_path);
            let bank = bank_forks.working_bank();
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
                &Arc::new(RpcSubscriptions::default()),
            );

            let keypair = voting_keypair.as_ref();
            let vote = VoteTransaction::new_vote(keypair, 0, bank.last_id(), 0);
            cluster_info_me.write().unwrap().push_vote(vote);

            info!("Send ReplayStage an entry, should see it on the ledger writer receiver");
            let next_tick = create_ticks(1, last_entry_id);
            blocktree.write_entries(1, 0, 0, next_tick.clone()).unwrap();

            let received_tick = ledger_writer_recv
                .recv()
                .expect("Expected to receive an entry on the ledger writer receiver");

            assert_eq!(next_tick[0], received_tick[0].entry);

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
        let mut last_entry_id = Hash::default();
        let mut current_blob_index = 0;
        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        for _ in 0..5 {
            let entry = next_entry_mut(&mut last_id, 1, vec![]); //just ticks
            entries.push(entry);
        }

        let genesis_block = GenesisBlock::new(10_000).0;
        let bank = Arc::new(Bank::new(&genesis_block));
        let voting_keypair = Some(Arc::new(Keypair::new()));
        let res = ReplayStage::process_entries(
            entries.clone(),
            &bank,
            &cluster_info_me,
            &voting_keypair,
            &ledger_entry_sender,
            &mut current_blob_index,
            &mut last_entry_id,
            &Arc::new(RpcSubscriptions::default()),
            0,
            None,
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
        let res = ReplayStage::process_entries(
            entries.clone(),
            &bank,
            &cluster_info_me,
            &voting_keypair,
            &ledger_entry_sender,
            &mut current_blob_index,
            &mut last_entry_id,
            &Arc::new(RpcSubscriptions::default()),
            0,
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
}
