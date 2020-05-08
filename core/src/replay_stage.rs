//! The `replay_stage` replays transactions broadcast by the leader.

use crate::{
    broadcast_stage::RetransmitSlotsSender,
    cluster_info::ClusterInfo,
    cluster_info_vote_listener::VoteTracker,
    cluster_slots::ClusterSlots,
    commitment::{AggregateCommitmentService, BlockCommitmentCache, CommitmentAggregationData},
    consensus::{StakeLockout, Tower},
    poh_recorder::{PohRecorder, GRACE_TICKS_FACTOR, MAX_GRACE_SLOTS},
    progress_map::{ForkProgress, ForkStats, ProgressMap, PropagatedStats},
    repair_service::DuplicateSlotsResetReceiver,
    result::Result,
    rewards_recorder_service::RewardsRecorderSender,
    rpc_subscriptions::RpcSubscriptions,
};
use solana_ledger::{
    bank_forks::BankForks,
    block_error::BlockError,
    blockstore::Blockstore,
    blockstore_processor::{self, BlockstoreProcessorError, TransactionStatusSender},
    entry::VerifyRecyclers,
    leader_schedule_cache::LeaderScheduleCache,
    snapshot_package::AccountsPackageSender,
};
use solana_measure::thread_mem_usage;
use solana_metrics::inc_new_counter_info;
use solana_runtime::bank::Bank;
use solana_sdk::{
    clock::{Slot, NUM_CONSECUTIVE_LEADER_SLOTS},
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    timing::{self, duration_as_ms},
    transaction::Transaction,
};
use solana_vote_program::{
    vote_instruction,
    vote_state::{Vote, VoteState},
};
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    rc::Rc,
    result,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex, RwLock,
    },
    thread::{self, Builder, JoinHandle},
    time::{Duration, Instant},
};

pub const MAX_ENTRY_RECV_PER_ITER: usize = 512;
pub const SUPERMINORITY_THRESHOLD: f64 = 1f64 / 3f64;
pub const MAX_UNCONFIRMED_SLOTS: usize = 5;

#[derive(PartialEq, Debug)]
pub(crate) enum HeaviestForkFailures {
    LockedOut(u64),
    FailedThreshold(u64),
    FailedSwitchThreshold(u64),
    NoPropagatedConfirmation(u64),
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

#[derive(Default)]
struct SkippedSlotsInfo {
    last_retransmit_slot: u64,
    last_skipped_slot: u64,
}

pub struct ReplayStageConfig {
    pub my_pubkey: Pubkey,
    pub vote_account: Pubkey,
    pub authorized_voter_keypairs: Vec<Arc<Keypair>>,
    pub exit: Arc<AtomicBool>,
    pub subscriptions: Arc<RpcSubscriptions>,
    pub leader_schedule_cache: Arc<LeaderScheduleCache>,
    pub latest_root_senders: Vec<Sender<Slot>>,
    pub accounts_hash_sender: Option<AccountsPackageSender>,
    pub block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    pub transaction_status_sender: Option<TransactionStatusSender>,
    pub rewards_recorder_sender: Option<RewardsRecorderSender>,
}

#[derive(Default)]
pub struct ReplayTiming {
    num_iterations: u64,
    compute_bank_stats_elapsed: u64,
    select_vote_and_reset_forks_elapsed: u64,
}
impl ReplayTiming {
    fn update(
        &mut self,
        compute_bank_stats_elapsed: u64,
        select_vote_and_reset_forks_elapsed: u64,
    ) {
        self.num_iterations += 1;
        self.compute_bank_stats_elapsed += compute_bank_stats_elapsed;
        self.select_vote_and_reset_forks_elapsed += select_vote_and_reset_forks_elapsed;
        if self.num_iterations == 100 {
            datapoint_info!(
                "replay-loop-timing-stats",
                (
                    "compute_bank_stats_elapsed",
                    self.compute_bank_stats_elapsed as i64 / 100,
                    i64
                ),
                (
                    "select_vote_and_reset_forks_elapsed",
                    self.select_vote_and_reset_forks_elapsed as i64 / 100,
                    i64
                ),
            );
            self.num_iterations = 0;
            self.compute_bank_stats_elapsed = 0;
            self.select_vote_and_reset_forks_elapsed = 0;
        }
    }
}

pub struct ReplayStage {
    t_replay: JoinHandle<Result<()>>,
    commitment_service: AggregateCommitmentService,
}

impl ReplayStage {
    #[allow(clippy::new_ret_no_self, clippy::too_many_arguments)]
    pub fn new(
        config: ReplayStageConfig,
        blockstore: Arc<Blockstore>,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: Arc<ClusterInfo>,
        ledger_signal_receiver: Receiver<bool>,
        poh_recorder: Arc<Mutex<PohRecorder>>,
        vote_tracker: Arc<VoteTracker>,
        cluster_slots: Arc<ClusterSlots>,
        retransmit_slots_sender: RetransmitSlotsSender,
        duplicate_slots_reset_receiver: DuplicateSlotsResetReceiver,
    ) -> (Self, Receiver<Vec<Arc<Bank>>>) {
        let ReplayStageConfig {
            my_pubkey,
            vote_account,
            authorized_voter_keypairs,
            exit,
            subscriptions,
            leader_schedule_cache,
            latest_root_senders,
            accounts_hash_sender,
            block_commitment_cache,
            transaction_status_sender,
            rewards_recorder_sender,
        } = config;

        let (root_bank_sender, root_bank_receiver) = channel();
        trace!("replay stage");
        let mut tower = Tower::new(&my_pubkey, &vote_account, &bank_forks.read().unwrap());

        // Start the replay stage loop
        let (lockouts_sender, commitment_service) =
            AggregateCommitmentService::new(&exit, block_commitment_cache.clone());

        #[allow(clippy::cognitive_complexity)]
        let t_replay = Builder::new()
            .name("solana-replay-stage".to_string())
            .spawn(move || {
                let mut all_pubkeys: HashSet<Rc<Pubkey>> = HashSet::new();
                let verify_recyclers = VerifyRecyclers::default();
                let _exit = Finalizer::new(exit.clone());
                let mut progress = ProgressMap::default();
                let mut frozen_banks: Vec<_> = bank_forks
                    .read()
                    .unwrap()
                    .frozen_banks()
                    .values()
                    .cloned()
                    .collect();

                frozen_banks.sort_by_key(|bank| bank.slot());

                // Initialize progress map with any root banks
                for bank in frozen_banks {
                    let prev_leader_slot = progress.get_bank_prev_leader_slot(&bank);
                    progress.insert(
                        bank.slot(),
                        ForkProgress::new_from_bank(
                            &bank,
                            &my_pubkey,
                            &vote_account,
                            prev_leader_slot,
                            0,
                            0,
                        ),
                    );
                }
                let mut current_leader = None;
                let mut last_reset = Hash::default();
                let mut partition = false;
                let mut skipped_slots_info = SkippedSlotsInfo::default();
                let mut replay_timing = ReplayTiming::default();
                loop {
                    let allocated = thread_mem_usage::Allocatedp::default();

                    thread_mem_usage::datapoint("solana-replay-stage");
                    // Stop getting entries if we get exit signal
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    let start = allocated.get();
                    Self::generate_new_bank_forks(
                        &blockstore,
                        &bank_forks,
                        &leader_schedule_cache,
                        &subscriptions,
                        rewards_recorder_sender.clone(),
                        &mut progress,
                        &mut all_pubkeys,
                    );
                    Self::report_memory(&allocated, "generate_new_bank_forks", start);

                    let mut tpu_has_bank = poh_recorder.lock().unwrap().has_bank();

                    let start = allocated.get();
                    let did_complete_bank = Self::replay_active_banks(
                        &blockstore,
                        &bank_forks,
                        &my_pubkey,
                        &vote_account,
                        &mut progress,
                        transaction_status_sender.clone(),
                        &verify_recyclers,
                    );
                    Self::report_memory(&allocated, "replay_active_banks", start);

                    let mut ancestors = bank_forks.read().unwrap().ancestors();
                    let mut descendants = bank_forks.read().unwrap().descendants();
                    let forks_root = bank_forks.read().unwrap().root();
                    let start = allocated.get();

                    // Reset any duplicate slots that have been confirmed
                    // by the network in anticipation of the confirmed version of
                    // the slot
                    Self::reset_duplicate_slots(
                        &duplicate_slots_reset_receiver,
                        &mut ancestors,
                        &mut descendants,
                        &mut progress,
                        &bank_forks,
                    );
                    let mut frozen_banks: Vec<_> = bank_forks
                        .read()
                        .unwrap()
                        .frozen_banks()
                        .into_iter()
                        .filter(|(slot, _)| *slot >= forks_root)
                        .map(|(_, bank)| bank)
                        .collect();
                    let now = Instant::now();
                    let newly_computed_slot_stats = Self::compute_bank_stats(
                        &my_pubkey,
                        &ancestors,
                        &mut frozen_banks,
                        &tower,
                        &mut progress,
                        &vote_tracker,
                        &cluster_slots,
                        &bank_forks,
                        &mut all_pubkeys,
                    );
                    let compute_bank_stats_elapsed = now.elapsed().as_micros();
                    for slot in newly_computed_slot_stats {
                        let fork_stats = progress.get_fork_stats(slot).unwrap();
                        let confirmed_forks = Self::confirm_forks(
                            &tower,
                            &fork_stats.stake_lockouts,
                            fork_stats.total_staked,
                            &progress,
                            &bank_forks,
                        );

                        for slot in confirmed_forks {
                            progress
                                .get_mut(&slot)
                                .unwrap()
                                .fork_stats
                                .confirmation_reported = true;
                        }
                    }

                    let (heaviest_bank, heaviest_bank_on_same_fork) =
                        Self::select_forks(&frozen_banks, &tower, &progress, &ancestors);

                    Self::report_memory(&allocated, "select_fork", start);

                    let now = Instant::now();
                    let (vote_bank, reset_bank, failure_reasons) =
                        Self::select_vote_and_reset_forks(
                            &heaviest_bank,
                            &heaviest_bank_on_same_fork,
                            &ancestors,
                            &descendants,
                            &progress,
                            &tower,
                        );
                    let select_vote_and_reset_forks_elapsed = now.elapsed().as_micros();
                    replay_timing.update(
                        compute_bank_stats_elapsed as u64,
                        select_vote_and_reset_forks_elapsed as u64,
                    );

                    if heaviest_bank.is_some()
                        && tower.is_recent(heaviest_bank.as_ref().unwrap().slot())
                        && !failure_reasons.is_empty()
                    {
                        info!(
                            "Couldn't vote on heaviest fork: {:?}, failure_reasons: {:?}",
                            heaviest_bank.as_ref().map(|b| b.slot()),
                            failure_reasons
                        );

                        for r in failure_reasons {
                            if let HeaviestForkFailures::NoPropagatedConfirmation(slot) = r {
                                if let Some(latest_leader_slot) =
                                    progress.get_latest_leader_slot(slot)
                                {
                                    progress.log_propagated_stats(latest_leader_slot, &bank_forks);
                                }
                            }
                        }
                    }

                    let start = allocated.get();

                    // Vote on a fork
                    if let Some(ref vote_bank) = vote_bank {
                        subscriptions
                            .notify_subscribers(block_commitment_cache.read().unwrap().slot());
                        if let Some(votable_leader) =
                            leader_schedule_cache.slot_leader_at(vote_bank.slot(), Some(vote_bank))
                        {
                            Self::log_leader_change(
                                &my_pubkey,
                                vote_bank.slot(),
                                &mut current_leader,
                                &votable_leader,
                            );
                        }

                        Self::handle_votable_bank(
                            &vote_bank,
                            &bank_forks,
                            &mut tower,
                            &mut progress,
                            &vote_account,
                            &authorized_voter_keypairs,
                            &cluster_info,
                            &blockstore,
                            &leader_schedule_cache,
                            &root_bank_sender,
                            &lockouts_sender,
                            &accounts_hash_sender,
                            &latest_root_senders,
                            &mut all_pubkeys,
                            &subscriptions,
                            &block_commitment_cache,
                        )?;
                    };

                    Self::report_memory(&allocated, "votable_bank", start);
                    let start = allocated.get();

                    // Reset onto a fork
                    if let Some(reset_bank) = reset_bank {
                        if last_reset != reset_bank.last_blockhash() {
                            info!(
                                "vote bank: {:?} reset bank: {:?}",
                                vote_bank.as_ref().map(|b| b.slot()),
                                reset_bank.slot(),
                            );
                            let fork_progress = progress
                                .get(&reset_bank.slot())
                                .expect("bank to reset to must exist in progress map");
                            datapoint_info!(
                                "blocks_produced",
                                ("num_blocks_on_fork", fork_progress.num_blocks_on_fork, i64),
                                (
                                    "num_dropped_blocks_on_fork",
                                    fork_progress.num_dropped_blocks_on_fork,
                                    i64
                                ),
                            );
                            Self::reset_poh_recorder(
                                &my_pubkey,
                                &blockstore,
                                &reset_bank,
                                &poh_recorder,
                                &leader_schedule_cache,
                            );
                            last_reset = reset_bank.last_blockhash();
                            tpu_has_bank = false;

                            if !partition
                                && vote_bank.as_ref().map(|b| b.slot()) != Some(reset_bank.slot())
                            {
                                warn!(
                                    "PARTITION DETECTED waiting to join fork: {} last vote: {:?}",
                                    reset_bank.slot(),
                                    tower.last_vote()
                                );
                                inc_new_counter_info!("replay_stage-partition_detected", 1);
                                datapoint_info!(
                                    "replay_stage-partition",
                                    ("slot", reset_bank.slot() as i64, i64)
                                );
                                partition = true;
                            } else if partition
                                && vote_bank.as_ref().map(|b| b.slot()) == Some(reset_bank.slot())
                            {
                                warn!(
                                    "PARTITION resolved fork: {} last vote: {:?}",
                                    reset_bank.slot(),
                                    tower.last_vote()
                                );
                                partition = false;
                                inc_new_counter_info!("replay_stage-partition_resolved", 1);
                            }
                        }
                        datapoint_debug!(
                            "replay_stage-memory",
                            ("reset_bank", (allocated.get() - start) as i64, i64),
                        );
                    }
                    Self::report_memory(&allocated, "reset_bank", start);

                    let start = allocated.get();
                    if !tpu_has_bank {
                        Self::maybe_start_leader(
                            &my_pubkey,
                            &bank_forks,
                            &poh_recorder,
                            &leader_schedule_cache,
                            &subscriptions,
                            rewards_recorder_sender.clone(),
                            &progress,
                            &retransmit_slots_sender,
                            &mut skipped_slots_info,
                        );

                        let poh_bank = poh_recorder.lock().unwrap().bank();
                        if let Some(bank) = poh_bank {
                            Self::log_leader_change(
                                &my_pubkey,
                                bank.slot(),
                                &mut current_leader,
                                &my_pubkey,
                            );
                        }
                    }
                    Self::report_memory(&allocated, "start_leader", start);
                    datapoint_debug!(
                        "replay_stage",
                        ("duration", duration_as_ms(&now.elapsed()) as i64, i64)
                    );
                    if did_complete_bank {
                        //just processed a bank, skip the signal; maybe there's more slots available
                        continue;
                    }
                    let timer = Duration::from_millis(100);
                    let result = ledger_signal_receiver.recv_timeout(timer);
                    match result {
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                        Ok(_) => trace!("blockstore signal"),
                    };
                }
                Ok(())
            })
            .unwrap();
        (
            Self {
                t_replay,
                commitment_service,
            },
            root_bank_receiver,
        )
    }

