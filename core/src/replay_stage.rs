//! The `replay_stage` replays transactions broadcast by the leader.

use crate::cluster_info::ClusterInfo;
use crate::commitment::{
    AggregateCommitmentService, BlockCommitmentCache, CommitmentAggregationData,
};
use crate::consensus::{StakeLockout, Tower};
use crate::poh_recorder::PohRecorder;
use crate::result::{Error, Result};
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use solana_ledger::{
    bank_forks::BankForks,
    block_error::BlockError,
    blocktree::{Blocktree, BlocktreeError},
    blocktree_processor,
    entry::{Entry, EntrySlice},
    leader_schedule_cache::LeaderScheduleCache,
    snapshot_package::SnapshotPackageSender,
};
use solana_measure::measure::Measure;
use solana_metrics::{datapoint_warn, inc_new_counter_info};
use solana_runtime::bank::Bank;
use solana_sdk::{
    clock::Slot,
    hash::Hash,
    pubkey::Pubkey,
    signature::KeypairUtil,
    timing::{self, duration_as_ms},
    transaction::Transaction,
};
use solana_vote_api::vote_instruction;
use std::{
    collections::HashMap,
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender},
    sync::{Arc, Mutex, RwLock},
    thread::{self, Builder, JoinHandle},
    time::Duration,
    time::Instant,
};

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
    t_replay: JoinHandle<Result<()>>,
    commitment_service: AggregateCommitmentService,
}

struct ReplaySlotStats {
    // Per-slot elapsed time
    slot: Slot,
    fetch_entries_elapsed: u64,
    fetch_entries_fail_elapsed: u64,
    entry_verification_elapsed: u64,
    replay_elapsed: u64,
    replay_start: Instant,
}

impl ReplaySlotStats {
    pub fn new(slot: Slot) -> Self {
        Self {
            slot,
            fetch_entries_elapsed: 0,
            fetch_entries_fail_elapsed: 0,
            entry_verification_elapsed: 0,
            replay_elapsed: 0,
            replay_start: Instant::now(),
        }
    }

    pub fn report_stats(&self, total_entries: usize, total_shreds: usize) {
        datapoint_info!(
            "replay-slot-stats",
            ("slot", self.slot as i64, i64),
            ("fetch_entries_time", self.fetch_entries_elapsed as i64, i64),
            (
                "fetch_entries_fail_time",
                self.fetch_entries_fail_elapsed as i64,
                i64
            ),
            (
                "entry_verification_time",
                self.entry_verification_elapsed as i64,
                i64
            ),
            ("replay_time", self.replay_elapsed as i64, i64),
            (
                "replay_total_elapsed",
                self.replay_start.elapsed().as_micros() as i64,
                i64
            ),
            ("total_entries", total_entries as i64, i64),
            ("total_shreds", total_shreds as i64, i64),
        );
    }
}

struct ForkProgress {
    last_entry: Hash,
    num_shreds: usize,
    num_entries: usize,
    tick_hash_count: u64,
    started_ms: u64,
    is_dead: bool,
    stats: ReplaySlotStats,
}

impl ForkProgress {
    pub fn new(slot: Slot, last_entry: Hash) -> Self {
        Self {
            last_entry,
            num_shreds: 0,
            num_entries: 0,
            tick_hash_count: 0,
            started_ms: timing::timestamp(),
            is_dead: false,
            stats: ReplaySlotStats::new(slot),
        }
    }
}

