//! The `replay_stage` replays transactions broadcast by the leader.

use crate::{
    cluster_info::ClusterInfo,
    commitment::{AggregateCommitmentService, BlockCommitmentCache, CommitmentAggregationData},
    consensus::{StakeLockout, Tower},
    poh_recorder::PohRecorder,
    result::{Error, Result},
    rpc_subscriptions::RpcSubscriptions,
    thread_mem_usage,
};
use solana_ledger::entry::EntryVerificationStatus;
use solana_ledger::{
    bank_forks::BankForks,
    block_error::BlockError,
    blocktree::{Blocktree, BlocktreeError},
    blocktree_processor::{self, TransactionStatusSender},
    entry::{Entry, EntrySlice, VerifyRecyclers},
    leader_schedule_cache::LeaderScheduleCache,
    snapshot_package::SnapshotPackageSender,
};
use solana_measure::measure::Measure;
use solana_metrics::inc_new_counter_info;
use solana_runtime::bank::Bank;
use solana_sdk::{
    clock::Slot,
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    timing::{self, duration_as_ms},
    transaction::Transaction,
};
use solana_vote_program::vote_instruction;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex, RwLock,
    },
    thread::{self, Builder, JoinHandle},
    time::{Duration, Instant},
};

pub const MAX_ENTRY_RECV_PER_ITER: usize = 512;

type VoteAndPoHBank = Option<(Arc<Bank>, ForkStats)>;

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
pub struct ReplayStageConfig {
    pub my_pubkey: Pubkey,
    pub vote_account: Pubkey,
    pub voting_keypair: Option<Arc<Keypair>>,
    pub exit: Arc<AtomicBool>,
    pub subscriptions: Arc<RpcSubscriptions>,
    pub leader_schedule_cache: Arc<LeaderScheduleCache>,
    pub slot_full_senders: Vec<Sender<(u64, Pubkey)>>,
    pub latest_root_senders: Vec<Sender<Slot>>,
    pub snapshot_package_sender: Option<SnapshotPackageSender>,
    pub block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    pub transaction_status_sender: Option<TransactionStatusSender>,
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

#[derive(Debug, Clone, Default)]
struct ForkStats {
    weight: u128,
    fork_weight: u128,
    total_staked: u64,
    slot: Slot,
    block_height: u64,
    has_voted: bool,
    is_recent: bool,
    is_empty: bool,
    vote_threshold: bool,
    is_locked_out: bool,
    stake_lockouts: HashMap<u64, StakeLockout>,
    computed: bool,
    confirmation_reported: bool,
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
    fork_stats: ForkStats,
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
            fork_stats: ForkStats::default(),
        }
    }
}