    fn report_memory(
        allocated: &solana_measure::thread_mem_usage::Allocatedp,
        name: &'static str,
        start: u64,
    ) {
        datapoint_debug!(
            "replay_stage-memory",
            (name, (allocated.get() - start) as i64, i64),
        );
    }

    fn reset_duplicate_slots(
        duplicate_slots_reset_receiver: &DuplicateSlotsResetReceiver,
        ancestors: &mut HashMap<Slot, HashSet<Slot>>,
        descendants: &mut HashMap<Slot, HashSet<Slot>>,
        progress: &mut ProgressMap,
        bank_forks: &RwLock<BankForks>,
    ) {
        for duplicate_slot in duplicate_slots_reset_receiver.try_iter() {
            Self::purge_unconfirmed_duplicate_slot(
                duplicate_slot,
                ancestors,
                descendants,
                progress,
                bank_forks,
            );
        }
    }

    fn purge_unconfirmed_duplicate_slot(
        duplicate_slot: Slot,
        ancestors: &mut HashMap<Slot, HashSet<Slot>>,
        descendants: &mut HashMap<Slot, HashSet<Slot>>,
        progress: &mut ProgressMap,
        bank_forks: &RwLock<BankForks>,
    ) {
        warn!("purging slot {}", duplicate_slot);
        let slot_descendants = descendants.get(&duplicate_slot).cloned();
        if slot_descendants.is_none() {
            // Root has already moved past this slot, no need to purge it
            return;
        }

        // Clear the ancestors/descendants map to keep them
        // consistent
        let slot_descendants = slot_descendants.unwrap();
        Self::purge_ancestors_descendants(
            duplicate_slot,
            &slot_descendants,
            ancestors,
            descendants,
        );

        for d in slot_descendants
            .iter()
            .chain(std::iter::once(&duplicate_slot))
        {
            // Clear the progress map of these forks
            let _ = progress.remove(d);

            // Clear the duplicate banks from BankForks
            {
                let mut w_bank_forks = bank_forks.write().unwrap();
                // Purging should have already been taken care of by logic
                // in repair_service, so make sure drop implementation doesn't
                // run
                if let Some(b) = w_bank_forks.get(*d) {
                    b.skip_drop.store(true, Ordering::Relaxed)
                }
                w_bank_forks.remove(*d);
            }
        }
    }

    // Purge given slot and all its descendants from the `ancestors` and
    // `descendants` structures so that they're consistent with `BankForks`
    // and the `progress` map.
    fn purge_ancestors_descendants(
        slot: Slot,
        slot_descendants: &HashSet<Slot>,
        ancestors: &mut HashMap<Slot, HashSet<Slot>>,
        descendants: &mut HashMap<Slot, HashSet<Slot>>,
    ) {
        if !ancestors.contains_key(&slot) {
            // Slot has already been purged
            return;
        }

        // Purge this slot from each of its ancestors' `descendants` maps
        for a in ancestors
            .get(&slot)
            .expect("must exist based on earlier check")
        {
            descendants
                .get_mut(&a)
                .expect("If exists in ancestor map must exist in descendants map")
                .retain(|d| *d != slot && !slot_descendants.contains(d));
        }
        ancestors
            .remove(&slot)
            .expect("must exist based on earlier check");

        // Purge all the descendants of this slot from both maps
        for descendant in slot_descendants {
            ancestors.remove(&descendant).expect("must exist");
            descendants
                .remove(&descendant)
                .expect("must exist based on earlier check");
        }
        descendants
            .remove(&slot)
            .expect("must exist based on earlier check");
    }

    fn log_leader_change(
        my_pubkey: &Pubkey,
        bank_slot: Slot,
        current_leader: &mut Option<Pubkey>,
        new_leader: &Pubkey,
    ) {
        if let Some(ref current_leader) = current_leader {
            if current_leader != new_leader {
                let msg = if current_leader == my_pubkey {
                    ". I am no longer the leader"
                } else if new_leader == my_pubkey {
                    ". I am now the leader"
                } else {
                    ""
                };
                info!(
                    "LEADER CHANGE at slot: {} leader: {}{}",
                    bank_slot, new_leader, msg
                );
            }
        }
        current_leader.replace(new_leader.to_owned());
    }

    fn check_propagation_for_start_leader(
        poh_slot: Slot,
        parent_slot: Slot,
        progress_map: &ProgressMap,
    ) -> bool {
        // Check if the next leader slot is part of a consecutive block, in
        // which case ignore the propagation check
        let is_consecutive_leader = progress_map
            .get_propagated_stats(parent_slot)
            .unwrap()
            .is_leader_slot
            && parent_slot == poh_slot - 1;

        if is_consecutive_leader {
            return true;
        }

        progress_map.is_propagated(parent_slot)
    }

    fn should_retransmit(poh_slot: Slot, last_retransmit_slot: &mut Slot) -> bool {
        if poh_slot < *last_retransmit_slot
            || poh_slot >= *last_retransmit_slot + NUM_CONSECUTIVE_LEADER_SLOTS
        {
            *last_retransmit_slot = poh_slot;
            true
        } else {
            false
        }
    }

    fn maybe_start_leader(
        my_pubkey: &Pubkey,
        bank_forks: &Arc<RwLock<BankForks>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        subscriptions: &Arc<RpcSubscriptions>,
        rewards_recorder_sender: Option<RewardsRecorderSender>,
        progress_map: &ProgressMap,
        retransmit_slots_sender: &RetransmitSlotsSender,
        skipped_slots_info: &mut SkippedSlotsInfo,
    ) {
        // all the individual calls to poh_recorder.lock() are designed to
        // increase granularity, decrease contention

        assert!(!poh_recorder.lock().unwrap().has_bank());

        let (reached_leader_slot, _grace_ticks, poh_slot, parent_slot) =
            poh_recorder.lock().unwrap().reached_leader_slot();

        if !reached_leader_slot {
            trace!("{} poh_recorder hasn't reached_leader_slot", my_pubkey);
            return;
        }
        trace!("{} reached_leader_slot", my_pubkey);

        let parent = bank_forks
            .read()
            .unwrap()
            .get(parent_slot)
            .expect("parent_slot doesn't exist in bank forks")
            .clone();

        assert!(parent.is_frozen());

        if bank_forks.read().unwrap().get(poh_slot).is_some() {
            warn!("{} already have bank in forks at {}?", my_pubkey, poh_slot);
            return;
        }
        trace!(
            "{} poh_slot {} parent_slot {}",
            my_pubkey,
            poh_slot,
            parent_slot
        );

        if let Some(next_leader) = leader_schedule_cache.slot_leader_at(poh_slot, Some(&parent)) {
            trace!(
                "{} leader {} at poh slot: {}",
                my_pubkey,
                next_leader,
                poh_slot
            );

            // I guess I missed my slot
            if next_leader != *my_pubkey {
                return;
            }

            datapoint_info!(
                "replay_stage-new_leader",
                ("slot", poh_slot, i64),
                ("leader", next_leader.to_string(), String),
            );

            if !Self::check_propagation_for_start_leader(poh_slot, parent_slot, progress_map) {
                let latest_unconfirmed_leader_slot = progress_map.get_latest_leader_slot(parent_slot).expect("In order for propagated check to fail, latest leader must exist in progress map");
                if poh_slot != skipped_slots_info.last_skipped_slot {
                    datapoint_info!(
                        "replay_stage-skip_leader_slot",
                        ("slot", poh_slot, i64),
                        ("parent_slot", parent_slot, i64),
                        (
                            "latest_unconfirmed_leader_slot",
                            latest_unconfirmed_leader_slot,
                            i64
                        )
                    );
                    progress_map.log_propagated_stats(latest_unconfirmed_leader_slot, bank_forks);
                    skipped_slots_info.last_skipped_slot = poh_slot;
                }
                let bank = bank_forks.read().unwrap().get(latest_unconfirmed_leader_slot)
                .expect("In order for propagated check to fail, latest leader must exist in progress map, and thus also in BankForks").clone();

                // Signal retransmit
                if Self::should_retransmit(poh_slot, &mut skipped_slots_info.last_retransmit_slot) {
                    datapoint_info!("replay_stage-retransmit", ("slot", bank.slot(), i64),);
                    retransmit_slots_sender
                        .send(vec![(bank.slot(), bank.clone())].into_iter().collect())
                        .unwrap();
                }
                return;
            }

            let root_slot = bank_forks.read().unwrap().root();
            datapoint_info!("replay_stage-my_leader_slot", ("slot", poh_slot, i64),);
            info!(
                "new fork:{} parent:{} (leader) root:{}",
                poh_slot, parent_slot, root_slot
            );

            let tpu_bank = Self::new_bank_from_parent_with_notify(
                &parent,
                poh_slot,
                root_slot,
                my_pubkey,
                &rewards_recorder_sender,
                subscriptions,
            );

            let tpu_bank = bank_forks.write().unwrap().insert(tpu_bank);
            poh_recorder.lock().unwrap().set_bank(&tpu_bank);
        } else {
            error!("{} No next leader found", my_pubkey);
        }
    }

    fn replay_blockstore_into_bank(
        bank: &Arc<Bank>,
        blockstore: &Blockstore,
        bank_progress: &mut ForkProgress,
        transaction_status_sender: Option<TransactionStatusSender>,
        verify_recyclers: &VerifyRecyclers,
    ) -> result::Result<usize, BlockstoreProcessorError> {
        let tx_count_before = bank_progress.replay_progress.num_txs;
        let confirm_result = blockstore_processor::confirm_slot(
            blockstore,
            bank,
            &mut bank_progress.replay_stats,
            &mut bank_progress.replay_progress,
            false,
            transaction_status_sender,
            None,
            verify_recyclers,
        );
        let tx_count_after = bank_progress.replay_progress.num_txs;
        let tx_count = tx_count_after - tx_count_before;

        confirm_result.map_err(|err| {
            // LedgerCleanupService should not be cleaning up anything
            // that comes after the root, so we should not see any
            // errors related to the slot being purged
            let slot = bank.slot();
            warn!("Fatal replay error in slot: {}, err: {:?}", slot, err);
            if let BlockstoreProcessorError::InvalidBlock(BlockError::InvalidTickCount) = err {
                datapoint_info!(
                    "replay-stage-mark_dead_slot",
                    ("error", format!("error: {:?}", err), String),
                    ("slot", slot, i64)
                );
            } else {
                datapoint_error!(
                    "replay-stage-mark_dead_slot",
                    ("error", format!("error: {:?}", err), String),
                    ("slot", slot, i64)
                );
            }
            bank_progress.is_dead = true;
            blockstore
                .set_dead_slot(slot)
                .expect("Failed to mark slot as dead in blockstore");
            err
        })?;

        Ok(tx_count)
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_votable_bank(
        bank: &Arc<Bank>,
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &mut Tower,
        progress: &mut ProgressMap,
        vote_account_pubkey: &Pubkey,
        authorized_voter_keypairs: &[Arc<Keypair>],
        cluster_info: &Arc<ClusterInfo>,
        blockstore: &Arc<Blockstore>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        root_bank_sender: &Sender<Vec<Arc<Bank>>>,
        lockouts_sender: &Sender<CommitmentAggregationData>,
        accounts_hash_sender: &Option<AccountsPackageSender>,
        latest_root_senders: &[Sender<Slot>],
        all_pubkeys: &mut HashSet<Rc<Pubkey>>,
        subscriptions: &Arc<RpcSubscriptions>,
        block_commitment_cache: &Arc<RwLock<BlockCommitmentCache>>,
    ) -> Result<()> {
        if bank.is_empty() {
            inc_new_counter_info!("replay_stage-voted_empty_bank", 1);
        }
        trace!("handle votable bank {}", bank.slot());
        let (vote, tower_index) = tower.new_vote_from_bank(bank, vote_account_pubkey);
        if let Some(new_root) = tower.record_bank_vote(vote) {
            // get the root bank before squash
            let root_bank = bank_forks
                .read()
                .unwrap()
                .get(new_root)
                .expect("Root bank doesn't exist")
                .clone();
            let mut rooted_banks = root_bank.parents();
            rooted_banks.push(root_bank);
            let rooted_slots: Vec<_> = rooted_banks.iter().map(|bank| bank.slot()).collect();
            // Call leader schedule_cache.set_root() before blockstore.set_root() because
            // bank_forks.root is consumed by repair_service to update gossip, so we don't want to
            // get shreds for repair on gossip before we update leader schedule, otherwise they may
            // get dropped.
            leader_schedule_cache.set_root(rooted_banks.last().unwrap());
            blockstore
                .set_roots(&rooted_slots)
                .expect("Ledger set roots failed");
            let largest_confirmed_root = Some(
                block_commitment_cache
                    .read()
                    .unwrap()
                    .largest_confirmed_root(),
            );
            Self::handle_new_root(
                new_root,
                &bank_forks,
                progress,
                accounts_hash_sender,
                all_pubkeys,
                largest_confirmed_root,
            );
            subscriptions.notify_roots(rooted_slots);
            latest_root_senders.iter().for_each(|s| {
                if let Err(e) = s.send(new_root) {
                    trace!("latest root send failed: {:?}", e);
                }
            });
            info!("new root {}", new_root);
            if let Err(e) = root_bank_sender.send(rooted_banks) {
                trace!("root_bank_sender failed: {:?}", e);
                return Err(e.into());
            }
        }

        Self::update_commitment_cache(
            bank.clone(),
            bank_forks.read().unwrap().root(),
            progress.get_fork_stats(bank.slot()).unwrap().total_staked,
            lockouts_sender,
        );

        Self::push_vote(
            cluster_info,
            bank,
            vote_account_pubkey,
            authorized_voter_keypairs,
            tower.last_vote_and_timestamp(),
            tower_index,
        );
        Ok(())
    }

    fn push_vote(
        cluster_info: &ClusterInfo,
        bank: &Arc<Bank>,
        vote_account_pubkey: &Pubkey,
        authorized_voter_keypairs: &[Arc<Keypair>],
        vote: Vote,
        tower_index: usize,
    ) {
        if authorized_voter_keypairs.is_empty() {
            return;
        }

        let vote_state =
            if let Some((_, vote_account)) = bank.vote_accounts().get(vote_account_pubkey) {
                if let Some(vote_state) = VoteState::from(&vote_account) {
                    vote_state
                } else {
                    warn!(
                        "Vote account {} is unreadable.  Unable to vote",
                        vote_account_pubkey,
                    );
                    return;
                }
            } else {
                warn!(
                    "Vote account {} does not exist.  Unable to vote",
                    vote_account_pubkey,
                );
                return;
            };

        let authorized_voter_pubkey =
            if let Some(authorized_voter_pubkey) = vote_state.get_authorized_voter(bank.epoch()) {
                authorized_voter_pubkey
            } else {
                warn!(
                    "Vote account {} has no authorized voter for epoch {}.  Unable to vote",
                    vote_account_pubkey,
                    bank.epoch()
                );
                return;
            };

        let authorized_voter_keypair = match authorized_voter_keypairs
            .iter()
            .find(|keypair| keypair.pubkey() == authorized_voter_pubkey)
        {
            None => {
                warn!("The authorized keypair {} for vote account {} is not available.  Unable to vote",
                      authorized_voter_pubkey, vote_account_pubkey);
                return;
            }
            Some(authorized_voter_keypair) => authorized_voter_keypair,
        };
        let node_keypair = cluster_info.keypair.clone();

        // Send our last few votes along with the new one
        let vote_ix = vote_instruction::vote(
            &vote_account_pubkey,
            &authorized_voter_keypair.pubkey(),
            vote,
        );

        let mut vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&node_keypair.pubkey()));