impl ReplayStage {
    #[allow(
        clippy::new_ret_no_self,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn new<T>(
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
        voting_keypair: Option<&Arc<T>>,
        blocktree: Arc<Blocktree>,
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        exit: &Arc<AtomicBool>,
        ledger_signal_receiver: Receiver<bool>,
        subscriptions: &Arc<RpcSubscriptions>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        slot_full_senders: Vec<Sender<(u64, Pubkey)>>,
        snapshot_package_sender: Option<SnapshotPackageSender>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    ) -> (Self, Receiver<Vec<Arc<Bank>>>)
    where
        T: 'static + KeypairUtil + Send + Sync,
    {
        let (root_bank_sender, root_bank_receiver) = channel();
        trace!("replay stage");
        let exit_ = exit.clone();
        let subscriptions = subscriptions.clone();
        let bank_forks = bank_forks.clone();
        let poh_recorder = poh_recorder.clone();
        let my_pubkey = *my_pubkey;
        let mut tower = Tower::new(&my_pubkey, &vote_account, &bank_forks.read().unwrap());
        // Start the replay stage loop
        let leader_schedule_cache = leader_schedule_cache.clone();
        let vote_account = *vote_account;
        let voting_keypair = voting_keypair.cloned();

        let (lockouts_sender, commitment_service) =
            AggregateCommitmentService::new(exit, block_commitment_cache);

        let t_replay = Builder::new()
            .name("solana-replay-stage".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_.clone());
                let mut progress = HashMap::new();
                let mut current_leader = None;

                loop {
                    let now = Instant::now();
                    // Stop getting entries if we get exit signal
                    if exit_.load(Ordering::Relaxed) {
                        break;
                    }

                    Self::generate_new_bank_forks(
                        &blocktree,
                        &mut bank_forks.write().unwrap(),
                        &leader_schedule_cache,
                    );

                    let mut tpu_has_bank = poh_recorder.lock().unwrap().has_bank();

                    let did_complete_bank = Self::replay_active_banks(
                        &blocktree,
                        &bank_forks,
                        &my_pubkey,
                        &mut progress,
                        &slot_full_senders,
                    );

                    let ancestors = Arc::new(bank_forks.read().unwrap().ancestors());
                    let votable = Self::generate_votable_banks(
                        &ancestors,
                        &bank_forks,
                        &tower,
                        &mut progress,
                    );

                    if let Some((_, bank, _, total_staked)) = votable.into_iter().last() {
                        subscriptions.notify_subscribers(bank.slot(), &bank_forks);

                        if let Some(votable_leader) =
                            leader_schedule_cache.slot_leader_at(bank.slot(), Some(&bank))
                        {
                            Self::log_leader_change(
                                &my_pubkey,
                                bank.slot(),
                                &mut current_leader,
                                &votable_leader,
                            );
                        }

                        Self::handle_votable_bank(
                            &bank,
                            &bank_forks,
                            &mut tower,
                            &mut progress,
                            &vote_account,
                            &voting_keypair,
                            &cluster_info,
                            &blocktree,
                            &leader_schedule_cache,
                            &root_bank_sender,
                            total_staked,
                            &lockouts_sender,
                            &snapshot_package_sender,
                        )?;

                        Self::reset_poh_recorder(
                            &my_pubkey,
                            &blocktree,
                            &bank,
                            &poh_recorder,
                            &leader_schedule_cache,
                        );
                        tpu_has_bank = false;
                    }

                    if !tpu_has_bank {
                        Self::maybe_start_leader(
                            &my_pubkey,
                            &bank_forks,
                            &poh_recorder,
                            &leader_schedule_cache,
                        );

                        if let Some(bank) = poh_recorder.lock().unwrap().bank() {
                            Self::log_leader_change(
                                &my_pubkey,
                                bank.slot(),
                                &mut current_leader,
                                &my_pubkey,
                            );
                        }
                    }

                    inc_new_counter_info!(
                        "replicate_stage-duration",
                        duration_as_ms(&now.elapsed()) as usize
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
                        Ok(_) => trace!("blocktree signal"),
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

    fn maybe_start_leader(
        my_pubkey: &Pubkey,
        bank_forks: &Arc<RwLock<BankForks>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
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

            datapoint_debug!(
                "replay_stage-new_leader",
                ("slot", poh_slot, i64),
                ("leader", next_leader.to_string(), String),
            );

            let tpu_bank = bank_forks
                .write()
                .unwrap()
                .insert(Bank::new_from_parent(&parent, my_pubkey, poh_slot));

            poh_recorder.lock().unwrap().set_bank(&tpu_bank);
        } else {
            error!("{} No next leader found", my_pubkey);
        }
    }

    // Returns Some(result) if the `result` is a fatal error, which is an error that will cause a
    // bank to be marked as dead/corrupted
    fn is_replay_result_fatal(result: &Result<()>) -> bool {
        match result {
            Err(Error::TransactionError(e)) => {
                // Transactions withand transaction errors mean this fork is bogus
                let tx_error = Err(e.clone());
                !Bank::can_commit(&tx_error)
            }
            Err(Error::BlockError(_)) => true,
            Err(Error::BlocktreeError(BlocktreeError::InvalidShredData(_))) => true,
            _ => false,
        }
    }

    // Returns the replay result and the number of replayed transactions
    fn replay_blocktree_into_bank(
        bank: &Arc<Bank>,
        blocktree: &Blocktree,
        progress: &mut HashMap<u64, ForkProgress>,
    ) -> (Result<()>, usize) {
        let mut tx_count = 0;
        let bank_progress = &mut progress
            .entry(bank.slot())
            .or_insert_with(|| ForkProgress::new(bank.slot(), bank.last_blockhash()));
        let now = Instant::now();
        let load_result =
            Self::load_blocktree_entries_with_shred_count(bank, blocktree, bank_progress);
        let fetch_entries_elapsed = now.elapsed().as_micros();
        if load_result.is_err() {
            bank_progress.stats.fetch_entries_fail_elapsed += fetch_entries_elapsed as u64;
        } else {
            bank_progress.stats.fetch_entries_elapsed += fetch_entries_elapsed as u64;
        }

        let replay_result = load_result.and_then(|(entries, num_shreds)| {
            trace!(
                "Fetch entries for slot {}, {:?} entries, num shreds {:?}",
                bank.slot(),
                entries.len(),
                num_shreds
            );
            tx_count += entries.iter().map(|e| e.transactions.len()).sum::<usize>();
            Self::replay_entries_into_bank(bank, entries, bank_progress, num_shreds)
        });

        if Self::is_replay_result_fatal(&replay_result) {
            warn!(
                "Fatal replay result in slot: {}, result: {:?}",
                bank.slot(),
                replay_result
            );
            datapoint_warn!("replay-stage-mark_dead_slot", ("slot", bank.slot(), i64),);
            Self::mark_dead_slot(bank.slot(), blocktree, progress);
        }

        (replay_result, tx_count)
    }

    fn mark_dead_slot(
        slot: Slot,
        blocktree: &Blocktree,
        progress: &mut HashMap<u64, ForkProgress>,
    ) {
        // Remove from progress map so we no longer try to replay this bank
        let mut progress_entry = progress
            .get_mut(&slot)
            .expect("Progress entry must exist after call to replay_entries_into_bank()");
        progress_entry.is_dead = true;
        blocktree
            .set_dead_slot(slot)
            .expect("Failed to mark slot as dead in blocktree");
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_votable_bank<T>(
        bank: &Arc<Bank>,
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &mut Tower,
        progress: &mut HashMap<u64, ForkProgress>,
        vote_account: &Pubkey,
        voting_keypair: &Option<Arc<T>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        blocktree: &Arc<Blocktree>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        root_bank_sender: &Sender<Vec<Arc<Bank>>>,
        total_staked: u64,
        lockouts_sender: &Sender<CommitmentAggregationData>,
        snapshot_package_sender: &Option<SnapshotPackageSender>,
    ) -> Result<()>
    where
        T: 'static + KeypairUtil + Send + Sync,
    {
        trace!("handle votable bank {}", bank.slot());
        let (vote, tower_index) = tower.new_vote_from_bank(bank, vote_account);
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
            // Call leader schedule_cache.set_root() before blocktree.set_root() because
            // bank_forks.root is consumed by repair_service to update gossip, so we don't want to
            // get blobs for repair on gossip before we update leader schedule, otherwise they may
            // get dropped.
            leader_schedule_cache.set_root(rooted_banks.last().unwrap());
            blocktree
                .set_roots(&rooted_slots)
                .expect("Ledger set roots failed");
            bank_forks
                .write()
                .unwrap()
                .set_root(new_root, snapshot_package_sender);
            Self::handle_new_root(&bank_forks, progress);
            trace!("new root {}", new_root);
            if let Err(e) = root_bank_sender.send(rooted_banks) {
                trace!("root_bank_sender failed: {:?}", e);
                return Err(e.into());
            }
        }
        Self::update_commitment_cache(bank.clone(), total_staked, lockouts_sender);

        if let Some(ref voting_keypair) = voting_keypair {
            let node_keypair = cluster_info.read().unwrap().keypair.clone();

            // Send our last few votes along with the new one
            let vote_ix =
                vote_instruction::vote(&vote_account, &voting_keypair.pubkey(), tower.last_vote());

            let mut vote_tx =
                Transaction::new_with_payer(vec![vote_ix], Some(&node_keypair.pubkey()));

            let blockhash = bank.last_blockhash();
            vote_tx.partial_sign(&[node_keypair.as_ref()], blockhash);
            vote_tx.partial_sign(&[voting_keypair.as_ref()], blockhash);
            cluster_info
                .write()
                .unwrap()
                .push_vote(tower_index, vote_tx);
        }
        Ok(())
    }

    fn update_commitment_cache(
        bank: Arc<Bank>,
        total_staked: u64,
        lockouts_sender: &Sender<CommitmentAggregationData>,
    ) {
        if let Err(e) = lockouts_sender.send(CommitmentAggregationData::new(bank, total_staked)) {
            trace!("lockouts_sender failed: {:?}", e);
        }
    }

    fn reset_poh_recorder(
        my_pubkey: &Pubkey,
        blocktree: &Blocktree,
        bank: &Arc<Bank>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
    ) {
        let next_leader_slot =
            leader_schedule_cache.next_leader_slot(&my_pubkey, bank.slot(), &bank, Some(blocktree));
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
            "{} voted and reset PoH at tick height {}. {}",
            my_pubkey,
            bank.tick_height(),
            next_leader_msg,
        );
    }

    fn replay_active_banks(
        blocktree: &Arc<Blocktree>,
        bank_forks: &Arc<RwLock<BankForks>>,
        my_pubkey: &Pubkey,
        progress: &mut HashMap<u64, ForkProgress>,
        slot_full_senders: &[Sender<(u64, Pubkey)>],
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
            if bank.collector_id() != my_pubkey {
                let (replay_result, replay_tx_count) =
                    Self::replay_blocktree_into_bank(&bank, &blocktree, progress);
                tx_count += replay_tx_count;
                if Self::is_replay_result_fatal(&replay_result) {
                    trace!("replay_result_fatal slot {}", bank_slot);
                    // If the bank was corrupted, don't try to run the below logic to check if the
                    // bank is completed
                    continue;
                }
            }
            assert_eq!(*bank_slot, bank.slot());
            if bank.tick_height() == bank.max_tick_height() {
                if let Some(bank_progress) = &mut progress.get(&bank.slot()) {
                    bank_progress
                        .stats
                        .report_stats(bank_progress.num_entries, bank_progress.num_shreds);
                }
                did_complete_bank = true;
                Self::process_completed_bank(my_pubkey, bank, slot_full_senders);
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

    #[allow(clippy::type_complexity)]
    fn generate_votable_banks(
        ancestors: &HashMap<u64, HashSet<u64>>,
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &Tower,
        progress: &mut HashMap<u64, ForkProgress>,
    ) -> Vec<(u128, Arc<Bank>, HashMap<u64, StakeLockout>, u64)> {
        let tower_start = Instant::now();
        let frozen_banks = bank_forks.read().unwrap().frozen_banks();
        trace!("frozen_banks {}", frozen_banks.len());
        let mut votable: Vec<(u128, Arc<Bank>, HashMap<u64, StakeLockout>, u64)> = frozen_banks
            .values()
            .filter(|b| {
                let is_votable = b.is_votable();
                trace!("bank is votable: {} {}", b.slot(), is_votable);
                is_votable
            })
            .filter(|b| {
                let has_voted = tower.has_voted(b.slot());
                trace!("bank has_voted: {} {}", b.slot(), has_voted);
                !has_voted
            })
            .filter(|b| {
                let is_locked_out = tower.is_locked_out(b.slot(), &ancestors);
                trace!("bank is is_locked_out: {} {}", b.slot(), is_locked_out);
                !is_locked_out
            })
            .map(|bank| {
                (
                    bank,
                    tower.collect_vote_lockouts(
                        bank.slot(),
                        bank.vote_accounts().into_iter(),
                        &ancestors,
                    ),
                )
            })
            .filter(|(b, (stake_lockouts, total_staked))| {
                let vote_threshold =
                    tower.check_vote_stake_threshold(b.slot(), &stake_lockouts, *total_staked);
                Self::confirm_forks(tower, &stake_lockouts, *total_staked, progress, bank_forks);
                debug!("bank vote_threshold: {} {}", b.slot(), vote_threshold);
                vote_threshold
            })
            .map(|(b, (stake_lockouts, total_staked))| {
                (
                    tower.calculate_weight(&stake_lockouts),
                    b.clone(),
                    stake_lockouts,
                    total_staked,
                )
            })
            .collect();

        votable.sort_by_key(|b| b.0);
        let ms = timing::duration_as_ms(&tower_start.elapsed());

        trace!("votable_banks {}", votable.len());
        if !votable.is_empty() {
            let weights: Vec<u128> = votable.iter().map(|x| x.0).collect();
            info!(
                "@{:?} tower duration: {:?} len: {} weights: {:?}",
                timing::timestamp(),
                ms,
                votable.len(),
                weights
            );
        }
        inc_new_counter_info!("replay_stage-tower_duration", ms as usize);

        votable
    }

    fn confirm_forks(
        tower: &Tower,
        stake_lockouts: &HashMap<u64, StakeLockout>,
        total_staked: u64,
        progress: &mut HashMap<u64, ForkProgress>,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        progress.retain(|slot, prog| {
            let duration = timing::timestamp() - prog.started_ms;
            if tower.is_slot_confirmed(*slot, stake_lockouts, total_staked)
                && bank_forks
                    .read()
                    .unwrap()
                    .get(*slot)
                    .map(|s| s.is_frozen())
                    .unwrap_or(true)
            {
                info!("validator fork confirmed {} {}ms", *slot, duration);
                datapoint_warn!("validator-confirmation", ("duration_ms", duration, i64));
                false
            } else {
                debug!(
                    "validator fork not confirmed {} {}ms {:?}",
                    *slot,
                    duration,
                    stake_lockouts.get(slot)
                );
                true
            }
        });
    }

    fn load_blocktree_entries_with_shred_count(
        bank: &Bank,
        blocktree: &Blocktree,
        bank_progress: &mut ForkProgress,
    ) -> Result<(Vec<Entry>, usize)> {
        let bank_slot = bank.slot();
        let entries_and_shred_count = blocktree
            .get_slot_entries_with_shred_count(bank_slot, bank_progress.num_shreds as u64)?;
        Ok(entries_and_shred_count)
    }

    fn replay_entries_into_bank(
        bank: &Arc<Bank>,
        entries: Vec<Entry>,
        bank_progress: &mut ForkProgress,
        num: usize,
    ) -> Result<()> {
        let result = Self::verify_and_process_entries(
            &bank,
            &entries,
            bank_progress.num_shreds,
            bank_progress,
        );
        bank_progress.num_shreds += num;
        bank_progress.num_entries += entries.len();
        if let Some(last_entry) = entries.last() {
            bank_progress.last_entry = last_entry.hash;
        }

        result
    }

    fn verify_ticks(
        bank: &Arc<Bank>,
        entries: &[Entry],
        tick_hash_count: &mut u64,
    ) -> std::result::Result<(), BlockError> {
        if entries.is_empty() {
            return Ok(());
        }

        let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
        if !entries.verify_tick_hash_count(tick_hash_count, hashes_per_tick) {
            return Err(BlockError::InvalidTickHashCount);
        }

        let next_bank_tick_height = bank.tick_height() + entries.tick_count();
        let max_bank_tick_height = bank.max_tick_height();
        if next_bank_tick_height > max_bank_tick_height {
            return Err(BlockError::InvalidTickCount);
        }

        let has_trailing_entry = !entries.last().unwrap().is_tick();
        if next_bank_tick_height == max_bank_tick_height && has_trailing_entry {
            return Err(BlockError::TrailingEntry);
        }

        Ok(())
    }

    fn verify_and_process_entries(
        bank: &Arc<Bank>,
        entries: &[Entry],
        shred_index: usize,
        bank_progress: &mut ForkProgress,
    ) -> Result<()> {
        let last_entry = &bank_progress.last_entry;
        let tick_hash_count = &mut bank_progress.tick_hash_count;
        let handle_block_error = move |block_error: BlockError| -> Result<()> {
            warn!(
                "{:#?}, slot: {}, entry len: {}, tick_height: {}, last entry: {}, last_blockhash: {}, shred_index: {}",
                block_error,
                bank.slot(),
                entries.len(),
                bank.tick_height(),
                last_entry,
                bank.last_blockhash(),
                shred_index
            );

            datapoint_error!(
                "replay-stage-entry_verification_failure",
                ("slot", bank.slot(), i64),
                ("last_entry", last_entry.to_string(), String),
            );

            Err(Error::BlockError(block_error))
        };

        if let Err(block_error) = Self::verify_ticks(bank, entries, tick_hash_count) {
            return handle_block_error(block_error);
        }

        datapoint_debug!("verify-batch-size", ("size", entries.len() as i64, i64));
        let mut verify_total = Measure::start("verify_and_process_entries");
        let mut entry_state = entries.start_verify(last_entry);

        let mut replay_elapsed = Measure::start("replay_elapsed");
        let res = blocktree_processor::process_entries(bank, entries, true);
        replay_elapsed.stop();
        bank_progress.stats.replay_elapsed += replay_elapsed.as_us();

        if !entry_state.finish_verify(entries) {
            return handle_block_error(BlockError::InvalidEntryHash);
        }

        verify_total.stop();
        bank_progress.stats.entry_verification_elapsed =
            verify_total.as_us() - replay_elapsed.as_us();

        res?;
        Ok(())
    }

    fn handle_new_root(
        bank_forks: &Arc<RwLock<BankForks>>,
        progress: &mut HashMap<u64, ForkProgress>,
    ) {
        let r_bank_forks = bank_forks.read().unwrap();
        progress.retain(|k, _| r_bank_forks.get(*k).is_some());
    }

    fn process_completed_bank(
        my_pubkey: &Pubkey,
        bank: Arc<Bank>,
        slot_full_senders: &[Sender<(u64, Pubkey)>],
    ) {
        bank.freeze();
        info!("bank frozen {}", bank.slot());
        slot_full_senders.iter().for_each(|sender| {
            if let Err(e) = sender.send((bank.slot(), *bank.collector_id())) {
                trace!("{} slot_full alert failed: {:?}", my_pubkey, e);
            }
        });
    }

    fn generate_new_bank_forks(
        blocktree: &Blocktree,
        forks: &mut BankForks,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
    ) {
        // Find the next slot that chains to the old slot
        let frozen_banks = forks.frozen_banks();
        let frozen_bank_slots: Vec<u64> = frozen_banks.keys().cloned().collect();
        let next_slots = blocktree
            .get_slots_since(&frozen_bank_slots)
            .expect("Db error");
        // Filter out what we've already seen
        trace!("generate new forks {:?}", {
            let mut next_slots = next_slots.iter().collect::<Vec<_>>();
            next_slots.sort();
            next_slots
        });
        for (parent_id, children) in next_slots {
            let parent_bank = frozen_banks
                .get(&parent_id)
                .expect("missing parent in bank forks")
                .clone();
            for child_id in children {
                if forks.get(child_id).is_some() {
                    trace!("child already active or frozen {}", child_id);
                    continue;
                }
                let leader = leader_schedule_cache
                    .slot_leader_at(child_id, Some(&parent_bank))
                    .unwrap();
                info!("new fork:{} parent:{}", child_id, parent_id);
                forks.insert(Bank::new_from_parent(&parent_bank, &leader, child_id));
            }
        }
    }
}

impl Service for ReplayStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.commitment_service.join()?;
        self.t_replay.join().map(|_| ())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::commitment::BlockCommitment;
    use crate::genesis_utils::{create_genesis_block, create_genesis_block_with_leader};
    use crate::replay_stage::ReplayStage;
    use solana_ledger::blocktree::make_slot_entries;
    use solana_ledger::blocktree::{entries_to_test_shreds, get_tmp_ledger_path, BlocktreeError};
    use solana_ledger::entry;
    use solana_ledger::shred::{
        CodingShredHeader, DataShredHeader, Shred, ShredCommonHeader, DATA_COMPLETE_SHRED,
        SIZE_OF_COMMON_SHRED_HEADER, SIZE_OF_DATA_SHRED_HEADER, SIZE_OF_DATA_SHRED_PAYLOAD,
    };
    use solana_runtime::genesis_utils::GenesisBlockInfo;
    use solana_sdk::hash::{hash, Hash};
    use solana_sdk::packet::PACKET_DATA_SIZE;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use solana_sdk::transaction::TransactionError;
    use solana_vote_api::vote_state::VoteState;
    use std::fs::remove_dir_all;
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_child_slots_of_same_parent() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );

            let genesis_block = create_genesis_block(10_000).genesis_block;
            let bank0 = Bank::new(&genesis_block);
            let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank0));
            let mut bank_forks = BankForks::new(0, bank0);
            bank_forks.working_bank().freeze();

            // Insert blob for slot 1, generate new forks, check result
            let (shreds, _) = make_slot_entries(1, 0, 8);
            blocktree.insert_shreds(shreds, None).unwrap();
            assert!(bank_forks.get(1).is_none());
            ReplayStage::generate_new_bank_forks(
                &blocktree,
                &mut bank_forks,
                &leader_schedule_cache,
            );
            assert!(bank_forks.get(1).is_some());

            // Insert blob for slot 3, generate new forks, check result
            let (shreds, _) = make_slot_entries(2, 0, 8);
            blocktree.insert_shreds(shreds, None).unwrap();
            assert!(bank_forks.get(2).is_none());
            ReplayStage::generate_new_bank_forks(
                &blocktree,
                &mut bank_forks,
                &leader_schedule_cache,
            );
            assert!(bank_forks.get(1).is_some());
            assert!(bank_forks.get(2).is_some());
        }

        let _ignored = remove_dir_all(&ledger_path);
    }

    #[test]
    fn test_handle_new_root() {
        let genesis_block = create_genesis_block(10_000).genesis_block;
        let bank0 = Bank::new(&genesis_block);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank0)));
        let mut progress = HashMap::new();
        progress.insert(5, ForkProgress::new(0, Hash::default()));
        ReplayStage::handle_new_root(&bank_forks, &mut progress);
        assert!(progress.is_empty());
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
            entries_to_test_shreds(vec![entry], slot, slot.saturating_sub(1), false)
        });

        assert_matches!(
            res,
            Err(Error::TransactionError(TransactionError::AccountNotFound))
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
            entries_to_test_shreds(vec![entry], slot, slot.saturating_sub(1), false)
        });

        if let Err(Error::BlockError(block_error)) = res {
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
            )
        });

        if let Err(Error::BlockError(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidTickHashCount);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_dead_fork_invalid_slot_tick_count() {
        let res = check_dead_fork(|_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            entries_to_test_shreds(
                entry::create_ticks(bank.ticks_per_slot() + 1, hashes_per_tick, blockhash),
                slot,
                slot.saturating_sub(1),
                false,
            )
        });

        if let Err(Error::BlockError(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidTickCount);
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
            entries_to_test_shreds(entries, slot, slot.saturating_sub(1), false)
        });

        if let Err(Error::BlockError(block_error)) = res {
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
            data_header.flags = DATA_COMPLETE_SHRED;
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
            Err(Error::BlocktreeError(BlocktreeError::InvalidShredData(_)))
        );
    }

    // Given a blob and a fatal expected error, check that replaying that blob causes causes the fork to be
    // marked as dead. Returns the error for caller to verify.
    fn check_dead_fork<F>(shred_to_insert: F) -> Result<()>
    where
        F: Fn(&Keypair, Arc<Bank>) -> Vec<Shred>,
    {
        let ledger_path = get_tmp_ledger_path!();
        let res = {
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );
            let GenesisBlockInfo {
                mut genesis_block,
                mint_keypair,
                ..
            } = create_genesis_block(1000);
            genesis_block.poh_config.hashes_per_tick = Some(2);
            let bank0 = Arc::new(Bank::new(&genesis_block));
            let mut progress = HashMap::new();
            let last_blockhash = bank0.last_blockhash();
            progress.insert(bank0.slot(), ForkProgress::new(0, last_blockhash));
            let shreds = shred_to_insert(&mint_keypair, bank0.clone());
            blocktree.insert_shreds(shreds, None).unwrap();
            let (res, _tx_count) =
                ReplayStage::replay_blocktree_into_bank(&bank0, &blocktree, &mut progress);

            // Check that the erroring bank was marked as dead in the progress map
            assert!(progress
                .get(&bank0.slot())
                .map(|b| b.is_dead)
                .unwrap_or(false));

            // Check that the erroring bank was marked as dead in blocktree
            assert!(blocktree.is_dead(bank0.slot()));
            res
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
            vote_state.to(&mut leader_vote_account).unwrap();
            bank.store_account(&pubkey, &leader_vote_account);
        }

        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let (lockouts_sender, _) = AggregateCommitmentService::new(
            &Arc::new(AtomicBool::new(false)),
            block_commitment_cache.clone(),
        );

        let leader_pubkey = Pubkey::new_rand();
        let leader_lamports = 3;
        let genesis_block_info =
            create_genesis_block_with_leader(50, &leader_pubkey, leader_lamports);
        let mut genesis_block = genesis_block_info.genesis_block;
        let leader_voting_pubkey = genesis_block_info.voting_keypair.pubkey();
        genesis_block.epoch_schedule.warmup = false;
        genesis_block.ticks_per_slot = 4;
        let bank0 = Bank::new(&genesis_block);
        for _ in 0..genesis_block.ticks_per_slot {
            bank0.register_tick(&Hash::default());
        }
        bank0.freeze();
        let arc_bank0 = Arc::new(bank0);
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(
            &[arc_bank0.clone()],
            vec![0],
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
        let _res = bank1.transfer(10, &genesis_block_info.mint_keypair, &Pubkey::new_rand());
        for _ in 0..genesis_block.ticks_per_slot {
            bank1.register_tick(&Hash::default());
        }
        bank1.freeze();
        bank_forks.write().unwrap().insert(bank1);
        let arc_bank1 = bank_forks.read().unwrap().get(1).unwrap().clone();
        leader_vote(&arc_bank1, &leader_voting_pubkey);
        ReplayStage::update_commitment_cache(arc_bank1.clone(), leader_lamports, &lockouts_sender);

        let bank2 = Bank::new_from_parent(&arc_bank1, &Pubkey::default(), arc_bank1.slot() + 1);
        let _res = bank2.transfer(10, &genesis_block_info.mint_keypair, &Pubkey::new_rand());
        for _ in 0..genesis_block.ticks_per_slot {
            bank2.register_tick(&Hash::default());
        }
        bank2.freeze();
        bank_forks.write().unwrap().insert(bank2);
        let arc_bank2 = bank_forks.read().unwrap().get(2).unwrap().clone();
        leader_vote(&arc_bank2, &leader_voting_pubkey);
        ReplayStage::update_commitment_cache(arc_bank2.clone(), leader_lamports, &lockouts_sender);
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
}