impl ReplayStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        config: ReplayStageConfig,
        blocktree: Arc<Blocktree>,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        ledger_signal_receiver: Receiver<bool>,
        poh_recorder: Arc<Mutex<PohRecorder>>,
    ) -> (Self, Receiver<Vec<Arc<Bank>>>) {
        let ReplayStageConfig {
            my_pubkey,
            vote_account,
            voting_keypair,
            exit,
            subscriptions,
            leader_schedule_cache,
            slot_full_senders,
            latest_root_senders,
            snapshot_package_sender,
            block_commitment_cache,
            transaction_status_sender,
        } = config;

        let (root_bank_sender, root_bank_receiver) = channel();
        trace!("replay stage");
        let mut tower = Tower::new(&my_pubkey, &vote_account, &bank_forks.read().unwrap());

        // Start the replay stage loop

        let (lockouts_sender, commitment_service) =
            AggregateCommitmentService::new(&exit, block_commitment_cache);

        let t_replay = Builder::new()
            .name("solana-replay-stage".to_string())
            .spawn(move || {
                let verify_recyclers = VerifyRecyclers::default();
                let _exit = Finalizer::new(exit.clone());
                let mut progress = HashMap::new();
                // Initialize progress map with any root banks
                for bank in bank_forks.read().unwrap().frozen_banks().values() {
                    progress.insert(
                        bank.slot(),
                        ForkProgress::new(bank.slot(), bank.last_blockhash()),
                    );
                }
                let mut current_leader = None;
                let mut last_reset = Hash::default();
                let mut partition = false;
                loop {
                    let allocated = thread_mem_usage::Allocatedp::default();

                    thread_mem_usage::datapoint("solana-replay-stage");
                    let now = Instant::now();
                    // Stop getting entries if we get exit signal
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    let start = allocated.get();
                    Self::generate_new_bank_forks(
                        &blocktree,
                        &bank_forks,
                        &leader_schedule_cache,
                        &subscriptions,
                    );
                    datapoint_debug!(
                        "replay_stage-memory",
                        (
                            "generate_new_bank_forks",
                            (allocated.get() - start) as i64,
                            i64
                        ),
                    );

                    let mut tpu_has_bank = poh_recorder.lock().unwrap().has_bank();

                    let start = allocated.get();
                    let did_complete_bank = Self::replay_active_banks(
                        &blocktree,
                        &bank_forks,
                        &my_pubkey,
                        &mut progress,
                        &slot_full_senders,
                        transaction_status_sender.clone(),
                        &verify_recyclers,
                    );
                    datapoint_debug!(
                        "replay_stage-memory",
                        ("replay_active_banks", (allocated.get() - start) as i64, i64),
                    );

                    let ancestors = Arc::new(bank_forks.read().unwrap().ancestors());
                    loop {
                        let start = allocated.get();
                        let vote_bank = Self::select_fork(
                            &my_pubkey,
                            &ancestors,
                            &bank_forks,
                            &tower,
                            &mut progress,
                        );
                        datapoint_debug!(
                            "replay_stage-memory",
                            ("select_fork", (allocated.get() - start) as i64, i64),
                        );
                        if vote_bank.is_none() {
                            break;
                        }
                        let (bank, stats) = vote_bank.unwrap();
                        let mut done = false;
                        let mut vote_bank_slot = None;
                        let start = allocated.get();
                        if !stats.is_locked_out && stats.vote_threshold {
                            info!("voting: {} {}", bank.slot(), stats.fork_weight);
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
                            vote_bank_slot = Some(bank.slot());
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
                                stats.total_staked,
                                &lockouts_sender,
                                &snapshot_package_sender,
                                &latest_root_senders,
                            )?;
                        }
                        datapoint_debug!(
                            "replay_stage-memory",
                            ("votable_bank", (allocated.get() - start) as i64, i64),
                        );
                        let start = allocated.get();
                        if last_reset != bank.last_blockhash() {
                            Self::reset_poh_recorder(
                                &my_pubkey,
                                &blocktree,
                                &bank,
                                &poh_recorder,
                                &leader_schedule_cache,
                            );
                            last_reset = bank.last_blockhash();
                            tpu_has_bank = false;
                            info!(
                                "vote bank: {:?} reset bank: {}",
                                vote_bank_slot,
                                bank.slot()
                            );
                            if !partition && vote_bank_slot != Some(bank.slot()) {
                                warn!(
                                    "PARTITION DETECTED waiting to join fork: {} last vote: {:?}",
                                    bank.slot(),
                                    tower.last_vote()
                                );
                                inc_new_counter_info!("replay_stage-partition_detected", 1);
                                datapoint_info!(
                                    "replay_stage-partition",
                                    ("slot", bank.slot() as i64, i64)
                                );
                                partition = true;
                            } else if partition && vote_bank_slot == Some(bank.slot()) {
                                warn!(
                                    "PARTITION resolved fork: {} last vote: {:?}",
                                    bank.slot(),
                                    tower.last_vote()
                                );
                                partition = false;
                                inc_new_counter_info!("replay_stage-partition_resolved", 1);
                            }
                        } else {
                            done = true;
                        }
                        datapoint_debug!(
                            "replay_stage-memory",
                            ("reset_bank", (allocated.get() - start) as i64, i64),
                        );
                        if done {
                            break;
                        }
                    }

                    let start = allocated.get();
                    if !tpu_has_bank {
                        Self::maybe_start_leader(
                            &my_pubkey,
                            &bank_forks,
                            &poh_recorder,
                            &leader_schedule_cache,
                            &subscriptions,
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
                    datapoint_debug!(
                        "replay_stage-memory",
                        ("start_leader", (allocated.get() - start) as i64, i64),
                    );
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
        subscriptions: &Arc<RpcSubscriptions>,
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

            let root_slot = bank_forks.read().unwrap().root();
            info!(
                "new fork:{} parent:{} (leader) root:{}",
                poh_slot, parent_slot, root_slot
            );
            subscriptions.notify_slot(poh_slot, parent_slot, root_slot);
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
            Err(Error::BlocktreeError(BlocktreeError::DeadSlot)) => true,
            _ => false,
        }
    }

    // Returns the replay result and the number of replayed transactions
    fn replay_blocktree_into_bank(
        bank: &Arc<Bank>,
        blocktree: &Blocktree,
        bank_progress: &mut ForkProgress,
        transaction_status_sender: Option<TransactionStatusSender>,
        verify_recyclers: &VerifyRecyclers,
    ) -> (Result<()>, usize) {
        let mut tx_count = 0;
        let now = Instant::now();
        let load_result =
            Self::load_blocktree_entries_with_shred_info(bank, blocktree, bank_progress);
        let fetch_entries_elapsed = now.elapsed().as_micros();
        if load_result.is_err() {
            bank_progress.stats.fetch_entries_fail_elapsed += fetch_entries_elapsed as u64;
        } else {
            bank_progress.stats.fetch_entries_elapsed += fetch_entries_elapsed as u64;
        }

        let replay_result = load_result.and_then(|(entries, num_shreds, slot_full)| {
            trace!(
                "Fetch entries for slot {}, {:?} entries, num shreds {}, slot_full: {}",
                bank.slot(),
                entries.len(),
                num_shreds,
                slot_full,
            );
            tx_count += entries.iter().map(|e| e.transactions.len()).sum::<usize>();
            Self::replay_entries_into_bank(
                bank,
                bank_progress,
                entries,
                num_shreds,
                slot_full,
                transaction_status_sender,
                verify_recyclers,
            )
        });

        if Self::is_replay_result_fatal(&replay_result) {
            warn!(
                "Fatal replay result in slot: {}, result: {:?}",
                bank.slot(),
                replay_result
            );
            datapoint_error!(
                "replay-stage-mark_dead_slot",
                ("error", format!("error: {:?}", replay_result), String),
                ("slot", bank.slot(), i64)
            );
            Self::mark_dead_slot(bank.slot(), blocktree, bank_progress);
        }

        (replay_result, tx_count)
    }

    fn mark_dead_slot(slot: Slot, blocktree: &Blocktree, bank_progress: &mut ForkProgress) {
        bank_progress.is_dead = true;
        blocktree
            .set_dead_slot(slot)
            .expect("Failed to mark slot as dead in blocktree");
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_votable_bank(
        bank: &Arc<Bank>,
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &mut Tower,
        progress: &mut HashMap<u64, ForkProgress>,
        vote_account: &Pubkey,
        voting_keypair: &Option<Arc<Keypair>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        blocktree: &Arc<Blocktree>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        root_bank_sender: &Sender<Vec<Arc<Bank>>>,
        total_staked: u64,
        lockouts_sender: &Sender<CommitmentAggregationData>,
        snapshot_package_sender: &Option<SnapshotPackageSender>,
        latest_root_senders: &[Sender<Slot>],
    ) -> Result<()> {
        if bank.is_empty() {
            inc_new_counter_info!("replay_stage-voted_empty_bank", 1);
        }
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
            // get shreds for repair on gossip before we update leader schedule, otherwise they may
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
            latest_root_senders.iter().for_each(|s| {
                if let Err(e) = s.send(new_root) {
                    trace!("latest root send failed: {:?}", e);
                }
            });
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
            let vote_ix = vote_instruction::vote(
                &vote_account,
                &voting_keypair.pubkey(),
                tower.last_vote_and_timestamp(),
            );

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
            "{} reset PoH to tick {} (within slot {}). {}",
            my_pubkey,
            bank.tick_height(),
            bank.slot(),
            next_leader_msg,
        );
    }

    fn replay_active_banks(
        blocktree: &Arc<Blocktree>,
        bank_forks: &Arc<RwLock<BankForks>>,
        my_pubkey: &Pubkey,
        progress: &mut HashMap<u64, ForkProgress>,
        slot_full_senders: &[Sender<(u64, Pubkey)>],
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

            // Insert a progress entry even for slots this node is the leader for, so that
            // 1) confirm_forks can report confirmation, 2) we can cache computations about
            // this bank in `select_fork()`
            let bank_progress = &mut progress
                .entry(bank.slot())
                .or_insert_with(|| ForkProgress::new(bank.slot(), bank.last_blockhash()));
            if bank.collector_id() != my_pubkey {
                let (replay_result, replay_tx_count) = Self::replay_blocktree_into_bank(
                    &bank,
                    &blocktree,
                    bank_progress,
                    transaction_status_sender.clone(),
                    verify_recyclers,
                );
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

    fn select_fork(
        my_pubkey: &Pubkey,
        ancestors: &HashMap<u64, HashSet<u64>>,
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &Tower,
        progress: &mut HashMap<u64, ForkProgress>,
    ) -> VoteAndPoHBank {
        let tower_start = Instant::now();

        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();
        frozen_banks.sort_by_key(|bank| bank.slot());
        let num_frozen_banks = frozen_banks.len();

        trace!("frozen_banks {}", frozen_banks.len());
        let num_old_banks = frozen_banks
            .iter()
            .filter(|b| b.slot() < tower.root().unwrap_or(0))
            .count();

        let stats: Vec<ForkStats> = frozen_banks
            .iter()
            .map(|bank| {
                // Only time progress map should be missing a bank slot
                // is if this node was the leader for this slot as those banks
                // are not replayed in replay_active_banks()
                let mut stats = progress
                    .get(&bank.slot())
                    .expect("All frozen banks must exist in the Progress map")
                    .fork_stats
                    .clone();

                if !stats.computed {
                    stats.slot = bank.slot();
                    let (stake_lockouts, total_staked, bank_weight) = tower.collect_vote_lockouts(
                        bank.slot(),
                        bank.vote_accounts().into_iter(),
                        &ancestors,
                    );
                    Self::confirm_forks(tower, &stake_lockouts, total_staked, progress, bank_forks);
                    stats.total_staked = total_staked;
                    stats.weight = bank_weight;
                    stats.fork_weight = stats.weight
                        + bank
                            .parent()
                            .and_then(|b| progress.get(&b.slot()))
                            .map(|x| x.fork_stats.fork_weight)
                            .unwrap_or(0);

                    datapoint_warn!(
                        "bank_weight",
                        ("slot", bank.slot(), i64),
                        // u128 too large for influx, convert to hex
                        ("weight", format!("{:X}", stats.weight), String),
                    );
                    warn!(
                        "{} slot_weight: {} {} {} {}",
                        my_pubkey,
                        stats.slot,
                        stats.weight,
                        stats.fork_weight,
                        bank.parent().map(|b| b.slot()).unwrap_or(0)
                    );
                    stats.stake_lockouts = stake_lockouts;
                    stats.block_height = bank.block_height();
                    stats.computed = true;
                }
                stats.vote_threshold = tower.check_vote_stake_threshold(
                    bank.slot(),
                    &stats.stake_lockouts,
                    stats.total_staked,
                );
                stats.is_locked_out = tower.is_locked_out(bank.slot(), &ancestors);
                stats.has_voted = tower.has_voted(bank.slot());
                stats.is_recent = tower.is_recent(bank.slot());
                progress
                    .get_mut(&bank.slot())
                    .expect("All frozen banks must exist in the Progress map")
                    .fork_stats = stats.clone();
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
        candidates.sort_by_key(|b| (b.1.fork_weight, 0i64 - b.1.slot as i64));
        let rv = candidates.last();
        let ms = timing::duration_as_ms(&tower_start.elapsed());
        let weights: Vec<(u128, u64, u64)> = candidates
            .iter()
            .map(|x| (x.1.weight, x.1.slot, x.1.block_height))
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
            "replay_stage-select_fork",
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
        rv.cloned().map(|x| (x.0.clone(), x.1.clone()))
    }

    fn confirm_forks(
        tower: &Tower,
        stake_lockouts: &HashMap<u64, StakeLockout>,
        total_staked: u64,
        progress: &mut HashMap<u64, ForkProgress>,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        for (slot, prog) in progress.iter_mut() {
            if !prog.fork_stats.confirmation_reported {
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
                    prog.fork_stats.confirmation_reported = true;
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
    }

    fn load_blocktree_entries_with_shred_info(
        bank: &Bank,
        blocktree: &Blocktree,
        bank_progress: &mut ForkProgress,
    ) -> Result<(Vec<Entry>, usize, bool)> {
        blocktree
            .get_slot_entries_with_shred_info(bank.slot(), bank_progress.num_shreds as u64)
            .map_err(|err| err.into())
    }

    fn replay_entries_into_bank(
        bank: &Arc<Bank>,
        bank_progress: &mut ForkProgress,
        entries: Vec<Entry>,
        num_shreds: usize,
        slot_full: bool,
        transaction_status_sender: Option<TransactionStatusSender>,
        verify_recyclers: &VerifyRecyclers,
    ) -> Result<()> {
        let result = Self::verify_and_process_entries(
            &bank,
            &entries,
            slot_full,
            bank_progress.num_shreds,
            bank_progress,
            transaction_status_sender,
            verify_recyclers,
        );
        bank_progress.num_shreds += num_shreds;
        bank_progress.num_entries += entries.len();
        if let Some(last_entry) = entries.last() {
            bank_progress.last_entry = last_entry.hash;
        }

        result
    }

    fn verify_ticks(
        bank: &Arc<Bank>,
        entries: &[Entry],
        slot_full: bool,
        tick_hash_count: &mut u64,
    ) -> std::result::Result<(), BlockError> {
        let next_bank_tick_height = bank.tick_height() + entries.tick_count();
        let max_bank_tick_height = bank.max_tick_height();
        if next_bank_tick_height > max_bank_tick_height {
            return Err(BlockError::InvalidTickCount);
        }

        if next_bank_tick_height < max_bank_tick_height && slot_full {
            return Err(BlockError::InvalidTickCount);
        }

        if next_bank_tick_height == max_bank_tick_height {
            let has_trailing_entry = !entries.last().unwrap().is_tick();
            if has_trailing_entry {
                return Err(BlockError::TrailingEntry);
            }

            if !slot_full {
                return Err(BlockError::InvalidLastTick);
            }
        }

        let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
        if !entries.verify_tick_hash_count(tick_hash_count, hashes_per_tick) {
            return Err(BlockError::InvalidTickHashCount);
        }

        Ok(())
    }

    fn verify_and_process_entries(
        bank: &Arc<Bank>,
        entries: &[Entry],
        slot_full: bool,
        shred_index: usize,
        bank_progress: &mut ForkProgress,
        transaction_status_sender: Option<TransactionStatusSender>,
        recyclers: &VerifyRecyclers,
    ) -> Result<()> {
        let last_entry = &bank_progress.last_entry;
        let tick_hash_count = &mut bank_progress.tick_hash_count;
        let handle_block_error = move |block_error: BlockError| -> Result<()> {
            warn!(
                "{:#?}, slot: {}, entry len: {}, tick_height: {}, last entry: {}, last_blockhash: {}, shred_index: {}, slot_full: {}",
                block_error,
                bank.slot(),
                entries.len(),
                bank.tick_height(),
                last_entry,
                bank.last_blockhash(),
                shred_index,
                slot_full,
            );

            datapoint_error!(
                "replay-stage-block-error",
                ("slot", bank.slot(), i64),
                ("last_entry", last_entry.to_string(), String),
            );

            Err(Error::BlockError(block_error))
        };

        if let Err(block_error) = Self::verify_ticks(bank, entries, slot_full, tick_hash_count) {
            return handle_block_error(block_error);
        }

        datapoint_debug!("verify-batch-size", ("size", entries.len() as i64, i64));
        let mut verify_total = Measure::start("verify_and_process_entries");
        let mut entry_state = entries.start_verify(last_entry, recyclers.clone());

        if entry_state.status() == EntryVerificationStatus::Failure {
            return handle_block_error(BlockError::InvalidEntryHash);
        }

        let mut replay_elapsed = Measure::start("replay_elapsed");
        let res =
            blocktree_processor::process_entries(bank, entries, true, transaction_status_sender);
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
        forks_lock: &RwLock<BankForks>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        subscriptions: &Arc<RpcSubscriptions>,
    ) {
        // Find the next slot that chains to the old slot
        let forks = forks_lock.read().unwrap();
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
                subscriptions.notify_slot(child_slot, parent_slot, forks.root());
                new_banks.insert(
                    child_slot,
                    Bank::new_from_parent(&parent_bank, &leader, child_slot),
                );
            }
        }
        drop(forks);

        let mut forks = forks_lock.write().unwrap();
        for (_, bank) in new_banks {
            forks.insert(bank);
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
        consensus::Tower,
        genesis_utils::{create_genesis_config, create_genesis_config_with_leader},
        replay_stage::ReplayStage,
        transaction_status_service::TransactionStatusService,
    };
    use crossbeam_channel::unbounded;
    use solana_ledger::{
        blocktree::make_slot_entries,
        blocktree::{entries_to_test_shreds, BlocktreeError},
        create_new_tmp_ledger,
        entry::{self, next_entry},
        get_tmp_ledger_path,
        shred::{
            CodingShredHeader, DataShredHeader, Shred, ShredCommonHeader, DATA_COMPLETE_SHRED,
            SIZE_OF_COMMON_SHRED_HEADER, SIZE_OF_DATA_SHRED_HEADER, SIZE_OF_DATA_SHRED_PAYLOAD,
        },
    };
    use solana_runtime::genesis_utils::GenesisConfigInfo;
    use solana_sdk::account::Account;
    use solana_sdk::rent::Rent;
    use solana_sdk::{
        hash::{hash, Hash},
        instruction::InstructionError,
        packet::PACKET_DATA_SIZE,
        signature::{Keypair, KeypairUtil, Signature},
        system_transaction,
        transaction::TransactionError,
    };
    use solana_stake_program::stake_state;
    use solana_vote_program::vote_state;
    use solana_vote_program::vote_state::{Vote, VoteState};
    use std::iter;
    use std::{
        fs::remove_dir_all,
        sync::{Arc, RwLock},
    };

    struct ForkInfo {
        leader: usize,
        fork: Vec<Slot>,
        voters: Vec<usize>,
    }

    struct ValidatorInfo {
        stake: u64,
        keypair: Keypair,
        voting_keypair: Keypair,
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
            vote_state.to(&mut vote_account).unwrap();
            bank.store_account(&pubkey, &vote_account);
        }

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
                    &validator.voting_keypair.pubkey(),
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
                    &validator.voting_keypair.pubkey(),
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
                validators[i].voting_keypair.pubkey(),
                genesis_vote_accounts[i].clone(),
            );
            genesis_config.accounts.insert(
                validators[i].staking_keypair.pubkey(),
                genesis_stake_accounts[i].clone(),
            );
        }

        let mut bank_forks = BankForks::new(neutral_fork.fork[0], Bank::new(&genesis_config));

        let mut fork_progresses: Vec<HashMap<u64, ForkProgress>> = iter::repeat_with(HashMap::new)
            .take(validators.len())
            .collect();

        for fork_progress in fork_progresses.iter_mut() {
            fork_progress
                .entry(neutral_fork.fork[0])
                .or_insert_with(|| {
                    ForkProgress::new(
                        bank_forks.banks[&0].slot(),
                        bank_forks.banks[&0].last_blockhash(),
                    )
                });
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
                    &validator.voting_keypair.pubkey(),
                    neutral_fork.fork[index - 1],
                );
            }

            bank_forks.banks[&neutral_fork.fork[index]].freeze();

            for fork_progress in fork_progresses.iter_mut() {
                fork_progress
                    .entry(bank_forks.banks[&neutral_fork.fork[index]].slot())
                    .or_insert_with(|| {
                        ForkProgress::new(
                            bank_forks.banks[&neutral_fork.fork[index]].slot(),
                            bank_forks.banks[&neutral_fork.fork[index]].last_blockhash(),
                        )
                    });
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
                        &validators[*voter_index].voting_keypair.pubkey(),
                        last_bank.slot(),
                    );
                }

                bank_forks.banks[&fork_info.fork[index]].freeze();

                for fork_progress in fork_progresses.iter_mut() {
                    fork_progress
                        .entry(bank_forks.banks[&fork_info.fork[index]].slot())
                        .or_insert_with(|| {
                            ForkProgress::new(
                                bank_forks.banks[&fork_info.fork[index]].slot(),
                                bank_forks.banks[&fork_info.fork[index]].last_blockhash(),
                            )
                        });
                }
            }
        }

        let bank_fork_ancestors = bank_forks.ancestors();
        let wrapped_bank_fork = Arc::new(RwLock::new(bank_forks));

        (0..validators.len())
            .map(|i| {
                let response = ReplayStage::select_fork(
                    &validators[i].keypair.pubkey(),
                    &bank_fork_ancestors,
                    &wrapped_bank_fork,
                    &towers[i],
                    &mut fork_progresses[i],
                );

                if response.is_none() {
                    None
                } else {
                    let (_bank, stats) = response.unwrap();

                    Some(ForkSelectionResponse {
                        slot: stats.slot,
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
                voting_keypair: Keypair::new(),
                staking_keypair: Keypair::new(),
            },
            ValidatorInfo {
                stake: 33_000_000,
                keypair: Keypair::new(),
                voting_keypair: Keypair::new(),
                staking_keypair: Keypair::new(),
            },
            // Malicious Node
            ValidatorInfo {
                stake: 33_000_000,
                keypair: Keypair::new(),
                voting_keypair: Keypair::new(),
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
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );

            let genesis_config = create_genesis_config(10_000).genesis_config;
            let bank0 = Bank::new(&genesis_config);
            let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank0));
            let subscriptions = Arc::new(RpcSubscriptions::default());
            let bank_forks = BankForks::new(0, bank0);
            bank_forks.working_bank().freeze();

            // Insert shred for slot 1, generate new forks, check result
            let (shreds, _) = make_slot_entries(1, 0, 8);
            blocktree.insert_shreds(shreds, None, false).unwrap();
            assert!(bank_forks.get(1).is_none());
            let bank_forks = RwLock::new(bank_forks);
            ReplayStage::generate_new_bank_forks(
                &blocktree,
                &bank_forks,
                &leader_schedule_cache,
                &subscriptions,
            );
            assert!(bank_forks.read().unwrap().get(1).is_some());

            // Insert shred for slot 3, generate new forks, check result
            let (shreds, _) = make_slot_entries(2, 0, 8);
            blocktree.insert_shreds(shreds, None, false).unwrap();
            assert!(bank_forks.read().unwrap().get(2).is_none());
            ReplayStage::generate_new_bank_forks(
                &blocktree,
                &bank_forks,
                &leader_schedule_cache,
                &subscriptions,
            );
            assert!(bank_forks.read().unwrap().get(1).is_some());
            assert!(bank_forks.read().unwrap().get(2).is_some());
        }

        let _ignored = remove_dir_all(&ledger_path);
    }

    #[test]
    fn test_handle_new_root() {
        let genesis_config = create_genesis_config(10_000).genesis_config;
        let bank0 = Bank::new(&genesis_config);
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
            entries_to_test_shreds(vec![entry], slot, slot.saturating_sub(1), false, 0)
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
            entries_to_test_shreds(vec![entry], slot, slot.saturating_sub(1), false, 0)
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
                0,
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

        if let Err(Error::BlockError(block_error)) = res {
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

        if let Err(Error::BlockError(block_error)) = res {
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

        if let Err(Error::BlockError(block_error)) = res {
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
            Err(Error::BlocktreeError(BlocktreeError::InvalidShredData(_)))
        );
    }

    // Given a shred and a fatal expected error, check that replaying that shred causes causes the fork to be
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
            let GenesisConfigInfo {
                mut genesis_config,
                mint_keypair,
                ..
            } = create_genesis_config(1000);
            genesis_config.poh_config.hashes_per_tick = Some(2);
            let bank0 = Arc::new(Bank::new(&genesis_config));
            let mut progress = HashMap::new();
            let last_blockhash = bank0.last_blockhash();
            let mut bank0_progress = progress
                .entry(bank0.slot())
                .or_insert_with(|| ForkProgress::new(0, last_blockhash));
            let shreds = shred_to_insert(&mint_keypair, bank0.clone());
            blocktree.insert_shreds(shreds, None, false).unwrap();
            let (res, _tx_count) = ReplayStage::replay_blocktree_into_bank(
                &bank0,
                &blocktree,
                &mut bank0_progress,
                None,
                &VerifyRecyclers::default(),
            );

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
        let _res = bank1.transfer(10, &genesis_config_info.mint_keypair, &Pubkey::new_rand());
        for _ in 0..genesis_config.ticks_per_slot {
            bank1.register_tick(&Hash::default());
        }
        bank1.freeze();
        bank_forks.write().unwrap().insert(bank1);
        let arc_bank1 = bank_forks.read().unwrap().get(1).unwrap().clone();
        leader_vote(&arc_bank1, &leader_voting_pubkey);
        ReplayStage::update_commitment_cache(arc_bank1.clone(), leader_lamports, &lockouts_sender);

        let bank2 = Bank::new_from_parent(&arc_bank1, &Pubkey::default(), arc_bank1.slot() + 1);
        let _res = bank2.transfer(10, &genesis_config_info.mint_keypair, &Pubkey::new_rand());
        for _ in 0..genesis_config.ticks_per_slot {
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

    pub fn create_test_transactions_and_populate_blocktree(
        keypairs: Vec<&Keypair>,
        previous_slot: Slot,
        bank: Arc<Bank>,
        blocktree: Arc<Blocktree>,
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
        blocktree.insert_shreds(shreds, None, false).unwrap();
        blocktree.set_roots(&[slot]).unwrap();

        let (transaction_status_sender, transaction_status_receiver) = unbounded();
        let transaction_status_service = TransactionStatusService::new(
            transaction_status_receiver,
            blocktree.clone(),
            &Arc::new(AtomicBool::new(false)),
        );

        // Check that process_entries successfully writes can_commit transactions statuses, and
        // that they are matched properly by get_confirmed_block
        let _result = blocktree_processor::process_entries(
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
            let blocktree = Blocktree::open(&ledger_path)
                .expect("Expected to successfully open database ledger");
            let blocktree = Arc::new(blocktree);

            let keypair1 = Keypair::new();
            let keypair2 = Keypair::new();
            let keypair3 = Keypair::new();

            let bank0 = Arc::new(Bank::new(&genesis_config));
            bank0
                .transfer(4, &mint_keypair, &keypair2.pubkey())
                .unwrap();

            let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
            let slot = bank1.slot();

            let signatures = create_test_transactions_and_populate_blocktree(
                vec![&mint_keypair, &keypair1, &keypair2, &keypair3],
                bank0.slot(),
                bank1,
                blocktree.clone(),
            );

            let confirmed_block = blocktree.get_confirmed_block(slot).unwrap();
            assert_eq!(confirmed_block.transactions.len(), 3);

            for (transaction, result) in confirmed_block.transactions.into_iter() {
                if transaction.signatures[0] == signatures[0] {
                    assert_eq!(result.unwrap().status, Ok(()));
                } else if transaction.signatures[0] == signatures[1] {
                    assert_eq!(
                        result.unwrap().status,
                        Err(TransactionError::InstructionError(
                            0,
                            InstructionError::CustomError(1)
                        ))
                    );
                } else {
                    assert_eq!(result, None);
                }
            }
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }
}