        let blockhash = bank.last_blockhash();
        vote_tx.partial_sign(&[node_keypair.as_ref()], blockhash);
        vote_tx.partial_sign(&[authorized_voter_keypair.as_ref()], blockhash);
        cluster_info.push_vote(tower_index, vote_tx);
    }

    fn update_commitment_cache(
        bank: Arc<Bank>,
        root: Slot,
        total_staked: u64,
        lockouts_sender: &Sender<CommitmentAggregationData>,
    ) {
        if let Err(e) =
            lockouts_sender.send(CommitmentAggregationData::new(bank, root, total_staked))
        {
            trace!("lockouts_sender failed: {:?}", e);
        }
    }

    fn reset_poh_recorder(
        my_pubkey: &Pubkey,
        blockstore: &Blockstore,
        bank: &Arc<Bank>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
    ) {
        let next_leader_slot = leader_schedule_cache.next_leader_slot(
            &my_pubkey,
            bank.slot(),
            &bank,
            Some(blockstore),
            GRACE_TICKS_FACTOR * MAX_GRACE_SLOTS,
        );
        poh_recorder
            .lock()
            .unwrap()
            .reset(bank.last_blockhash(), bank.slot(), next_leader_slot);

        let next_leader_msg = if let Some(next_leader_slot) = next_leader_slot {
            format!("My next leader slot is {}", next_leader_slot.0)
        } else {
            "I am not in the leader schedule yet".to_owned()
        };

        info!(
            "{} reset PoH to tick {} (within slot {}). {}",
            my_pubkey,
            bank.tick_height(),
            bank.slot(),
            next_leader_msg,
        );
    }

    fn replay_active_banks(
        blockstore: &Arc<Blockstore>,
        bank_forks: &Arc<RwLock<BankForks>>,
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
        progress: &mut ProgressMap,
        transaction_status_sender: Option<TransactionStatusSender>,
        verify_recyclers: &VerifyRecyclers,
    ) -> bool {
        let mut did_complete_bank = false;
        let mut tx_count = 0;
        let active_banks = bank_forks.read().unwrap().active_banks();
        trace!("active banks {:?}", active_banks);

        for bank_slot in &active_banks {
            // If the fork was marked as dead, don't replay it
            if progress.get(bank_slot).map(|p| p.is_dead).unwrap_or(false) {
                debug!("bank_slot {:?} is marked dead", *bank_slot);
                continue;
            }

            let bank = bank_forks.read().unwrap().get(*bank_slot).unwrap().clone();
            let parent_slot = bank.parent_slot();
            let prev_leader_slot = progress.get_bank_prev_leader_slot(&bank);
            let (num_blocks_on_fork, num_dropped_blocks_on_fork) = {
                let stats = progress
                    .get(&parent_slot)
                    .expect("parent of active bank must exist in progress map");
                let num_blocks_on_fork = stats.num_blocks_on_fork + 1;
                let new_dropped_blocks = bank.slot() - parent_slot - 1;
                let num_dropped_blocks_on_fork =
                    stats.num_dropped_blocks_on_fork + new_dropped_blocks;
                (num_blocks_on_fork, num_dropped_blocks_on_fork)
            };
            // Insert a progress entry even for slots this node is the leader for, so that
            // 1) confirm_forks can report confirmation, 2) we can cache computations about
            // this bank in `select_forks()`
            let bank_progress = &mut progress.entry(bank.slot()).or_insert_with(|| {
                ForkProgress::new_from_bank(
                    &bank,
                    &my_pubkey,
                    vote_account,
                    prev_leader_slot,
                    num_blocks_on_fork,
                    num_dropped_blocks_on_fork,
                )
            });
            if bank.collector_id() != my_pubkey {
                let replay_result = Self::replay_blockstore_into_bank(
                    &bank,
                    &blockstore,
                    bank_progress,
                    transaction_status_sender.clone(),
                    verify_recyclers,
                );
                match replay_result {
                    Ok(replay_tx_count) => tx_count += replay_tx_count,
                    Err(err) => {
                        trace!("replay_result err: {:?}, slot {}", err, bank_slot);
                        // If the bank was corrupted, don't try to run the below logic to check if the
                        // bank is completed
                        continue;
                    }
                }
            }
            assert_eq!(*bank_slot, bank.slot());
            if bank.is_complete() {
                bank_progress.replay_stats.report_stats(
                    bank.slot(),
                    bank_progress.replay_progress.num_entries,
                    bank_progress.replay_progress.num_shreds,
                );
                did_complete_bank = true;
                info!("bank frozen: {}", bank.slot());
                bank.freeze();
            } else {
                trace!(
                    "bank {} not completed tick_height: {}, max_tick_height: {}",
                    bank.slot(),
                    bank.tick_height(),
                    bank.max_tick_height()
                );
            }
        }
        inc_new_counter_info!("replay_stage-replay_transactions", tx_count);
        did_complete_bank
    }

    pub(crate) fn compute_bank_stats(
        my_pubkey: &Pubkey,
        ancestors: &HashMap<u64, HashSet<u64>>,
        frozen_banks: &mut Vec<Arc<Bank>>,
        tower: &Tower,
        progress: &mut ProgressMap,
        vote_tracker: &VoteTracker,
        cluster_slots: &ClusterSlots,
        bank_forks: &RwLock<BankForks>,
        all_pubkeys: &mut HashSet<Rc<Pubkey>>,
    ) -> Vec<Slot> {
        frozen_banks.sort_by_key(|bank| bank.slot());
        let mut new_stats = vec![];
        for bank in frozen_banks {
            let bank_slot = bank.slot();

            // Only time progress map should be missing a bank slot
            // is if this node was the leader for this slot as those banks
            // are not replayed in replay_active_banks()
            let parent_weight = bank
                .parent()
                .and_then(|b| progress.get(&b.slot()))
                .map(|x| x.fork_stats.fork_weight)
                .unwrap_or(0);
            {
                let stats = progress
                    .get_fork_stats_mut(bank_slot)
                    .expect("All frozen banks must exist in the Progress map");

                if !stats.computed {
                    let (stake_lockouts, total_staked, bank_weight) = tower.collect_vote_lockouts(
                        bank_slot,
                        bank.vote_accounts().into_iter(),
                        &ancestors,
                    );
                    stats.total_staked = total_staked;
                    stats.weight = bank_weight;
                    stats.fork_weight = stats.weight + parent_weight;

                    datapoint_info!(
                        "bank_weight",
                        ("slot", bank_slot, i64),
                        // u128 too large for influx, convert to hex
                        ("weight", format!("{:X}", stats.weight), String),
                    );
                    info!(
                        "{} slot_weight: {} {} {} {}",
                        my_pubkey,
                        bank_slot,
                        stats.weight,
                        stats.fork_weight,
                        bank.parent().map(|b| b.slot()).unwrap_or(0)
                    );
                    stats.stake_lockouts = stake_lockouts;
                    stats.block_height = bank.block_height();
                    stats.computed = true;
                    new_stats.push(bank_slot);
                }
            }

            Self::update_propagation_status(
                progress,
                bank_slot,
                all_pubkeys,
                bank_forks,
                vote_tracker,
                cluster_slots,
            );

            let stats = progress
                .get_fork_stats_mut(bank_slot)
                .expect("All frozen banks must exist in the Progress map");

            stats.vote_threshold = tower.check_vote_stake_threshold(
                bank_slot,
                &stats.stake_lockouts,
                stats.total_staked,
            );
            stats.is_locked_out = tower.is_locked_out(bank_slot, &ancestors);
            stats.has_voted = tower.has_voted(bank_slot);
            stats.is_recent = tower.is_recent(bank_slot);
        }
        new_stats
    }

    fn update_propagation_status(
        progress: &mut ProgressMap,
        slot: Slot,
        all_pubkeys: &mut HashSet<Rc<Pubkey>>,
        bank_forks: &RwLock<BankForks>,
        vote_tracker: &VoteTracker,
        cluster_slots: &ClusterSlots,
    ) {
        // If propagation has already been confirmed, return
        if progress.is_propagated(slot) {
            return;
        }

        // Otherwise we have to check the votes for confirmation
        let mut slot_vote_tracker = progress
            .get_propagated_stats(slot)
            .expect("All frozen banks must exist in the Progress map")
            .slot_vote_tracker
            .clone();

        if slot_vote_tracker.is_none() {
            slot_vote_tracker = vote_tracker.get_slot_vote_tracker(slot);
            progress
                .get_propagated_stats_mut(slot)
                .expect("All frozen banks must exist in the Progress map")
                .slot_vote_tracker = slot_vote_tracker.clone();
        }

        let mut cluster_slot_pubkeys = progress
            .get_propagated_stats(slot)
            .expect("All frozen banks must exist in the Progress map")
            .cluster_slot_pubkeys
            .clone();

        if cluster_slot_pubkeys.is_none() {
            cluster_slot_pubkeys = cluster_slots.lookup(slot);
            progress
                .get_propagated_stats_mut(slot)
                .expect("All frozen banks must exist in the Progress map")
                .cluster_slot_pubkeys = cluster_slot_pubkeys.clone();
        }

        let newly_voted_pubkeys = slot_vote_tracker
            .as_ref()
            .and_then(|slot_vote_tracker| slot_vote_tracker.write().unwrap().get_updates())
            .unwrap_or_else(|| vec![]);

        let cluster_slot_pubkeys = cluster_slot_pubkeys
            .map(|v| v.read().unwrap().keys().cloned().collect())
            .unwrap_or_else(|| vec![]);

        Self::update_fork_propagated_threshold_from_votes(
            progress,
            newly_voted_pubkeys,
            cluster_slot_pubkeys,
            slot,
            bank_forks,
            all_pubkeys,
        );
    }

    // Returns:
    // 1) The heaviest bank
    // 2) The latest votable bank on the same fork as the last vote
    pub(crate) fn select_forks(
        frozen_banks: &[Arc<Bank>],
        tower: &Tower,
        progress: &ProgressMap,
        ancestors: &HashMap<u64, HashSet<u64>>,
    ) -> (Option<Arc<Bank>>, Option<Arc<Bank>>) {
        let tower_start = Instant::now();
        let num_frozen_banks = frozen_banks.len();

        trace!("frozen_banks {}", frozen_banks.len());
        let num_old_banks = frozen_banks
            .iter()
            .filter(|b| b.slot() < tower.root().unwrap_or(0))
            .count();

        let last_vote = tower.last_vote().slots.last().cloned();
        let mut heaviest_bank_on_same_fork = None;
        let mut heaviest_same_fork_weight = 0;
        let stats: Vec<&ForkStats> = frozen_banks
            .iter()
            .map(|bank| {
                // Only time progress map should be missing a bank slot
                // is if this node was the leader for this slot as those banks
                // are not replayed in replay_active_banks()
                let stats = progress
                    .get_fork_stats(bank.slot())
                    .expect("All frozen banks must exist in the Progress map");

                if let Some(last_vote) = last_vote {
                    if ancestors
                        .get(&bank.slot())
                        .expect("Entry in frozen banks must exist in ancestors")
                        .contains(&last_vote)
                    {
                        // Descendant of last vote cannot be locked out
                        assert!(!stats.is_locked_out);

                        // ancestors(slot) should not contain the slot itself,
                        // so we should never get the same bank as the last vote
                        assert_ne!(bank.slot(), last_vote);
                        // highest weight, lowest slot first. frozen_banks is sorted
                        // from least slot to greatest slot, so if two banks have
                        // the same fork weight, the lower slot will be picked
                        if stats.fork_weight > heaviest_same_fork_weight {
                            heaviest_bank_on_same_fork = Some(bank.clone());
                            heaviest_same_fork_weight = stats.fork_weight;
                        }
                    }
                }

                stats
            })
            .collect();
        let num_not_recent = stats.iter().filter(|s| !s.is_recent).count();
        let num_has_voted = stats.iter().filter(|s| s.has_voted).count();
        let num_empty = stats.iter().filter(|s| s.is_empty).count();
        let num_threshold_failure = stats.iter().filter(|s| !s.vote_threshold).count();
        let num_votable_threshold_failure = stats
            .iter()
            .filter(|s| s.is_recent && !s.has_voted && !s.vote_threshold)
            .count();

        let mut candidates: Vec<_> = frozen_banks.iter().zip(stats.iter()).collect();

        //highest weight, lowest slot first
        candidates.sort_by_key(|b| (b.1.fork_weight, 0i64 - b.0.slot() as i64));
        let rv = candidates.last();
        let ms = timing::duration_as_ms(&tower_start.elapsed());
        let weights: Vec<(u128, u64, u64)> = candidates
            .iter()
            .map(|x| (x.1.weight, x.0.slot(), x.1.block_height))
            .collect();
        debug!(
            "@{:?} tower duration: {:?} len: {}/{} weights: {:?} voting: {}",
            timing::timestamp(),
            ms,
            candidates.len(),
            stats.iter().filter(|s| !s.has_voted).count(),
            weights,
            rv.is_some()
        );
        datapoint_debug!(
            "replay_stage-select_forks",
            ("frozen_banks", num_frozen_banks as i64, i64),
            ("not_recent", num_not_recent as i64, i64),
            ("has_voted", num_has_voted as i64, i64),
            ("old_banks", num_old_banks as i64, i64),
            ("empty_banks", num_empty as i64, i64),
            ("threshold_failure", num_threshold_failure as i64, i64),
            (
                "votable_threshold_failure",
                num_votable_threshold_failure as i64,
                i64
            ),
            ("tower_duration", ms as i64, i64),
        );

        (rv.map(|x| x.0.clone()), heaviest_bank_on_same_fork)
    }

    // Given a heaviest bank, `heaviest_bank` and the next votable bank
    // `heaviest_bank_on_same_fork` as the validator's last vote, return
    // a bank to vote on, a bank to reset to,
    pub(crate) fn select_vote_and_reset_forks(
        heaviest_bank: &Option<Arc<Bank>>,
        heaviest_bank_on_same_fork: &Option<Arc<Bank>>,
        ancestors: &HashMap<u64, HashSet<u64>>,
        descendants: &HashMap<u64, HashSet<u64>>,
        progress: &ProgressMap,
        tower: &Tower,
    ) -> (
        Option<Arc<Bank>>,
        Option<Arc<Bank>>,
        Vec<HeaviestForkFailures>,
    ) {
        // Try to vote on the actual heaviest fork. If the heaviest bank is
        // locked out or fails the threshold check, the validator will:
        // 1) Not continue to vote on current fork, waiting for lockouts to expire/
        //    threshold check to pass
        // 2) Will reset PoH to heaviest fork in order to make sure the heaviest
        //    fork is propagated
        // This above behavior should ensure correct voting and resetting PoH
        // behavior under all cases:
        // 1) The best "selected" bank is on same fork
        // 2) The best "selected" bank is on a different fork,
        //    switch_threshold fails
        // 3) The best "selected" bank is on a different fork,
        //    switch_threshold succceeds
        let mut failure_reasons = vec![];
        let selected_fork = {
            if let Some(bank) = heaviest_bank {
                let switch_threshold = tower.check_switch_threshold(
                    bank.slot(),
                    &ancestors,
                    &descendants,
                    &progress,
                    bank.total_epoch_stake(),
                    bank.epoch_vote_accounts(bank.epoch()).expect(
                        "Bank epoch vote accounts must contain entry for the bank's own epoch",
                    ),
                );
                if !switch_threshold {
                    // If we can't switch, then reset to the the next votable
                    // bank on the same fork as our last vote, but don't vote
                    info!(
                        "Waiting to switch to {}, voting on {:?} on same fork for now",
                        bank.slot(),
                        heaviest_bank_on_same_fork.as_ref().map(|b| b.slot())
                    );
                    failure_reasons.push(HeaviestForkFailures::FailedSwitchThreshold(bank.slot()));
                    heaviest_bank_on_same_fork
                        .as_ref()
                        .map(|b| (b, switch_threshold))
                } else {
                    // If the switch threshold is observed, halt voting on
                    // the current fork and attempt to vote/reset Poh to
                    // the heaviest bank
                    heaviest_bank.as_ref().map(|b| (b, switch_threshold))
                }
            } else {
                None
            }
        };

        if let Some((bank, switch_threshold)) = selected_fork {
            let (is_locked_out, vote_threshold, is_leader_slot, fork_weight) = {
                let fork_stats = progress.get_fork_stats(bank.slot()).unwrap();
                let propagated_stats = &progress.get_propagated_stats(bank.slot()).unwrap();
                (
                    fork_stats.is_locked_out,
                    fork_stats.vote_threshold,
                    propagated_stats.is_leader_slot,
                    fork_stats.weight,
                )
            };

            let propagation_confirmed = is_leader_slot || progress.is_propagated(bank.slot());

            if is_locked_out {
                failure_reasons.push(HeaviestForkFailures::LockedOut(bank.slot()));
            }
            if !vote_threshold {
                failure_reasons.push(HeaviestForkFailures::FailedThreshold(bank.slot()));
            }
            if !propagation_confirmed {
                failure_reasons.push(HeaviestForkFailures::NoPropagatedConfirmation(bank.slot()));
            }
            if !switch_threshold {
                failure_reasons.push(HeaviestForkFailures::FailedSwitchThreshold(bank.slot()));
            }

            if !is_locked_out && vote_threshold && propagation_confirmed && switch_threshold {
                info!("voting: {} {}", bank.slot(), fork_weight);
                (Some(bank.clone()), Some(bank.clone()), failure_reasons)
            } else {
                (None, Some(bank.clone()), failure_reasons)
            }
        } else {
            (None, None, failure_reasons)
        }
    }

    fn update_fork_propagated_threshold_from_votes(
        progress: &mut ProgressMap,
        mut newly_voted_pubkeys: Vec<impl Deref<Target = Pubkey>>,
        mut cluster_slot_pubkeys: Vec<impl Deref<Target = Pubkey>>,
        fork_tip: Slot,
        bank_forks: &RwLock<BankForks>,
        all_pubkeys: &mut HashSet<Rc<Pubkey>>,
    ) {
        let mut current_leader_slot = progress.get_latest_leader_slot(fork_tip);
        let mut did_newly_reach_threshold = false;
        let root = bank_forks.read().unwrap().root();
        loop {
            // These cases mean confirmation of propagation on any earlier
            // leader blocks must have been reached
            if current_leader_slot == None || current_leader_slot.unwrap() <= root {
                break;
            }

            let leader_propagated_stats = progress
                .get_propagated_stats_mut(current_leader_slot.unwrap())
                .expect("current_leader_slot > root, so must exist in the progress map");

            // If a descendant has reached propagation threshold, then
            // all its ancestor banks have also reached propagation
            // threshold as well (Validators can't have voted for a
            // descendant without also getting the ancestor block)
            if leader_propagated_stats.is_propagated ||
                // If there's no new validators to record, and there's no
                // newly achieved threshold, then there's no further
                // information to propagate backwards to past leader blocks
                (newly_voted_pubkeys.is_empty() && cluster_slot_pubkeys.is_empty() &&
                !did_newly_reach_threshold)
            {
                break;
            }

            // We only iterate through the list of leader slots by traversing
            // the linked list of 'prev_leader_slot`'s outlined in the
            // `progress` map
            assert!(leader_propagated_stats.is_leader_slot);
            let leader_bank = bank_forks
                .read()
                .unwrap()
                .get(current_leader_slot.unwrap())
                .expect("Entry in progress map must exist in BankForks")
                .clone();

            did_newly_reach_threshold = Self::update_slot_propagated_threshold_from_votes(
                &mut newly_voted_pubkeys,
                &mut cluster_slot_pubkeys,
                &leader_bank,
                leader_propagated_stats,
                all_pubkeys,
                did_newly_reach_threshold,
            ) || did_newly_reach_threshold;

            // Now jump to process the previous leader slot
            current_leader_slot = leader_propagated_stats.prev_leader_slot;
        }
    }

    fn update_slot_propagated_threshold_from_votes(
        newly_voted_pubkeys: &mut Vec<impl Deref<Target = Pubkey>>,
        cluster_slot_pubkeys: &mut Vec<impl Deref<Target = Pubkey>>,
        leader_bank: &Bank,
        leader_propagated_stats: &mut PropagatedStats,
        all_pubkeys: &mut HashSet<Rc<Pubkey>>,
        did_child_reach_threshold: bool,
    ) -> bool {
        // Track whether this slot newly confirm propagation
        // throughout the network (switched from is_propagated == false
        // to is_propagated == true)
        let mut did_newly_reach_threshold = false;

        // If a child of this slot confirmed propagation, then
        // we can return early as this implies this slot must also
        // be propagated
        if did_child_reach_threshold {
            if !leader_propagated_stats.is_propagated {
                leader_propagated_stats.is_propagated = true;
                return true;
            } else {
                return false;
            }
        }

        if leader_propagated_stats.is_propagated {
            return false;
        }

        // Remove the vote/node pubkeys that we already know voted for this
        // slot. These vote accounts/validator identities are safe to drop
        // because they don't to be ported back any further because earler
        // parents must have:
        // 1) Also recorded these pubkeyss already, or
        // 2) Already reached the propagation threshold, in which case
        //    they no longer need to track the set of propagated validators
        newly_voted_pubkeys.retain(|vote_pubkey| {
            let exists = leader_propagated_stats
                .propagated_validators
                .contains(&**vote_pubkey);
            leader_propagated_stats.add_vote_pubkey(
                &*vote_pubkey,
                all_pubkeys,
                leader_bank.epoch_vote_account_stake(&vote_pubkey),
            );
            !exists
        });

        cluster_slot_pubkeys.retain(|node_pubkey| {
            let exists = leader_propagated_stats
                .propagated_node_ids
                .contains(&**node_pubkey);
            leader_propagated_stats.add_node_pubkey(&*node_pubkey, all_pubkeys, leader_bank);
            !exists
        });

        if leader_propagated_stats.total_epoch_stake == 0
            || leader_propagated_stats.propagated_validators_stake as f64
                / leader_propagated_stats.total_epoch_stake as f64
                > SUPERMINORITY_THRESHOLD
        {
            leader_propagated_stats.is_propagated = true;
            did_newly_reach_threshold = true
        }

        did_newly_reach_threshold
    }

    fn confirm_forks(
        tower: &Tower,
        stake_lockouts: &HashMap<u64, StakeLockout>,
        total_staked: u64,
        progress: &ProgressMap,
        bank_forks: &RwLock<BankForks>,
    ) -> Vec<Slot> {
        let mut confirmed_forks = vec![];
        for (slot, prog) in progress.iter() {
            if !prog.fork_stats.confirmation_reported {
                let bank = bank_forks
                    .read()
                    .unwrap()
                    .get(*slot)
                    .expect("bank in progress must exist in BankForks")
                    .clone();
                let duration = prog.replay_stats.started.elapsed().as_millis();
                if bank.is_frozen() && tower.is_slot_confirmed(*slot, stake_lockouts, total_staked)
                {
                    info!("validator fork confirmed {} {}ms", *slot, duration);
                    datapoint_info!("validator-confirmation", ("duration_ms", duration, i64));
                    confirmed_forks.push(*slot);
                } else {
                    debug!(
                        "validator fork not confirmed {} {}ms {:?}",
                        *slot,
                        duration,
                        stake_lockouts.get(slot)
                    );
                }
            }
        }
        confirmed_forks
    }

    pub(crate) fn handle_new_root(
        new_root: Slot,
        bank_forks: &RwLock<BankForks>,
        progress: &mut ProgressMap,
        accounts_hash_sender: &Option<AccountsPackageSender>,
        all_pubkeys: &mut HashSet<Rc<Pubkey>>,
        largest_confirmed_root: Option<Slot>,
    ) {
        let old_epoch = bank_forks.read().unwrap().root_bank().epoch();
        bank_forks.write().unwrap().set_root(
            new_root,
            accounts_hash_sender,
            largest_confirmed_root,
        );
        let r_bank_forks = bank_forks.read().unwrap();
        let new_epoch = bank_forks.read().unwrap().root_bank().epoch();
        if old_epoch != new_epoch {
            all_pubkeys.retain(|x| Rc::strong_count(x) > 1);
        }
        progress.handle_new_root(&r_bank_forks);
    }

    fn generate_new_bank_forks(
        blockstore: &Blockstore,
        bank_forks: &RwLock<BankForks>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        subscriptions: &Arc<RpcSubscriptions>,
        rewards_recorder_sender: Option<RewardsRecorderSender>,
        progress: &mut ProgressMap,
        all_pubkeys: &mut HashSet<Rc<Pubkey>>,
    ) {
        // Find the next slot that chains to the old slot
        let forks = bank_forks.read().unwrap();
        let frozen_banks = forks.frozen_banks();
        let frozen_bank_slots: Vec<u64> = frozen_banks
            .keys()
            .cloned()
            .filter(|s| *s >= forks.root())
            .collect();
        let next_slots = blockstore
            .get_slots_since(&frozen_bank_slots)
            .expect("Db error");
        // Filter out what we've already seen
        trace!("generate new forks {:?}", {
            let mut next_slots = next_slots.iter().collect::<Vec<_>>();
            next_slots.sort();
            next_slots
        });
        let mut new_banks = HashMap::new();
        for (parent_slot, children) in next_slots {
            let parent_bank = frozen_banks
                .get(&parent_slot)
                .expect("missing parent in bank forks")
                .clone();
            for child_slot in children {
                if forks.get(child_slot).is_some() || new_banks.get(&child_slot).is_some() {
                    trace!("child already active or frozen {}", child_slot);
                    continue;
                }
                let leader = leader_schedule_cache
                    .slot_leader_at(child_slot, Some(&parent_bank))
                    .unwrap();
                info!(
                    "new fork:{} parent:{} root:{}",
                    child_slot,
                    parent_slot,
                    forks.root()
                );
                let child_bank = Self::new_bank_from_parent_with_notify(
                    &parent_bank,
                    child_slot,
                    forks.root(),
                    &leader,
                    &rewards_recorder_sender,
                    subscriptions,
                );
                let empty: Vec<&Pubkey> = vec![];
                Self::update_fork_propagated_threshold_from_votes(
                    progress,
                    empty,
                    vec![&leader],
                    parent_bank.slot(),
                    bank_forks,
                    all_pubkeys,
                );
                new_banks.insert(child_slot, child_bank);
            }
        }
        drop(forks);

        let mut forks = bank_forks.write().unwrap();
        for (_, bank) in new_banks {
            forks.insert(bank);
        }
    }

    fn new_bank_from_parent_with_notify(
        parent: &Arc<Bank>,
        slot: u64,
        root_slot: u64,
        leader: &Pubkey,
        rewards_recorder_sender: &Option<RewardsRecorderSender>,
        subscriptions: &Arc<RpcSubscriptions>,
    ) -> Bank {
        subscriptions.notify_slot(slot, parent.slot(), root_slot);

        let child_bank = Bank::new_from_parent(parent, leader, slot);
        Self::record_rewards(&child_bank, &rewards_recorder_sender);
        child_bank
    }

    fn record_rewards(bank: &Bank, rewards_recorder_sender: &Option<RewardsRecorderSender>) {
        if let Some(rewards_recorder_sender) = rewards_recorder_sender {
            if let Some(ref rewards) = bank.rewards {
                rewards_recorder_sender
                    .send((bank.slot(), rewards.iter().copied().collect()))
                    .unwrap_or_else(|err| warn!("rewards_recorder_sender failed: {:?}", err));
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.commitment_service.join()?;
        self.t_replay.join().map(|_| ())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        commitment::BlockCommitment,
        consensus::test::{initialize_state, VoteSimulator},
        consensus::Tower,
        progress_map::ValidatorStakeInfo,
        replay_stage::ReplayStage,
        transaction_status_service::TransactionStatusService,
    };
    use crossbeam_channel::unbounded;
    use solana_ledger::{
        blockstore::make_slot_entries,
        blockstore::{entries_to_test_shreds, BlockstoreError},
        create_new_tmp_ledger,
        entry::{self, next_entry, Entry},
        genesis_utils::{create_genesis_config, create_genesis_config_with_leader},
        get_tmp_ledger_path,
        shred::{
            CodingShredHeader, DataShredHeader, Shred, ShredCommonHeader, DATA_COMPLETE_SHRED,
            SIZE_OF_COMMON_SHRED_HEADER, SIZE_OF_DATA_SHRED_HEADER, SIZE_OF_DATA_SHRED_PAYLOAD,
        },
    };
    use solana_runtime::genesis_utils::{self, GenesisConfigInfo, ValidatorVoteKeypairs};
    use solana_sdk::{
        account::Account,
        clock::NUM_CONSECUTIVE_LEADER_SLOTS,
        genesis_config,
        hash::{hash, Hash},
        instruction::InstructionError,
        packet::PACKET_DATA_SIZE,
        rent::Rent,
        signature::{Keypair, Signature, Signer},
        system_transaction,
        transaction::TransactionError,
    };
    use solana_stake_program::stake_state;
    use solana_transaction_status::{EncodedTransaction, TransactionWithStatusMeta};
    use solana_vote_program::{
        vote_state::{self, Vote, VoteState, VoteStateVersions},
        vote_transaction,
    };
    use std::{
        fs::remove_dir_all,
        iter,
        sync::{Arc, RwLock},
    };
    use trees::tr;

    struct ForkInfo {
        leader: usize,
        fork: Vec<Slot>,
        voters: Vec<usize>,
    }

    struct ValidatorInfo {
        stake: u64,
        keypair: Keypair,
        authorized_voter_keypair: Keypair,
        staking_keypair: Keypair,
    }

    struct ForkSelectionResponse {
        slot: u64,
        is_locked_out: bool,
    }

    fn simulate_fork_selection(
        neutral_fork: &ForkInfo,
        forks: &Vec<ForkInfo>,
        validators: &Vec<ValidatorInfo>,
    ) -> Vec<Option<ForkSelectionResponse>> {
        fn vote(bank: &Arc<Bank>, pubkey: &Pubkey, slot: Slot) {
            let mut vote_account = bank.get_account(&pubkey).unwrap();
            let mut vote_state = VoteState::from(&vote_account).unwrap();
            vote_state.process_slot_vote_unchecked(slot);
            let versioned = VoteStateVersions::Current(Box::new(vote_state));
            VoteState::to(&versioned, &mut vote_account).unwrap();
            bank.store_account(&pubkey, &vote_account);
        }

        let vote_tracker = VoteTracker::default();
        let cluster_slots = ClusterSlots::default();
        let mut towers: Vec<Tower> = iter::repeat_with(|| Tower::new_for_tests(8, 0.67))
            .take(validators.len())
            .collect();

        for slot in &neutral_fork.fork {
            for tower in towers.iter_mut() {
                tower.record_bank_vote(Vote {
                    hash: Hash::default(),
                    slots: vec![*slot],
                    timestamp: None,
                });
            }
        }

        for fork_info in forks.iter() {
            for slot in fork_info.fork.iter() {
                for voter_index in fork_info.voters.iter() {
                    towers[*voter_index].record_bank_vote(Vote {
                        hash: Hash::default(),
                        slots: vec![*slot],
                        timestamp: None,
                    });
                }
            }
        }

        let genesis_vote_accounts: Vec<Account> = validators
            .iter()
            .map(|validator| {
                vote_state::create_account(
                    &validator.authorized_voter_keypair.pubkey(),
                    &validator.keypair.pubkey(),
                    0,
                    validator.stake,
                )
            })
            .collect();

        let genesis_stake_accounts: Vec<Account> = validators
            .iter()
            .enumerate()
            .map(|(i, validator)| {
                stake_state::create_account(
                    &validator.staking_keypair.pubkey(),
                    &validator.authorized_voter_keypair.pubkey(),
                    &genesis_vote_accounts[i],
                    &Rent::default(),
                    validator.stake,
                )
            })
            .collect();

        let mut genesis_config = create_genesis_config(10_000).genesis_config;
        genesis_config.accounts.clear();

        for i in 0..validators.len() {
            genesis_config.accounts.insert(
                validators[i].authorized_voter_keypair.pubkey(),
                genesis_vote_accounts[i].clone(),
            );
            genesis_config.accounts.insert(
                validators[i].staking_keypair.pubkey(),
                genesis_stake_accounts[i].clone(),
            );
        }

        let mut bank_forks = BankForks::new(neutral_fork.fork[0], Bank::new(&genesis_config));

        let mut fork_progresses: Vec<ProgressMap> = iter::repeat_with(ProgressMap::default)
            .take(validators.len())
            .collect();

        for fork_progress in fork_progresses.iter_mut() {
            let bank = &bank_forks.banks[&0];
            fork_progress
                .entry(neutral_fork.fork[0])
                .or_insert_with(|| ForkProgress::new(bank.last_blockhash(), None, None, 0, 0));
        }

        for index in 1..neutral_fork.fork.len() {
            let bank = Bank::new_from_parent(
                &bank_forks.banks[&neutral_fork.fork[index - 1]].clone(),
                &validators[neutral_fork.leader].keypair.pubkey(),
                neutral_fork.fork[index],
            );

            bank_forks.insert(bank);

            for validator in validators.iter() {
                vote(
                    &bank_forks.banks[&neutral_fork.fork[index]].clone(),
                    &validator.authorized_voter_keypair.pubkey(),
                    neutral_fork.fork[index - 1],
                );
            }

            bank_forks.banks[&neutral_fork.fork[index]].freeze();

            for fork_progress in fork_progresses.iter_mut() {
                let bank = &bank_forks.banks[&neutral_fork.fork[index]];
                fork_progress
                    .entry(bank_forks.banks[&neutral_fork.fork[index]].slot())
                    .or_insert_with(|| ForkProgress::new(bank.last_blockhash(), None, None, 0, 0));
            }
        }

        let last_neutral_bank = &bank_forks.banks[neutral_fork.fork.last().unwrap()].clone();

        for fork_info in forks.iter() {
            for index in 0..fork_info.fork.len() {
                let last_bank: &Arc<Bank>;
                let last_bank_in_fork: Arc<Bank>;

                if index == 0 {
                    last_bank = &last_neutral_bank;
                } else {
                    last_bank_in_fork = bank_forks.banks[&fork_info.fork[index - 1]].clone();
                    last_bank = &last_bank_in_fork;
                }

                let bank = Bank::new_from_parent(
                    last_bank,
                    &validators[fork_info.leader].keypair.pubkey(),
                    fork_info.fork[index],
                );

                bank_forks.insert(bank);

                for voter_index in fork_info.voters.iter() {
                    vote(
                        &bank_forks.banks[&fork_info.fork[index]].clone(),
                        &validators[*voter_index].authorized_voter_keypair.pubkey(),
                        last_bank.slot(),
                    );
                }

                bank_forks.banks[&fork_info.fork[index]].freeze();

                for fork_progress in fork_progresses.iter_mut() {
                    let bank = &bank_forks.banks[&fork_info.fork[index]];
                    fork_progress
                        .entry(bank_forks.banks[&fork_info.fork[index]].slot())
                        .or_insert_with(|| {
                            ForkProgress::new(bank.last_blockhash(), None, None, 0, 0)
                        });
                }
            }
        }

        let bank_fork_ancestors = bank_forks.ancestors();
        let wrapped_bank_fork = Arc::new(RwLock::new(bank_forks));
        let mut all_pubkeys = HashSet::new();
        (0..validators.len())
            .map(|i| {
                let mut frozen_banks: Vec<_> = wrapped_bank_fork
                    .read()
                    .unwrap()
                    .frozen_banks()
                    .values()
                    .cloned()
                    .collect();
                ReplayStage::compute_bank_stats(
                    &validators[i].keypair.pubkey(),
                    &bank_fork_ancestors,
                    &mut frozen_banks,
                    &towers[i],
                    &mut fork_progresses[i],
                    &vote_tracker,
                    &cluster_slots,
                    &wrapped_bank_fork,
                    &mut all_pubkeys,
                );
                let (heaviest_bank, _) = ReplayStage::select_forks(
                    &frozen_banks,
                    &towers[i],
                    &mut fork_progresses[i],
                    &bank_fork_ancestors,
                );

                if heaviest_bank.is_none() {
                    None
                } else {
                    let bank = heaviest_bank.unwrap();
                    let stats = &fork_progresses[i].get_fork_stats(bank.slot()).unwrap();
                    Some(ForkSelectionResponse {
                        slot: bank.slot(),
                        is_locked_out: stats.is_locked_out,
                    })
                }
            })
            .collect()
    }

    #[test]
    fn test_minority_fork_overcommit_attack() {
        let neutral_fork = ForkInfo {
            leader: 0,
            fork: vec![0, 1, 2],
            voters: vec![],
        };

        let forks: Vec<ForkInfo> = vec![
            // Minority fork
            ForkInfo {
                leader: 2,
                fork: (3..=3 + 8).collect(),
                voters: vec![2],
            },
            ForkInfo {
                leader: 1,
                fork: (12..12 + 8).collect(),
                voters: vec![0, 1],
            },
        ];

        let validators: Vec<ValidatorInfo> = vec![
            ValidatorInfo {
                stake: 34_000_000,
                keypair: Keypair::new(),
                authorized_voter_keypair: Keypair::new(),
                staking_keypair: Keypair::new(),
            },
            ValidatorInfo {
                stake: 33_000_000,
                keypair: Keypair::new(),
                authorized_voter_keypair: Keypair::new(),
                staking_keypair: Keypair::new(),
            },
            // Malicious Node
            ValidatorInfo {
                stake: 33_000_000,
                keypair: Keypair::new(),
                authorized_voter_keypair: Keypair::new(),
                staking_keypair: Keypair::new(),
            },
        ];

        let resp = simulate_fork_selection(&neutral_fork, &forks, &validators);
        // Both honest nodes are now want to switch to minority fork and are locked out
        assert!(resp[0].is_some());
        assert_eq!(resp[0].as_ref().unwrap().is_locked_out, true);
        assert_eq!(
            resp[0].as_ref().unwrap().slot,
            forks[0].fork.last().unwrap().clone()
        );
        assert!(resp[1].is_some());
        assert_eq!(resp[1].as_ref().unwrap().is_locked_out, true);
        assert_eq!(
            resp[1].as_ref().unwrap().slot,
            forks[0].fork.last().unwrap().clone()
        );
    }

    #[test]
    fn test_child_slots_of_same_parent() {
        let ledger_path = get_tmp_ledger_path!();
        {
            // Setup
            let blockstore = Arc::new(
                Blockstore::open(&ledger_path)
                    .expect("Expected to be able to open database ledger"),
            );
            let validator_authorized_voter_keypairs: Vec<_> = (0..20)
                .map(|_| ValidatorVoteKeypairs::new(Keypair::new(), Keypair::new(), Keypair::new()))
                .collect();

            let validator_voting_keys: HashMap<_, _> = validator_authorized_voter_keypairs
                .iter()
                .map(|v| (v.node_keypair.pubkey(), v.vote_keypair.pubkey()))
                .collect();
            let GenesisConfigInfo { genesis_config, .. } =
                genesis_utils::create_genesis_config_with_vote_accounts(
                    10_000,
                    &validator_authorized_voter_keypairs,
                    100,
                );
            let bank0 = Bank::new(&genesis_config);
            let mut progress = ProgressMap::default();
            progress.insert(
                0,
                ForkProgress::new_from_bank(
                    &bank0,
                    bank0.collector_id(),
                    &Pubkey::default(),
                    None,
                    0,
                    0,
                ),
            );
            let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank0));
            let exit = Arc::new(AtomicBool::new(false));
            let mut bank_forks = BankForks::new(0, bank0);

            // Insert a non-root bank so that the propagation logic will update this
            // bank
            let bank1 = Bank::new_from_parent(
                bank_forks.get(0).unwrap(),
                &leader_schedule_cache.slot_leader_at(1, None).unwrap(),
                1,
            );
            progress.insert(
                1,
                ForkProgress::new_from_bank(
                    &bank1,
                    bank1.collector_id(),
                    &validator_voting_keys.get(&bank1.collector_id()).unwrap(),
                    Some(0),
                    0,
                    0,
                ),
            );
            assert!(progress.get_propagated_stats(1).unwrap().is_leader_slot);
            bank1.freeze();
            bank_forks.insert(bank1);
            let bank_forks = Arc::new(RwLock::new(bank_forks));
            let subscriptions = Arc::new(RpcSubscriptions::new(
                &exit,
                bank_forks.clone(),
                Arc::new(RwLock::new(BlockCommitmentCache::default_with_blockstore(
                    blockstore.clone(),
                ))),
            ));

            // Insert shreds for slot NUM_CONSECUTIVE_LEADER_SLOTS,
            // chaining to slot 1
            let (shreds, _) = make_slot_entries(NUM_CONSECUTIVE_LEADER_SLOTS, 1, 8);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            assert!(bank_forks
                .read()
                .unwrap()
                .get(NUM_CONSECUTIVE_LEADER_SLOTS)
                .is_none());
            ReplayStage::generate_new_bank_forks(
                &blockstore,
                &bank_forks,
                &leader_schedule_cache,
                &subscriptions,
                None,
                &mut progress,
                &mut HashSet::new(),
            );
            assert!(bank_forks
                .read()
                .unwrap()
                .get(NUM_CONSECUTIVE_LEADER_SLOTS)
                .is_some());

            // Insert shreds for slot 2 * NUM_CONSECUTIVE_LEADER_SLOTS,
            // chaining to slot 1
            let (shreds, _) = make_slot_entries(2 * NUM_CONSECUTIVE_LEADER_SLOTS, 1, 8);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            assert!(bank_forks
                .read()
                .unwrap()
                .get(2 * NUM_CONSECUTIVE_LEADER_SLOTS)
                .is_none());
            ReplayStage::generate_new_bank_forks(
                &blockstore,
                &bank_forks,
                &leader_schedule_cache,
                &subscriptions,
                None,
                &mut progress,
                &mut HashSet::new(),
            );
            assert!(bank_forks
                .read()
                .unwrap()
                .get(NUM_CONSECUTIVE_LEADER_SLOTS)
                .is_some());
            assert!(bank_forks
                .read()
                .unwrap()
                .get(2 * NUM_CONSECUTIVE_LEADER_SLOTS)
                .is_some());

            // // There are 20 equally staked acccounts, of which 3 have built
            // banks above or at bank 1. Because 3/20 < SUPERMINORITY_THRESHOLD,
            // we should see 3 validators in bank 1's propagated_validator set.
            let expected_leader_slots = vec![
                1,
                NUM_CONSECUTIVE_LEADER_SLOTS,
                2 * NUM_CONSECUTIVE_LEADER_SLOTS,
            ];
            for slot in expected_leader_slots {
                let leader = leader_schedule_cache.slot_leader_at(slot, None).unwrap();
                let vote_key = validator_voting_keys.get(&leader).unwrap();
                assert!(progress
                    .get_propagated_stats(1)
                    .unwrap()
                    .propagated_validators
                    .contains(vote_key));
            }
        }
    }

    #[test]
    fn test_handle_new_root() {
        let genesis_config = create_genesis_config(10_000).genesis_config;
        let bank0 = Bank::new(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank0)));
        let root = 3;
        let root_bank = Bank::new_from_parent(
            bank_forks.read().unwrap().get(0).unwrap(),
            &Pubkey::default(),
            root,
        );
        bank_forks.write().unwrap().insert(root_bank);
        let mut progress = ProgressMap::default();
        for i in 0..=root {
            progress.insert(i, ForkProgress::new(Hash::default(), None, None, 0, 0));
        }
        ReplayStage::handle_new_root(
            root,
            &bank_forks,
            &mut progress,
            &None,
            &mut HashSet::new(),
            None,
        );
        assert_eq!(bank_forks.read().unwrap().root(), root);
        assert_eq!(progress.len(), 1);
        assert!(progress.get(&root).is_some());
    }

    #[test]
    fn test_handle_new_root_ahead_of_largest_confirmed_root() {
        let genesis_config = create_genesis_config(10_000).genesis_config;
        let bank0 = Bank::new(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank0)));
        let confirmed_root = 1;
        let fork = 2;
        let bank1 = Bank::new_from_parent(
            bank_forks.read().unwrap().get(0).unwrap(),
            &Pubkey::default(),
            confirmed_root,
        );
        bank_forks.write().unwrap().insert(bank1);
        let bank2 = Bank::new_from_parent(
            bank_forks.read().unwrap().get(confirmed_root).unwrap(),
            &Pubkey::default(),
            fork,
        );
        bank_forks.write().unwrap().insert(bank2);
        let root = 3;
        let root_bank = Bank::new_from_parent(
            bank_forks.read().unwrap().get(confirmed_root).unwrap(),
            &Pubkey::default(),
            root,
        );
        bank_forks.write().unwrap().insert(root_bank);
        let mut progress = ProgressMap::default();
        for i in 0..=root {
            progress.insert(i, ForkProgress::new(Hash::default(), None, None, 0, 0));
        }
        ReplayStage::handle_new_root(
            root,
            &bank_forks,
            &mut progress,
            &None,
            &mut HashSet::new(),
            Some(confirmed_root),
        );
        assert_eq!(bank_forks.read().unwrap().root(), root);
        assert!(bank_forks.read().unwrap().get(confirmed_root).is_some());
        assert!(bank_forks.read().unwrap().get(fork).is_none());
        assert_eq!(progress.len(), 2);
        assert!(progress.get(&root).is_some());
        assert!(progress.get(&confirmed_root).is_some());
        assert!(progress.get(&fork).is_none());
    }

    #[test]
    fn test_dead_fork_transaction_error() {
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let missing_keypair = Keypair::new();
        let missing_keypair2 = Keypair::new();

        let res = check_dead_fork(|_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            let entry = entry::next_entry(
                &blockhash,
                hashes_per_tick.saturating_sub(1),
                vec![
                    system_transaction::transfer(&keypair1, &keypair2.pubkey(), 2, blockhash), // should be fine,
                    system_transaction::transfer(
                        &missing_keypair,
                        &missing_keypair2.pubkey(),
                        2,
                        blockhash,
                    ), // should cause AccountNotFound error
                ],
            );
            entries_to_test_shreds(vec![entry], slot, slot.saturating_sub(1), false, 0)
        });

        assert_matches!(
            res,
            Err(BlockstoreProcessorError::InvalidTransaction(
                TransactionError::AccountNotFound
            ))
        );
    }

    #[test]
    fn test_dead_fork_entry_verification_failure() {
        let keypair2 = Keypair::new();
        let res = check_dead_fork(|genesis_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let bad_hash = hash(&[2; 30]);
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            let entry = entry::next_entry(
                // Use wrong blockhash so that the entry causes an entry verification failure
                &bad_hash,
                hashes_per_tick.saturating_sub(1),
                vec![system_transaction::transfer(
                    &genesis_keypair,
                    &keypair2.pubkey(),
                    2,
                    blockhash,
                )],
            );
            entries_to_test_shreds(vec![entry], slot, slot.saturating_sub(1), false, 0)
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidEntryHash);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_dead_fork_invalid_tick_hash_count() {
        let res = check_dead_fork(|_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            assert!(hashes_per_tick > 0);

            let too_few_hashes_tick = Entry::new(&blockhash, hashes_per_tick - 1, vec![]);
            entries_to_test_shreds(
                vec![too_few_hashes_tick],
                slot,
                slot.saturating_sub(1),
                false,
                0,
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidTickHashCount);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_dead_fork_invalid_slot_tick_count() {
        // Too many ticks per slot
        let res = check_dead_fork(|_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            entries_to_test_shreds(
                entry::create_ticks(bank.ticks_per_slot() + 1, hashes_per_tick, blockhash),
                slot,
                slot.saturating_sub(1),
                false,
                0,
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidTickCount);
        } else {
            assert!(false);
        }

        // Too few ticks per slot
        let res = check_dead_fork(|_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            entries_to_test_shreds(
                entry::create_ticks(bank.ticks_per_slot() - 1, hashes_per_tick, blockhash),
                slot,
                slot.saturating_sub(1),
                true,
                0,
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidTickCount);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_dead_fork_invalid_last_tick() {
        let res = check_dead_fork(|_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            entries_to_test_shreds(
                entry::create_ticks(bank.ticks_per_slot(), hashes_per_tick, blockhash),
                slot,
                slot.saturating_sub(1),
                false,
                0,
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidLastTick);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_dead_fork_trailing_entry() {
        let keypair = Keypair::new();
        let res = check_dead_fork(|genesis_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            let mut entries =
                entry::create_ticks(bank.ticks_per_slot(), hashes_per_tick, blockhash.clone());
            let last_entry_hash = entries.last().unwrap().hash;
            let tx =
                system_transaction::transfer(&genesis_keypair, &keypair.pubkey(), 2, blockhash);
            let trailing_entry = entry::next_entry(&last_entry_hash, 1, vec![tx]);
            entries.push(trailing_entry);
            entries_to_test_shreds(entries, slot, slot.saturating_sub(1), true, 0)
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::TrailingEntry);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_dead_fork_entry_deserialize_failure() {
        // Insert entry that causes deserialization failure
        let res = check_dead_fork(|_, _| {
            let payload_len = SIZE_OF_DATA_SHRED_PAYLOAD;
            let gibberish = [0xa5u8; PACKET_DATA_SIZE];
            let mut data_header = DataShredHeader::default();
            data_header.flags |= DATA_COMPLETE_SHRED;
            let mut shred = Shred::new_empty_from_header(
                ShredCommonHeader::default(),
                data_header,
                CodingShredHeader::default(),
            );
            bincode::serialize_into(
                &mut shred.payload[SIZE_OF_COMMON_SHRED_HEADER + SIZE_OF_DATA_SHRED_HEADER..],
                &gibberish[..payload_len],
            )
            .unwrap();
            vec![shred]
        });

        assert_matches!(
            res,
            Err(
                BlockstoreProcessorError::FailedToLoadEntries(BlockstoreError::InvalidShredData(_)),
            )
        );
    }

    // Given a shred and a fatal expected error, check that replaying that shred causes causes the fork to be
    // marked as dead. Returns the error for caller to verify.
    fn check_dead_fork<F>(shred_to_insert: F) -> result::Result<(), BlockstoreProcessorError>
    where
        F: Fn(&Keypair, Arc<Bank>) -> Vec<Shred>,
    {
        let ledger_path = get_tmp_ledger_path!();
        let res = {
            let blockstore = Arc::new(
                Blockstore::open(&ledger_path)
                    .expect("Expected to be able to open database ledger"),
            );
            let GenesisConfigInfo {
                mut genesis_config,
                mint_keypair,
                ..
            } = create_genesis_config(1000);
            genesis_config.poh_config.hashes_per_tick = Some(2);
            let bank0 = Arc::new(Bank::new(&genesis_config));
            let mut progress = ProgressMap::default();
            let last_blockhash = bank0.last_blockhash();
            let mut bank0_progress = progress
                .entry(bank0.slot())
                .or_insert_with(|| ForkProgress::new(last_blockhash, None, None, 0, 0));
            let shreds = shred_to_insert(&mint_keypair, bank0.clone());
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let res = ReplayStage::replay_blockstore_into_bank(
                &bank0,
                &blockstore,
                &mut bank0_progress,
                None,
                &VerifyRecyclers::default(),
            );

            // Check that the erroring bank was marked as dead in the progress map
            assert!(progress
                .get(&bank0.slot())
                .map(|b| b.is_dead)
                .unwrap_or(false));

            // Check that the erroring bank was marked as dead in blockstore
            assert!(blockstore.is_dead(bank0.slot()));
            res.map(|_| ())
        };
        let _ignored = remove_dir_all(&ledger_path);
        res
    }

    #[test]
    fn test_replay_commitment_cache() {
        fn leader_vote(bank: &Arc<Bank>, pubkey: &Pubkey) {
            let mut leader_vote_account = bank.get_account(&pubkey).unwrap();
            let mut vote_state = VoteState::from(&leader_vote_account).unwrap();
            vote_state.process_slot_vote_unchecked(bank.slot());
            let versioned = VoteStateVersions::Current(Box::new(vote_state));
            VoteState::to(&versioned, &mut leader_vote_account).unwrap();
            bank.store_account(&pubkey, &leader_vote_account);
        }

        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::default_with_blockstore(blockstore.clone()),
        ));
        let (lockouts_sender, _) = AggregateCommitmentService::new(
            &Arc::new(AtomicBool::new(false)),
            block_commitment_cache.clone(),
        );

        let leader_pubkey = Pubkey::new_rand();
        let leader_lamports = 3;
        let genesis_config_info =
            create_genesis_config_with_leader(50, &leader_pubkey, leader_lamports);
        let mut genesis_config = genesis_config_info.genesis_config;
        let leader_voting_pubkey = genesis_config_info.voting_keypair.pubkey();
        genesis_config.epoch_schedule.warmup = false;
        genesis_config.ticks_per_slot = 4;
        let bank0 = Bank::new(&genesis_config);
        for _ in 0..genesis_config.ticks_per_slot {
            bank0.register_tick(&Hash::default());
        }
        bank0.freeze();
        let arc_bank0 = Arc::new(bank0);
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(
            &[arc_bank0.clone()],
            0,
        )));

        assert!(block_commitment_cache
            .read()
            .unwrap()
            .get_block_commitment(0)
            .is_none());
        assert!(block_commitment_cache
            .read()
            .unwrap()
            .get_block_commitment(1)
            .is_none());

        let bank1 = Bank::new_from_parent(&arc_bank0, &Pubkey::default(), arc_bank0.slot() + 1);
        let _res = bank1.transfer(10, &genesis_config_info.mint_keypair, &Pubkey::new_rand());
        for _ in 0..genesis_config.ticks_per_slot {
            bank1.register_tick(&Hash::default());
        }
        bank1.freeze();
        bank_forks.write().unwrap().insert(bank1);
        let arc_bank1 = bank_forks.read().unwrap().get(1).unwrap().clone();
        leader_vote(&arc_bank1, &leader_voting_pubkey);
        ReplayStage::update_commitment_cache(
            arc_bank1.clone(),
            0,
            leader_lamports,
            &lockouts_sender,
        );

        let bank2 = Bank::new_from_parent(&arc_bank1, &Pubkey::default(), arc_bank1.slot() + 1);
        let _res = bank2.transfer(10, &genesis_config_info.mint_keypair, &Pubkey::new_rand());
        for _ in 0..genesis_config.ticks_per_slot {
            bank2.register_tick(&Hash::default());
        }
        bank2.freeze();
        bank_forks.write().unwrap().insert(bank2);
        let arc_bank2 = bank_forks.read().unwrap().get(2).unwrap().clone();
        leader_vote(&arc_bank2, &leader_voting_pubkey);
        ReplayStage::update_commitment_cache(
            arc_bank2.clone(),
            0,
            leader_lamports,
            &lockouts_sender,
        );
        thread::sleep(Duration::from_millis(200));

        let mut expected0 = BlockCommitment::default();
        expected0.increase_confirmation_stake(2, leader_lamports);
        assert_eq!(
            block_commitment_cache
                .read()
                .unwrap()
                .get_block_commitment(0)
                .unwrap(),
            &expected0,
        );
        let mut expected1 = BlockCommitment::default();
        expected1.increase_confirmation_stake(2, leader_lamports);
        assert_eq!(
            block_commitment_cache
                .read()
                .unwrap()
                .get_block_commitment(1)
                .unwrap(),
            &expected1
        );
        let mut expected2 = BlockCommitment::default();
        expected2.increase_confirmation_stake(1, leader_lamports);
        assert_eq!(
            block_commitment_cache
                .read()
                .unwrap()
                .get_block_commitment(2)
                .unwrap(),
            &expected2
        );
    }

    pub fn create_test_transactions_and_populate_blockstore(
        keypairs: Vec<&Keypair>,
        previous_slot: Slot,
        bank: Arc<Bank>,
        blockstore: Arc<Blockstore>,
    ) -> Vec<Signature> {
        let mint_keypair = keypairs[0];
        let keypair1 = keypairs[1];
        let keypair2 = keypairs[2];
        let keypair3 = keypairs[3];
        let slot = bank.slot();
        let blockhash = bank.confirmed_last_blockhash().0;

        // Generate transactions for processing
        // Successful transaction
        let success_tx =
            system_transaction::transfer(&mint_keypair, &keypair1.pubkey(), 2, blockhash);
        let success_signature = success_tx.signatures[0];
        let entry_1 = next_entry(&blockhash, 1, vec![success_tx]);
        // Failed transaction, InstructionError
        let ix_error_tx =
            system_transaction::transfer(&keypair2, &keypair3.pubkey(), 10, blockhash);
        let ix_error_signature = ix_error_tx.signatures[0];
        let entry_2 = next_entry(&entry_1.hash, 1, vec![ix_error_tx]);
        // Failed transaction
        let fail_tx =
            system_transaction::transfer(&mint_keypair, &keypair2.pubkey(), 2, Hash::default());
        let entry_3 = next_entry(&entry_2.hash, 1, vec![fail_tx]);
        let entries = vec![entry_1, entry_2, entry_3];

        let shreds = entries_to_test_shreds(entries.clone(), slot, previous_slot, true, 0);
        blockstore.insert_shreds(shreds, None, false).unwrap();
        blockstore.set_roots(&[slot]).unwrap();

        let (transaction_status_sender, transaction_status_receiver) = unbounded();
        let transaction_status_service = TransactionStatusService::new(
            transaction_status_receiver,
            blockstore.clone(),
            &Arc::new(AtomicBool::new(false)),
        );

        // Check that process_entries successfully writes can_commit transactions statuses, and
        // that they are matched properly by get_confirmed_block
        let _result = blockstore_processor::process_entries(
            &bank,
            &entries,
            true,
            Some(transaction_status_sender),
        );

        transaction_status_service.join().unwrap();

        vec![success_signature, ix_error_signature]
    }

    #[test]
    fn test_write_persist_transaction_status() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1000);
        let (ledger_path, _) = create_new_tmp_ledger!(&genesis_config);
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to successfully open database ledger");
            let blockstore = Arc::new(blockstore);

            let keypair1 = Keypair::new();
            let keypair2 = Keypair::new();
            let keypair3 = Keypair::new();

            let bank0 = Arc::new(Bank::new(&genesis_config));
            bank0
                .transfer(4, &mint_keypair, &keypair2.pubkey())
                .unwrap();

            let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
            let slot = bank1.slot();

            let signatures = create_test_transactions_and_populate_blockstore(
                vec![&mint_keypair, &keypair1, &keypair2, &keypair3],
                bank0.slot(),
                bank1,
                blockstore.clone(),
            );

            let confirmed_block = blockstore.get_confirmed_block(slot, None).unwrap();
            assert_eq!(confirmed_block.transactions.len(), 3);

            for TransactionWithStatusMeta { transaction, meta } in
                confirmed_block.transactions.into_iter()
            {
                if let EncodedTransaction::Json(transaction) = transaction {
                    if transaction.signatures[0] == signatures[0].to_string() {
                        let meta = meta.unwrap();
                        assert_eq!(meta.err, None);
                        assert_eq!(meta.status, Ok(()));
                    } else if transaction.signatures[0] == signatures[1].to_string() {
                        let meta = meta.unwrap();
                        assert_eq!(
                            meta.err,
                            Some(TransactionError::InstructionError(
                                0,
                                InstructionError::Custom(1)
                            ))
                        );
                        assert_eq!(
                            meta.status,
                            Err(TransactionError::InstructionError(
                                0,
                                InstructionError::Custom(1)
                            ))
                        );
                    } else {
                        assert_eq!(meta, None);
                    }
                }
            }
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_compute_bank_stats_confirmed() {
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let stake_keypair = Keypair::new();
        let node_pubkey = node_keypair.pubkey();
        let mut keypairs = HashMap::new();
        keypairs.insert(
            node_pubkey,
            ValidatorVoteKeypairs::new(node_keypair, vote_keypair, stake_keypair),
        );

        let (bank_forks, mut progress) = initialize_state(&keypairs, 10_000);
        let bank0 = bank_forks.get(0).unwrap().clone();
        let my_keypairs = keypairs.get(&node_pubkey).unwrap();
        let vote_tx = vote_transaction::new_vote_transaction(
            vec![0],
            bank0.hash(),
            bank0.last_blockhash(),
            &my_keypairs.node_keypair,
            &my_keypairs.vote_keypair,
            &my_keypairs.vote_keypair,
        );

        let bank_forks = RwLock::new(bank_forks);
        let bank1 = Bank::new_from_parent(&bank0, &node_pubkey, 1);
        bank1.process_transaction(&vote_tx).unwrap();
        bank1.freeze();

        // Test confirmations
        let ancestors = bank_forks.read().unwrap().ancestors();
        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();
        let tower = Tower::new_for_tests(0, 0.67);
        let newly_computed = ReplayStage::compute_bank_stats(
            &node_pubkey,
            &ancestors,
            &mut frozen_banks,
            &tower,
            &mut progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &bank_forks,
            &mut HashSet::new(),
        );
        assert_eq!(newly_computed, vec![0]);
        // The only vote is in bank 1, and bank_forks does not currently contain
        // bank 1, so no slot should be confirmed.
        {
            let fork_progress = progress.get(&0).unwrap();
            let confirmed_forks = ReplayStage::confirm_forks(
                &tower,
                &fork_progress.fork_stats.stake_lockouts,
                fork_progress.fork_stats.total_staked,
                &progress,
                &bank_forks,
            );

            assert!(confirmed_forks.is_empty())
        }

        // Insert the bank that contains a vote for slot 0, which confirms slot 0
        bank_forks.write().unwrap().insert(bank1);
        progress.insert(
            1,
            ForkProgress::new(bank0.last_blockhash(), None, None, 0, 0),
        );
        let ancestors = bank_forks.read().unwrap().ancestors();
        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();
        let newly_computed = ReplayStage::compute_bank_stats(
            &node_pubkey,
            &ancestors,
            &mut frozen_banks,
            &tower,
            &mut progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &bank_forks,
            &mut HashSet::new(),
        );

        assert_eq!(newly_computed, vec![1]);
        {
            let fork_progress = progress.get(&1).unwrap();
            let confirmed_forks = ReplayStage::confirm_forks(
                &tower,
                &fork_progress.fork_stats.stake_lockouts,
                fork_progress.fork_stats.total_staked,
                &progress,
                &bank_forks,
            );
            assert_eq!(confirmed_forks, vec![0]);
        }

        let ancestors = bank_forks.read().unwrap().ancestors();
        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();
        let newly_computed = ReplayStage::compute_bank_stats(
            &node_pubkey,
            &ancestors,
            &mut frozen_banks,
            &tower,
            &mut progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &bank_forks,
            &mut HashSet::new(),
        );
        // No new stats should have been computed
        assert!(newly_computed.is_empty());
    }

    #[test]
    fn test_same_weight_select_lower_slot() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(1);
        let node_pubkey = vote_simulator.node_pubkeys[0];
        let tower = Tower::new_with_key(&node_pubkey);

        // Create the tree of banks in a BankForks object
        let forks = tr(0) / (tr(1)) / (tr(2));
        vote_simulator.fill_bank_forks(forks, &HashMap::new());
        let mut frozen_banks: Vec<_> = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();

        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();
        ReplayStage::compute_bank_stats(
            &Pubkey::default(),
            &ancestors,
            &mut frozen_banks,
            &tower,
            &mut vote_simulator.progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &vote_simulator.bank_forks,
            &mut HashSet::new(),
        );

        assert_eq!(
            vote_simulator
                .progress
                .get_fork_stats(1)
                .unwrap()
                .fork_weight,
            vote_simulator
                .progress
                .get_fork_stats(2)
                .unwrap()
                .fork_weight,
        );
        let (heaviest_bank, _) =
            ReplayStage::select_forks(&frozen_banks, &tower, &vote_simulator.progress, &ancestors);

        // Should pick the lower of the two equally weighted banks
        assert_eq!(heaviest_bank.unwrap().slot(), 1);
    }

    #[test]
    fn test_child_bank_heavier() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(1);
        let node_pubkey = vote_simulator.node_pubkeys[0];
        let mut tower = Tower::new_with_key(&node_pubkey);

        // Create the tree of banks in a BankForks object
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3))));

        // Set the voting behavior
        let mut cluster_votes = HashMap::new();
        let votes = vec![0, 2];
        cluster_votes.insert(node_pubkey, votes.clone());
        vote_simulator.fill_bank_forks(forks, &cluster_votes);
        for vote in votes {
            assert!(vote_simulator
                .simulate_vote(vote, &node_pubkey, &mut tower,)
                .is_empty());
        }

        let mut frozen_banks: Vec<_> = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();

        ReplayStage::compute_bank_stats(
            &Pubkey::default(),
            &vote_simulator.bank_forks.read().unwrap().ancestors(),
            &mut frozen_banks,
            &tower,
            &mut vote_simulator.progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &vote_simulator.bank_forks,
            &mut HashSet::new(),
        );

        frozen_banks.sort_by_key(|bank| bank.slot());
        for pair in frozen_banks.windows(2) {
            let first = vote_simulator
                .progress
                .get_fork_stats(pair[0].slot())
                .unwrap()
                .fork_weight;
            let second = vote_simulator
                .progress
                .get_fork_stats(pair[1].slot())
                .unwrap()
                .fork_weight;
            assert!(second >= first);
        }
    }

    #[test]
    fn test_should_retransmit() {
        let poh_slot = 4;
        let mut last_retransmit_slot = 4;
        // We retransmitted already at slot 4, shouldn't retransmit until
        // >= 4 + NUM_CONSECUTIVE_LEADER_SLOTS, or if we reset to < 4
        assert!(!ReplayStage::should_retransmit(
            poh_slot,
            &mut last_retransmit_slot
        ));
        assert_eq!(last_retransmit_slot, 4);

        for poh_slot in 4..4 + NUM_CONSECUTIVE_LEADER_SLOTS {
            assert!(!ReplayStage::should_retransmit(
                poh_slot,
                &mut last_retransmit_slot
            ));
            assert_eq!(last_retransmit_slot, 4);
        }

        let poh_slot = 4 + NUM_CONSECUTIVE_LEADER_SLOTS;
        last_retransmit_slot = 4;
        assert!(ReplayStage::should_retransmit(
            poh_slot,
            &mut last_retransmit_slot
        ));
        assert_eq!(last_retransmit_slot, poh_slot);

        let poh_slot = 3;
        last_retransmit_slot = 4;
        assert!(ReplayStage::should_retransmit(
            poh_slot,
            &mut last_retransmit_slot
        ));
        assert_eq!(last_retransmit_slot, poh_slot);
    }

    #[test]
    fn test_update_slot_propagated_threshold_from_votes() {
        let keypairs: HashMap<_, _> = iter::repeat_with(|| {
            let node_keypair = Keypair::new();
            let vote_keypair = Keypair::new();
            let stake_keypair = Keypair::new();
            let node_pubkey = node_keypair.pubkey();
            (
                node_pubkey,
                ValidatorVoteKeypairs::new(node_keypair, vote_keypair, stake_keypair),
            )
        })
        .take(10)
        .collect();

        let new_vote_pubkeys: Vec<_> = keypairs
            .values()
            .map(|keys| keys.vote_keypair.pubkey())
            .collect();
        let new_node_pubkeys: Vec<_> = keypairs
            .values()
            .map(|keys| keys.node_keypair.pubkey())
            .collect();

        // Once 4/10 validators have voted, we have hit threshold
        run_test_update_slot_propagated_threshold_from_votes(&keypairs, &new_vote_pubkeys, &[], 4);
        // Adding the same node pubkey's instead of the corresponding
        // vote pubkeys should be equivalent
        run_test_update_slot_propagated_threshold_from_votes(&keypairs, &[], &new_node_pubkeys, 4);
        // Adding the same node pubkey's in the same order as their
        // corresponding vote accounts is redundant, so we don't
        // reach the threshold any sooner.
        run_test_update_slot_propagated_threshold_from_votes(
            &keypairs,
            &new_vote_pubkeys,
            &new_node_pubkeys,
            4,
        );
        // However, if we add different node pubkey's than the
        // vote accounts, we should hit threshold much faster
        // because now we are getting 2 new pubkeys on each
        // iteration instead of 1, so by the 2nd iteration
        // we should have 4/10 validators voting
        run_test_update_slot_propagated_threshold_from_votes(
            &keypairs,
            &new_vote_pubkeys[0..5],
            &new_node_pubkeys[5..],
            2,
        );
    }

    fn run_test_update_slot_propagated_threshold_from_votes(
        all_keypairs: &HashMap<Pubkey, ValidatorVoteKeypairs>,
        new_vote_pubkeys: &[Pubkey],
        new_node_pubkeys: &[Pubkey],
        success_index: usize,
    ) {
        let stake = 10_000;
        let (bank_forks, _) = initialize_state(&all_keypairs, stake);
        let root_bank = bank_forks.root_bank().clone();
        let mut propagated_stats = PropagatedStats {
            total_epoch_stake: stake * all_keypairs.len() as u64,
            ..PropagatedStats::default()
        };

        let mut all_pubkeys = HashSet::new();
        let child_reached_threshold = false;
        for i in 0..std::cmp::max(new_vote_pubkeys.len(), new_node_pubkeys.len()) {
            propagated_stats.is_propagated = false;
            let len = std::cmp::min(i, new_vote_pubkeys.len());
            let mut voted_pubkeys = new_vote_pubkeys[..len]
                .iter()
                .cloned()
                .map(Arc::new)
                .collect();
            let len = std::cmp::min(i, new_node_pubkeys.len());
            let mut node_pubkeys = new_node_pubkeys[..len]
                .iter()
                .cloned()
                .map(Arc::new)
                .collect();
            let did_newly_reach_threshold =
                ReplayStage::update_slot_propagated_threshold_from_votes(
                    &mut voted_pubkeys,
                    &mut node_pubkeys,
                    &root_bank,
                    &mut propagated_stats,
                    &mut all_pubkeys,
                    child_reached_threshold,
                );

            // Only the i'th voted pubkey should be new (everything else was
            // inserted in previous iteration of the loop), so those redundant
            // pubkeys should have been filtered out
            let remaining_vote_pubkeys = {
                if i == 0 || i >= new_vote_pubkeys.len() {
                    vec![]
                } else {
                    vec![Arc::new(new_vote_pubkeys[i - 1])]
                }
            };
            let remaining_node_pubkeys = {
                if i == 0 || i >= new_node_pubkeys.len() {
                    vec![]
                } else {
                    vec![Arc::new(new_node_pubkeys[i - 1])]
                }
            };
            assert_eq!(voted_pubkeys, remaining_vote_pubkeys);
            assert_eq!(node_pubkeys, remaining_node_pubkeys);

            // If we crossed the superminority threshold, then
            // `did_newly_reach_threshold == true`, otherwise the
            // threshold has not been reached
            if i >= success_index {
                assert!(propagated_stats.is_propagated);
                assert!(did_newly_reach_threshold);
            } else {
                assert!(!propagated_stats.is_propagated);
                assert!(!did_newly_reach_threshold);
            }
        }
    }

    #[test]
    fn test_update_slot_propagated_threshold_from_votes2() {
        let mut empty: Vec<&Pubkey> = vec![];
        let genesis_config = create_genesis_config(100_000_000).genesis_config;
        let root_bank = Bank::new(&genesis_config);
        let stake = 10_000;
        // Simulate a child slot seeing threshold (`child_reached_threshold` = true),
        // then the parent should also be marked as having reached threshold,
        // even if there are no new pubkeys to add (`newly_voted_pubkeys.is_empty()`)
        let mut propagated_stats = PropagatedStats {
            total_epoch_stake: stake * 10,
            ..PropagatedStats::default()
        };
        propagated_stats.total_epoch_stake = stake * 10;
        let mut all_pubkeys = HashSet::new();
        let child_reached_threshold = true;
        let mut newly_voted_pubkeys: Vec<Arc<Pubkey>> = vec![];

        assert!(ReplayStage::update_slot_propagated_threshold_from_votes(
            &mut newly_voted_pubkeys,
            &mut empty,
            &root_bank,
            &mut propagated_stats,
            &mut all_pubkeys,
            child_reached_threshold,
        ));

        // If propagation already happened (propagated_stats.is_propagated = true),
        // always returns false
        propagated_stats = PropagatedStats {
            total_epoch_stake: stake * 10,
            ..PropagatedStats::default()
        };
        propagated_stats.is_propagated = true;
        all_pubkeys = HashSet::new();
        newly_voted_pubkeys = vec![];
        assert!(!ReplayStage::update_slot_propagated_threshold_from_votes(
            &mut newly_voted_pubkeys,
            &mut empty,
            &root_bank,
            &mut propagated_stats,
            &mut all_pubkeys,
            child_reached_threshold,
        ));

        let child_reached_threshold = false;
        assert!(!ReplayStage::update_slot_propagated_threshold_from_votes(
            &mut newly_voted_pubkeys,
            &mut empty,
            &root_bank,
            &mut propagated_stats,
            &mut all_pubkeys,
            child_reached_threshold,
        ));
    }

    #[test]
    fn test_update_propagation_status() {
        // Create genesis stakers
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let stake_keypair = Keypair::new();
        let vote_pubkey = Arc::new(vote_keypair.pubkey());
        let mut keypairs = HashMap::new();

        keypairs.insert(
            node_keypair.pubkey(),
            ValidatorVoteKeypairs::new(node_keypair, vote_keypair, stake_keypair),
        );

        let stake = 10_000;
        let (mut bank_forks, mut progress_map) = initialize_state(&keypairs, stake);

        let bank0 = bank_forks.get(0).unwrap().clone();
        bank_forks.insert(Bank::new_from_parent(&bank0, &Pubkey::default(), 9));
        let bank9 = bank_forks.get(9).unwrap().clone();
        bank_forks.insert(Bank::new_from_parent(&bank9, &Pubkey::default(), 10));
        bank_forks.set_root(9, &None, None);
        let total_epoch_stake = bank0.total_epoch_stake();

        // Insert new ForkProgress for slot 10 and its
        // previous leader slot 9
        progress_map.insert(
            10,
            ForkProgress::new(
                Hash::default(),
                Some(9),
                Some(ValidatorStakeInfo {
                    total_epoch_stake,
                    ..ValidatorStakeInfo::default()
                }),
                0,
                0,
            ),
        );
        progress_map.insert(
            9,
            ForkProgress::new(
                Hash::default(),
                Some(8),
                Some(ValidatorStakeInfo {
                    total_epoch_stake,
                    ..ValidatorStakeInfo::default()
                }),
                0,
                0,
            ),
        );

        // Make sure is_propagated == false so that the propagation logic
        // runs in `update_propagation_status`
        assert!(!progress_map.is_propagated(10));

        let vote_tracker = VoteTracker::new(&bank_forks.root_bank());
        vote_tracker.insert_vote(10, vote_pubkey.clone());
        ReplayStage::update_propagation_status(
            &mut progress_map,
            10,
            &mut HashSet::new(),
            &RwLock::new(bank_forks),
            &vote_tracker,
            &ClusterSlots::default(),
        );

        let propagated_stats = &progress_map.get(&10).unwrap().propagated_stats;

        // There should now be a cached reference to the VoteTracker for
        // slot 10
        assert!(propagated_stats.slot_vote_tracker.is_some());

        // Updates should have been consumed
        assert!(propagated_stats
            .slot_vote_tracker
            .as_ref()
            .unwrap()
            .write()
            .unwrap()
            .get_updates()
            .is_none());

        // The voter should be recorded
        assert!(propagated_stats
            .propagated_validators
            .contains(&*vote_pubkey));

        assert_eq!(propagated_stats.propagated_validators_stake, stake);
    }

    #[test]
    fn test_chain_update_propagation_status() {
        let keypairs: HashMap<_, _> = iter::repeat_with(|| {
            let node_keypair = Keypair::new();
            let vote_keypair = Keypair::new();
            let stake_keypair = Keypair::new();
            let node_pubkey = node_keypair.pubkey();
            (
                node_pubkey,
                ValidatorVoteKeypairs::new(node_keypair, vote_keypair, stake_keypair),
            )
        })
        .take(10)
        .collect();

        let vote_pubkeys: Vec<_> = keypairs
            .values()
            .map(|keys| keys.vote_keypair.pubkey())
            .collect();

        let stake_per_validator = 10_000;
        let (mut bank_forks, mut progress_map) = initialize_state(&keypairs, stake_per_validator);
        bank_forks.set_root(0, &None, None);
        let total_epoch_stake = bank_forks.root_bank().total_epoch_stake();

        // Insert new ForkProgress representing a slot for all slots 1..=num_banks. Only
        // make even numbered ones leader slots
        for i in 1..=10 {
            let parent_bank = bank_forks.get(i - 1).unwrap().clone();
            let prev_leader_slot = ((i - 1) / 2) * 2;
            bank_forks.insert(Bank::new_from_parent(&parent_bank, &Pubkey::default(), i));
            progress_map.insert(
                i,
                ForkProgress::new(
                    Hash::default(),
                    Some(prev_leader_slot),
                    {
                        if i % 2 == 0 {
                            Some(ValidatorStakeInfo {
                                total_epoch_stake,
                                ..ValidatorStakeInfo::default()
                            })
                        } else {
                            None
                        }
                    },
                    0,
                    0,
                ),
            );
        }

        let vote_tracker = VoteTracker::new(&bank_forks.root_bank());
        for vote_pubkey in &vote_pubkeys {
            // Insert a vote for the last bank for each voter
            vote_tracker.insert_vote(10, Arc::new(vote_pubkey.clone()));
        }

        // The last bank should reach propagation threshold, and propagate it all
        // the way back through earlier leader banks
        ReplayStage::update_propagation_status(
            &mut progress_map,
            10,
            &mut HashSet::new(),
            &RwLock::new(bank_forks),
            &vote_tracker,
            &ClusterSlots::default(),
        );

        for i in 1..=10 {
            let propagated_stats = &progress_map.get(&i).unwrap().propagated_stats;
            // Only the even numbered ones were leader banks, so only
            // those should have been updated
            if i % 2 == 0 {
                assert!(propagated_stats.is_propagated);
            } else {
                assert!(!propagated_stats.is_propagated);
            }
        }
    }

    #[test]
    fn test_chain_update_propagation_status2() {
        let num_validators = 6;
        let keypairs: HashMap<_, _> = iter::repeat_with(|| {
            let node_keypair = Keypair::new();
            let vote_keypair = Keypair::new();
            let stake_keypair = Keypair::new();
            let node_pubkey = node_keypair.pubkey();
            (
                node_pubkey,
                ValidatorVoteKeypairs::new(node_keypair, vote_keypair, stake_keypair),
            )
        })
        .take(num_validators)
        .collect();

        let vote_pubkeys: Vec<_> = keypairs
            .values()
            .map(|keys| keys.vote_keypair.pubkey())
            .collect();

        let stake_per_validator = 10_000;
        let (mut bank_forks, mut progress_map) = initialize_state(&keypairs, stake_per_validator);
        bank_forks.set_root(0, &None, None);

        let total_epoch_stake = num_validators as u64 * stake_per_validator;

        // Insert new ForkProgress representing a slot for all slots 1..=num_banks. Only
        // make even numbered ones leader slots
        for i in 1..=10 {
            let parent_bank = bank_forks.get(i - 1).unwrap().clone();
            let prev_leader_slot = i - 1;
            bank_forks.insert(Bank::new_from_parent(&parent_bank, &Pubkey::default(), i));
            let mut fork_progress = ForkProgress::new(
                Hash::default(),
                Some(prev_leader_slot),
                Some(ValidatorStakeInfo {
                    total_epoch_stake,
                    ..ValidatorStakeInfo::default()
                }),
                0,
                0,
            );

            let end_range = {
                // The earlier slots are one pubkey away from reaching confirmation
                if i < 5 {
                    2
                } else {
                    // The later slots are two pubkeys away from reaching confirmation
                    1
                }
            };
            fork_progress.propagated_stats.propagated_validators = vote_pubkeys[0..end_range]
                .iter()
                .cloned()
                .map(Rc::new)
                .collect();
            fork_progress.propagated_stats.propagated_validators_stake =
                end_range as u64 * stake_per_validator;
            progress_map.insert(i, fork_progress);
        }

        let vote_tracker = VoteTracker::new(&bank_forks.root_bank());
        // Insert a new vote
        vote_tracker.insert_vote(10, Arc::new(vote_pubkeys[2].clone()));

        // The last bank should reach propagation threshold, and propagate it all
        // the way back through earlier leader banks
        ReplayStage::update_propagation_status(
            &mut progress_map,
            10,
            &mut HashSet::new(),
            &RwLock::new(bank_forks),
            &vote_tracker,
            &ClusterSlots::default(),
        );

        // Only the first 5 banks should have reached the threshold
        for i in 1..=10 {
            let propagated_stats = &progress_map.get(&i).unwrap().propagated_stats;
            if i < 5 {
                assert!(propagated_stats.is_propagated);
            } else {
                assert!(!propagated_stats.is_propagated);
            }
        }
    }

    #[test]
    fn test_check_propagation_for_start_leader() {
        let mut progress_map = ProgressMap::default();
        let poh_slot = 5;
        let parent_slot = 3;

        // If there is no previous leader slot (previous leader slot is None),
        // should succeed
        progress_map.insert(3, ForkProgress::new(Hash::default(), None, None, 0, 0));
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));

        // If the parent was itself the leader, then requires propagation confirmation
        progress_map.insert(
            3,
            ForkProgress::new(
                Hash::default(),
                None,
                Some(ValidatorStakeInfo::default()),
                0,
                0,
            ),
        );
        assert!(!ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));
        progress_map
            .get_mut(&3)
            .unwrap()
            .propagated_stats
            .is_propagated = true;
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));
        // Now, set up the progress map to show that the previous leader slot of 5 is
        // 2 (even though the parent is 3), so 2 needs to see propagation confirmation
        // before we can start a leader for block 5
        progress_map.insert(3, ForkProgress::new(Hash::default(), Some(2), None, 0, 0));
        progress_map.insert(
            2,
            ForkProgress::new(
                Hash::default(),
                None,
                Some(ValidatorStakeInfo::default()),
                0,
                0,
            ),
        );

        // Last leader slot has not seen propagation threshold, so should fail
        assert!(!ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));

        // If we set the is_propagated = true for the last leader slot, should
        // allow the block to be generated
        progress_map
            .get_mut(&2)
            .unwrap()
            .propagated_stats
            .is_propagated = true;
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));

        // If the root is 3, this filters out slot 2 from the progress map,
        // which implies confirmation
        let mut bank_forks = BankForks::new(
            3,
            Bank::new(&genesis_config::create_genesis_config(10000).0),
        );
        let bank5 = Bank::new_from_parent(bank_forks.get(3).unwrap(), &Pubkey::default(), 5);
        bank_forks.insert(bank5);

        // Should purge only slot 2 from the progress map
        progress_map.handle_new_root(&bank_forks);

        // Should succeed
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));
    }

    #[test]
    fn test_check_propagation_for_consecutive_start_leader() {
        let mut progress_map = ProgressMap::default();
        let poh_slot = 4;
        let mut parent_slot = 3;

        // Set up the progress map to show that the last leader slot of 4 is 3,
        // which means 3 and 4 are consecutiive leader slots
        progress_map.insert(
            3,
            ForkProgress::new(
                Hash::default(),
                None,
                Some(ValidatorStakeInfo::default()),
                0,
                0,
            ),
        );
        progress_map.insert(
            2,
            ForkProgress::new(
                Hash::default(),
                None,
                Some(ValidatorStakeInfo::default()),
                0,
                0,
            ),
        );

        // If the last leader slot has not seen propagation threshold, but
        // was the direct parent (implying consecutive leader slots), create
        // the block regardless
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));

        // If propagation threshold was achieved on parent, block should
        // also be created
        progress_map
            .get_mut(&3)
            .unwrap()
            .propagated_stats
            .is_propagated = true;
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));

        parent_slot = 2;

        // Even thought 2 is also a leader slot, because it's not consecutive
        // we still have to respect the propagation threshold check
        assert!(!ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));
    }

    #[test]
    fn test_purge_unconfirmed_duplicate_slot() {
        let (bank_forks, mut progress) = setup_forks();
        let mut descendants = bank_forks.read().unwrap().descendants();
        let mut ancestors = bank_forks.read().unwrap().ancestors();

        // Purging slot 5 should purge only slots 5 and its descendant 6
        ReplayStage::purge_unconfirmed_duplicate_slot(
            5,
            &mut ancestors,
            &mut descendants,
            &mut progress,
            &bank_forks,
        );
        for i in 5..=6 {
            assert!(bank_forks.read().unwrap().get(i).is_none());
            assert!(progress.get(&i).is_none());
        }
        for i in 0..=4 {
            assert!(bank_forks.read().unwrap().get(i).is_some());
            assert!(progress.get(&i).is_some());
        }

        // Purging slot 4 should purge only slot 4
        let mut descendants = bank_forks.read().unwrap().descendants();
        let mut ancestors = bank_forks.read().unwrap().ancestors();
        ReplayStage::purge_unconfirmed_duplicate_slot(
            4,
            &mut ancestors,
            &mut descendants,
            &mut progress,
            &bank_forks,
        );
        for i in 4..=6 {
            assert!(bank_forks.read().unwrap().get(i).is_none());
            assert!(progress.get(&i).is_none());
        }
        for i in 0..=3 {
            assert!(bank_forks.read().unwrap().get(i).is_some());
            assert!(progress.get(&i).is_some());
        }

        // Purging slot 1 should purge both forks 2 and 3
        let mut descendants = bank_forks.read().unwrap().descendants();
        let mut ancestors = bank_forks.read().unwrap().ancestors();
        ReplayStage::purge_unconfirmed_duplicate_slot(
            1,
            &mut ancestors,
            &mut descendants,
            &mut progress,
            &bank_forks,
        );
        for i in 1..=6 {
            assert!(bank_forks.read().unwrap().get(i).is_none());
            assert!(progress.get(&i).is_none());
        }
        assert!(bank_forks.read().unwrap().get(0).is_some());
        assert!(progress.get(&0).is_some());
    }

    #[test]
    fn test_purge_ancestors_descendants() {
        let (bank_forks, _) = setup_forks();

        // Purge branch rooted at slot 2
        let mut descendants = bank_forks.read().unwrap().descendants();
        let mut ancestors = bank_forks.read().unwrap().ancestors();
        let slot_2_descendants = descendants.get(&2).unwrap().clone();
        ReplayStage::purge_ancestors_descendants(
            2,
            &slot_2_descendants,
            &mut ancestors,
            &mut descendants,
        );

        // Result should be equivalent to removing slot from BankForks
        // and regeneratinig the `ancestor` `descendant` maps
        for d in slot_2_descendants {
            bank_forks.write().unwrap().remove(d);
        }
        bank_forks.write().unwrap().remove(2);
        assert!(check_map_eq(
            &ancestors,
            &bank_forks.read().unwrap().ancestors()
        ));
        assert!(check_map_eq(
            &descendants,
            &bank_forks.read().unwrap().descendants()
        ));

        // Try to purge the root
        bank_forks.write().unwrap().set_root(3, &None, None);
        let mut descendants = bank_forks.read().unwrap().descendants();
        let mut ancestors = bank_forks.read().unwrap().ancestors();
        let slot_3_descendants = descendants.get(&3).unwrap().clone();
        ReplayStage::purge_ancestors_descendants(
            3,
            &slot_3_descendants,
            &mut ancestors,
            &mut descendants,
        );

        assert!(ancestors.is_empty());
        // Only remaining keys should be ones < root
        for k in descendants.keys() {
            assert!(*k < 3);
        }
    }

    fn setup_forks() -> (RwLock<BankForks>, ProgressMap) {
        /*
            Build fork structure:
                 slot 0
                   |
                 slot 1
                 /    \
            slot 2    |
               |    slot 3
            slot 4    |
                    slot 5
                      |
                    slot 6
        */
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(4))) / (tr(3) / (tr(5) / (tr(6)))));

        let mut vote_simulator = VoteSimulator::new(1);
        vote_simulator.fill_bank_forks(forks, &HashMap::new());

        (vote_simulator.bank_forks, vote_simulator.progress)
    }

    fn check_map_eq<K: Eq + std::hash::Hash + std::fmt::Debug, T: PartialEq + std::fmt::Debug>(
        map1: &HashMap<K, T>,
        map2: &HashMap<K, T>,
    ) -> bool {
        map1.len() == map2.len() && map1.iter().all(|(k, v)| map2.get(k).unwrap() == v)
    }
}
