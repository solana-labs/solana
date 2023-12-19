//! The `replay_stage` replays transactions broadcast by the leader.

use {
    crate::{
        banking_trace::BankingTracer,
        cache_block_meta_service::CacheBlockMetaSender,
        cluster_info_vote_listener::{
            GossipDuplicateConfirmedSlotsReceiver, GossipVerifiedVoteHashReceiver, VoteTracker,
        },
        cluster_slots_service::{cluster_slots::ClusterSlots, ClusterSlotsUpdateSender},
        commitment_service::{AggregateCommitmentService, CommitmentAggregationData},
        consensus::{
            fork_choice::{ForkChoice, SelectVoteAndResetForkResult},
            heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
            latest_validator_votes_for_frozen_banks::LatestValidatorVotesForFrozenBanks,
            progress_map::{ForkProgress, ProgressMap, PropagatedStats, ReplaySlotStats},
            tower_storage::{SavedTower, SavedTowerVersions, TowerStorage},
            ComputedBankState, Stake, SwitchForkDecision, ThresholdDecision, Tower, VotedStakes,
            SWITCH_FORK_THRESHOLD,
        },
        cost_update_service::CostUpdate,
        repair::{
            ancestor_hashes_service::AncestorHashesReplayUpdateSender,
            cluster_slot_state_verifier::*,
            duplicate_repair_status::AncestorDuplicateSlotToRepair,
            repair_service::{
                AncestorDuplicateSlotsReceiver, DumpedSlotsSender, PopularPrunedForksReceiver,
            },
        },
        rewards_recorder_service::{RewardsMessage, RewardsRecorderSender},
        unfrozen_gossip_verified_vote_hashes::UnfrozenGossipVerifiedVoteHashes,
        validator::ProcessBlockStore,
        voting_service::VoteOp,
        window_service::DuplicateSlotReceiver,
    },
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender},
    lazy_static::lazy_static,
    rayon::{prelude::*, ThreadPool},
    solana_entry::entry::VerifyRecyclers,
    solana_geyser_plugin_manager::block_metadata_notifier_interface::BlockMetadataNotifierLock,
    solana_gossip::cluster_info::ClusterInfo,
    solana_ledger::{
        block_error::BlockError,
        blockstore::Blockstore,
        blockstore_processor::{
            self, BlockstoreProcessorError, ConfirmationProgress, TransactionStatusSender,
        },
        entry_notifier_service::EntryNotifierSender,
        leader_schedule_cache::LeaderScheduleCache,
        leader_schedule_utils::first_of_consecutive_leader_slots,
    },
    solana_measure::measure::Measure,
    solana_poh::poh_recorder::{PohLeaderStatus, PohRecorder, GRACE_TICKS_FACTOR, MAX_GRACE_SLOTS},
    solana_program_runtime::timings::ExecuteTimings,
    solana_rpc::{
        optimistically_confirmed_bank_tracker::{BankNotification, BankNotificationSenderConfig},
        rpc_subscriptions::RpcSubscriptions,
    },
    solana_rpc_client_api::response::SlotUpdate,
    solana_runtime::{
        accounts_background_service::AbsRequestSender,
        bank::{bank_hash_details, Bank, NewBankOptions},
        bank_forks::{BankForks, MAX_ROOT_DISTANCE_FOR_VOTE_ONLY},
        commitment::BlockCommitmentCache,
        prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_sdk::{
        clock::{BankId, Slot, MAX_PROCESSING_AGE, NUM_CONSECUTIVE_LEADER_SLOTS},
        feature_set,
        genesis_config::ClusterType,
        hash::Hash,
        pubkey::Pubkey,
        saturating_add_assign,
        signature::{Keypair, Signature, Signer},
        timing::timestamp,
        transaction::Transaction,
    },
    solana_vote::vote_sender_types::ReplayVoteSender,
    solana_vote_program::vote_state::VoteTransaction,
    std::{
        collections::{HashMap, HashSet},
        result,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

pub const MAX_ENTRY_RECV_PER_ITER: usize = 512;
pub const SUPERMINORITY_THRESHOLD: f64 = 1f64 / 3f64;
pub const MAX_UNCONFIRMED_SLOTS: usize = 5;
pub const DUPLICATE_LIVENESS_THRESHOLD: f64 = 0.1;
pub const DUPLICATE_THRESHOLD: f64 = 1.0 - SWITCH_FORK_THRESHOLD - DUPLICATE_LIVENESS_THRESHOLD;
const MAX_VOTE_SIGNATURES: usize = 200;
const MAX_VOTE_REFRESH_INTERVAL_MILLIS: usize = 5000;
// Expect this number to be small enough to minimize thread pool overhead while large enough
// to be able to replay all active forks at the same time in most cases.
const MAX_CONCURRENT_FORKS_TO_REPLAY: usize = 4;
const MAX_REPAIR_RETRY_LOOP_ATTEMPTS: usize = 10;

lazy_static! {
    static ref PAR_THREAD_POOL: ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(MAX_CONCURRENT_FORKS_TO_REPLAY)
        .thread_name(|i| format!("solReplay{i:02}"))
        .build()
        .unwrap();
}

#[derive(PartialEq, Eq, Debug)]
pub enum HeaviestForkFailures {
    LockedOut(u64),
    FailedThreshold(
        Slot,
        /* Observed stake */ u64,
        /* Total stake */ u64,
    ),
    FailedSwitchThreshold(
        Slot,
        /* Observed stake */ u64,
        /* Total stake */ u64,
    ),
    NoPropagatedConfirmation(
        Slot,
        /* Observed stake */ u64,
        /* Total stake */ u64,
    ),
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

struct ReplaySlotFromBlockstore {
    is_slot_dead: bool,
    bank_slot: Slot,
    replay_result: Option<Result<usize /* tx count */, BlockstoreProcessorError>>,
}

struct LastVoteRefreshTime {
    last_refresh_time: Instant,
    last_print_time: Instant,
}

#[derive(Default)]
struct SkippedSlotsInfo {
    last_retransmit_slot: u64,
    last_skipped_slot: u64,
}

struct PartitionInfo {
    partition_start_time: Option<Instant>,
}

impl PartitionInfo {
    fn new() -> Self {
        Self {
            partition_start_time: None,
        }
    }

    fn update(
        &mut self,
        partition_detected: bool,
        heaviest_slot: Slot,
        last_voted_slot: Slot,
        reset_bank_slot: Slot,
        heaviest_fork_failures: Vec<HeaviestForkFailures>,
    ) {
        if self.partition_start_time.is_none() && partition_detected {
            warn!("PARTITION DETECTED waiting to join heaviest fork: {} last vote: {:?}, reset slot: {}",
                heaviest_slot,
                last_voted_slot,
                reset_bank_slot,
            );
            datapoint_info!(
                "replay_stage-partition-start",
                ("heaviest_slot", heaviest_slot as i64, i64),
                ("last_vote_slot", last_voted_slot as i64, i64),
                ("reset_slot", reset_bank_slot as i64, i64),
                (
                    "heaviest_fork_failure_first",
                    format!("{:?}", heaviest_fork_failures.first()),
                    String
                ),
                (
                    "heaviest_fork_failure_second",
                    format!("{:?}", heaviest_fork_failures.get(1)),
                    String
                ),
            );
            self.partition_start_time = Some(Instant::now());
        } else if self.partition_start_time.is_some() && !partition_detected {
            warn!(
                "PARTITION resolved heaviest fork: {} last vote: {:?}, reset slot: {}",
                heaviest_slot, last_voted_slot, reset_bank_slot
            );
            datapoint_info!(
                "replay_stage-partition-resolved",
                ("heaviest_slot", heaviest_slot as i64, i64),
                ("last_vote_slot", last_voted_slot as i64, i64),
                ("reset_slot", reset_bank_slot as i64, i64),
                (
                    "partition_duration_ms",
                    self.partition_start_time.unwrap().elapsed().as_millis() as i64,
                    i64
                ),
            );
            self.partition_start_time = None;
        }
    }
}

pub struct ReplayStageConfig {
    pub vote_account: Pubkey,
    pub authorized_voter_keypairs: Arc<RwLock<Vec<Arc<Keypair>>>>,
    pub exit: Arc<AtomicBool>,
    pub rpc_subscriptions: Arc<RpcSubscriptions>,
    pub leader_schedule_cache: Arc<LeaderScheduleCache>,
    pub latest_root_senders: Vec<Sender<Slot>>,
    pub accounts_background_request_sender: AbsRequestSender,
    pub block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    pub transaction_status_sender: Option<TransactionStatusSender>,
    pub rewards_recorder_sender: Option<RewardsRecorderSender>,
    pub cache_block_meta_sender: Option<CacheBlockMetaSender>,
    pub entry_notification_sender: Option<EntryNotifierSender>,
    pub bank_notification_sender: Option<BankNotificationSenderConfig>,
    pub wait_for_vote_to_start_leader: bool,
    pub ancestor_hashes_replay_update_sender: AncestorHashesReplayUpdateSender,
    pub tower_storage: Arc<dyn TowerStorage>,
    // Stops voting until this slot has been reached. Should be used to avoid
    // duplicate voting which can lead to slashing.
    pub wait_to_vote_slot: Option<Slot>,
    pub replay_slots_concurrently: bool,
}

#[derive(Default)]
pub struct ReplayTiming {
    last_print: u64,
    collect_frozen_banks_elapsed: u64,
    compute_bank_stats_elapsed: u64,
    select_vote_and_reset_forks_elapsed: u64,
    start_leader_elapsed: u64,
    reset_bank_elapsed: u64,
    voting_elapsed: u64,
    vote_push_us: u64,
    vote_send_us: u64,
    generate_vote_us: u64,
    update_commitment_cache_us: u64,
    select_forks_elapsed: u64,
    compute_slot_stats_elapsed: u64,
    generate_new_bank_forks_elapsed: u64,
    replay_active_banks_elapsed: u64,
    wait_receive_elapsed: u64,
    heaviest_fork_failures_elapsed: u64,
    bank_count: u64,
    process_ancestor_hashes_duplicate_slots_elapsed: u64,
    process_gossip_duplicate_confirmed_slots_elapsed: u64,
    process_duplicate_slots_elapsed: u64,
    process_unfrozen_gossip_verified_vote_hashes_elapsed: u64,
    process_popular_pruned_forks_elapsed: u64,
    repair_correct_slots_elapsed: u64,
    retransmit_not_propagated_elapsed: u64,
    generate_new_bank_forks_read_lock_us: u64,
    generate_new_bank_forks_get_slots_since_us: u64,
    generate_new_bank_forks_loop_us: u64,
    generate_new_bank_forks_write_lock_us: u64,
    replay_blockstore_us: u64, //< When processing forks concurrently, only captures the longest fork
}
impl ReplayTiming {
    #[allow(clippy::too_many_arguments)]
    fn update(
        &mut self,
        collect_frozen_banks_elapsed: u64,
        compute_bank_stats_elapsed: u64,
        select_vote_and_reset_forks_elapsed: u64,
        start_leader_elapsed: u64,
        reset_bank_elapsed: u64,
        voting_elapsed: u64,
        select_forks_elapsed: u64,
        compute_slot_stats_elapsed: u64,
        generate_new_bank_forks_elapsed: u64,
        replay_active_banks_elapsed: u64,
        wait_receive_elapsed: u64,
        heaviest_fork_failures_elapsed: u64,
        bank_count: u64,
        process_ancestor_hashes_duplicate_slots_elapsed: u64,
        process_gossip_duplicate_confirmed_slots_elapsed: u64,
        process_unfrozen_gossip_verified_vote_hashes_elapsed: u64,
        process_popular_pruned_forks_elapsed: u64,
        process_duplicate_slots_elapsed: u64,
        repair_correct_slots_elapsed: u64,
        retransmit_not_propagated_elapsed: u64,
    ) {
        self.collect_frozen_banks_elapsed += collect_frozen_banks_elapsed;
        self.compute_bank_stats_elapsed += compute_bank_stats_elapsed;
        self.select_vote_and_reset_forks_elapsed += select_vote_and_reset_forks_elapsed;
        self.start_leader_elapsed += start_leader_elapsed;
        self.reset_bank_elapsed += reset_bank_elapsed;
        self.voting_elapsed += voting_elapsed;
        self.select_forks_elapsed += select_forks_elapsed;
        self.compute_slot_stats_elapsed += compute_slot_stats_elapsed;
        self.generate_new_bank_forks_elapsed += generate_new_bank_forks_elapsed;
        self.replay_active_banks_elapsed += replay_active_banks_elapsed;
        self.wait_receive_elapsed += wait_receive_elapsed;
        self.heaviest_fork_failures_elapsed += heaviest_fork_failures_elapsed;
        self.bank_count += bank_count;
        self.process_ancestor_hashes_duplicate_slots_elapsed +=
            process_ancestor_hashes_duplicate_slots_elapsed;
        self.process_gossip_duplicate_confirmed_slots_elapsed +=
            process_gossip_duplicate_confirmed_slots_elapsed;
        self.process_unfrozen_gossip_verified_vote_hashes_elapsed +=
            process_unfrozen_gossip_verified_vote_hashes_elapsed;
        self.process_popular_pruned_forks_elapsed += process_popular_pruned_forks_elapsed;
        self.process_duplicate_slots_elapsed += process_duplicate_slots_elapsed;
        self.repair_correct_slots_elapsed += repair_correct_slots_elapsed;
        self.retransmit_not_propagated_elapsed += retransmit_not_propagated_elapsed;
        let now = timestamp();
        let elapsed_ms = now - self.last_print;
        if elapsed_ms > 1000 {
            datapoint_info!(
                "replay-loop-voting-stats",
                ("vote_push_us", self.vote_push_us, i64),
                ("vote_send_us", self.vote_send_us, i64),
                ("generate_vote_us", self.generate_vote_us, i64),
                (
                    "update_commitment_cache_us",
                    self.update_commitment_cache_us,
                    i64
                ),
            );
            datapoint_info!(
                "replay-loop-timing-stats",
                ("total_elapsed_us", elapsed_ms * 1000, i64),
                (
                    "collect_frozen_banks_elapsed",
                    self.collect_frozen_banks_elapsed as i64,
                    i64
                ),
                (
                    "compute_bank_stats_elapsed",
                    self.compute_bank_stats_elapsed as i64,
                    i64
                ),
                (
                    "select_vote_and_reset_forks_elapsed",
                    self.select_vote_and_reset_forks_elapsed as i64,
                    i64
                ),
                (
                    "start_leader_elapsed",
                    self.start_leader_elapsed as i64,
                    i64
                ),
                ("reset_bank_elapsed", self.reset_bank_elapsed as i64, i64),
                ("voting_elapsed", self.voting_elapsed as i64, i64),
                (
                    "select_forks_elapsed",
                    self.select_forks_elapsed as i64,
                    i64
                ),
                (
                    "compute_slot_stats_elapsed",
                    self.compute_slot_stats_elapsed as i64,
                    i64
                ),
                (
                    "generate_new_bank_forks_elapsed",
                    self.generate_new_bank_forks_elapsed as i64,
                    i64
                ),
                (
                    "replay_active_banks_elapsed",
                    self.replay_active_banks_elapsed as i64,
                    i64
                ),
                (
                    "process_ancestor_hashes_duplicate_slots_elapsed",
                    self.process_ancestor_hashes_duplicate_slots_elapsed as i64,
                    i64
                ),
                (
                    "process_gossip_duplicate_confirmed_slots_elapsed",
                    self.process_gossip_duplicate_confirmed_slots_elapsed as i64,
                    i64
                ),
                (
                    "process_unfrozen_gossip_verified_vote_hashes_elapsed",
                    self.process_unfrozen_gossip_verified_vote_hashes_elapsed as i64,
                    i64
                ),
                (
                    "process_popular_pruned_forks_elapsed",
                    self.process_popular_pruned_forks_elapsed as i64,
                    i64
                ),
                (
                    "wait_receive_elapsed",
                    self.wait_receive_elapsed as i64,
                    i64
                ),
                (
                    "heaviest_fork_failures_elapsed",
                    self.heaviest_fork_failures_elapsed as i64,
                    i64
                ),
                ("bank_count", self.bank_count as i64, i64),
                (
                    "process_duplicate_slots_elapsed",
                    self.process_duplicate_slots_elapsed as i64,
                    i64
                ),
                (
                    "repair_correct_slots_elapsed",
                    self.repair_correct_slots_elapsed as i64,
                    i64
                ),
                (
                    "retransmit_not_propagated_elapsed",
                    self.retransmit_not_propagated_elapsed as i64,
                    i64
                ),
                (
                    "generate_new_bank_forks_read_lock_us",
                    self.generate_new_bank_forks_read_lock_us as i64,
                    i64
                ),
                (
                    "generate_new_bank_forks_get_slots_since_us",
                    self.generate_new_bank_forks_get_slots_since_us as i64,
                    i64
                ),
                (
                    "generate_new_bank_forks_loop_us",
                    self.generate_new_bank_forks_loop_us as i64,
                    i64
                ),
                (
                    "generate_new_bank_forks_write_lock_us",
                    self.generate_new_bank_forks_write_lock_us as i64,
                    i64
                ),
                (
                    "replay_blockstore_us",
                    self.replay_blockstore_us as i64,
                    i64
                ),
            );
            *self = ReplayTiming::default();
            self.last_print = now;
        }
    }
}

pub struct ReplayStage {
    t_replay: JoinHandle<()>,
    commitment_service: AggregateCommitmentService,
}

impl ReplayStage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: ReplayStageConfig,
        blockstore: Arc<Blockstore>,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: Arc<ClusterInfo>,
        ledger_signal_receiver: Receiver<bool>,
        duplicate_slots_receiver: DuplicateSlotReceiver,
        poh_recorder: Arc<RwLock<PohRecorder>>,
        maybe_process_blockstore: Option<ProcessBlockStore>,
        vote_tracker: Arc<VoteTracker>,
        cluster_slots: Arc<ClusterSlots>,
        retransmit_slots_sender: Sender<Slot>,
        ancestor_duplicate_slots_receiver: AncestorDuplicateSlotsReceiver,
        replay_vote_sender: ReplayVoteSender,
        gossip_duplicate_confirmed_slots_receiver: GossipDuplicateConfirmedSlotsReceiver,
        gossip_verified_vote_hash_receiver: GossipVerifiedVoteHashReceiver,
        cluster_slots_update_sender: ClusterSlotsUpdateSender,
        cost_update_sender: Sender<CostUpdate>,
        voting_sender: Sender<VoteOp>,
        drop_bank_sender: Sender<Vec<Arc<Bank>>>,
        block_metadata_notifier: Option<BlockMetadataNotifierLock>,
        log_messages_bytes_limit: Option<usize>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
        dumped_slots_sender: DumpedSlotsSender,
        banking_tracer: Arc<BankingTracer>,
        popular_pruned_forks_receiver: PopularPrunedForksReceiver,
    ) -> Result<Self, String> {
        let mut tower = if let Some(process_blockstore) = maybe_process_blockstore {
            let tower = process_blockstore.process_to_create_tower()?;
            info!("Tower state: {:?}", tower);
            tower
        } else {
            warn!("creating default tower....");
            Tower::default()
        };

        let ReplayStageConfig {
            vote_account,
            authorized_voter_keypairs,
            exit,
            rpc_subscriptions,
            leader_schedule_cache,
            latest_root_senders,
            accounts_background_request_sender,
            block_commitment_cache,
            transaction_status_sender,
            rewards_recorder_sender,
            cache_block_meta_sender,
            entry_notification_sender,
            bank_notification_sender,
            wait_for_vote_to_start_leader,
            ancestor_hashes_replay_update_sender,
            tower_storage,
            wait_to_vote_slot,
            replay_slots_concurrently,
        } = config;

        trace!("replay stage");
        // Start the replay stage loop
        let (lockouts_sender, commitment_service) = AggregateCommitmentService::new(
            exit.clone(),
            block_commitment_cache.clone(),
            rpc_subscriptions.clone(),
        );
        let run_replay = move || {
            let verify_recyclers = VerifyRecyclers::default();
            let _exit = Finalizer::new(exit.clone());
            let mut identity_keypair = cluster_info.keypair().clone();
            let mut my_pubkey = identity_keypair.pubkey();
            let (mut progress, mut heaviest_subtree_fork_choice) =
                Self::initialize_progress_and_fork_choice_with_locked_bank_forks(
                    &bank_forks,
                    &my_pubkey,
                    &vote_account,
                    &blockstore,
                );
            let mut current_leader = None;
            let mut last_reset = Hash::default();
            let mut partition_info = PartitionInfo::new();
            let mut skipped_slots_info = SkippedSlotsInfo::default();
            let mut replay_timing = ReplayTiming::default();
            let mut duplicate_slots_tracker = DuplicateSlotsTracker::default();
            let mut gossip_duplicate_confirmed_slots: GossipDuplicateConfirmedSlots =
                GossipDuplicateConfirmedSlots::default();
            let mut epoch_slots_frozen_slots: EpochSlotsFrozenSlots =
                EpochSlotsFrozenSlots::default();
            let mut duplicate_slots_to_repair = DuplicateSlotsToRepair::default();
            let mut purge_repair_slot_counter = PurgeRepairSlotCounter::default();
            let mut unfrozen_gossip_verified_vote_hashes: UnfrozenGossipVerifiedVoteHashes =
                UnfrozenGossipVerifiedVoteHashes::default();
            let mut latest_validator_votes_for_frozen_banks: LatestValidatorVotesForFrozenBanks =
                LatestValidatorVotesForFrozenBanks::default();
            let mut voted_signatures = Vec::new();
            let mut has_new_vote_been_rooted = !wait_for_vote_to_start_leader;
            let mut last_vote_refresh_time = LastVoteRefreshTime {
                last_refresh_time: Instant::now(),
                last_print_time: Instant::now(),
            };
            let (working_bank, in_vote_only_mode) = {
                let r_bank_forks = bank_forks.read().unwrap();
                (
                    r_bank_forks.working_bank(),
                    r_bank_forks.get_vote_only_mode_signal(),
                )
            };

            Self::reset_poh_recorder(
                &my_pubkey,
                &blockstore,
                working_bank,
                &poh_recorder,
                &leader_schedule_cache,
            );

            loop {
                // Stop getting entries if we get exit signal
                if exit.load(Ordering::Relaxed) {
                    break;
                }

                let mut generate_new_bank_forks_time =
                    Measure::start("generate_new_bank_forks_time");
                Self::generate_new_bank_forks(
                    &blockstore,
                    &bank_forks,
                    &leader_schedule_cache,
                    &rpc_subscriptions,
                    &mut progress,
                    &mut replay_timing,
                );
                generate_new_bank_forks_time.stop();

                let mut tpu_has_bank = poh_recorder.read().unwrap().has_bank();

                let mut replay_active_banks_time = Measure::start("replay_active_banks_time");
                let mut ancestors = bank_forks.read().unwrap().ancestors();
                let mut descendants = bank_forks.read().unwrap().descendants();
                let did_complete_bank = Self::replay_active_banks(
                    &blockstore,
                    &bank_forks,
                    &my_pubkey,
                    &vote_account,
                    &mut progress,
                    transaction_status_sender.as_ref(),
                    cache_block_meta_sender.as_ref(),
                    entry_notification_sender.as_ref(),
                    &verify_recyclers,
                    &mut heaviest_subtree_fork_choice,
                    &replay_vote_sender,
                    &bank_notification_sender,
                    &rewards_recorder_sender,
                    &rpc_subscriptions,
                    &mut duplicate_slots_tracker,
                    &gossip_duplicate_confirmed_slots,
                    &mut epoch_slots_frozen_slots,
                    &mut unfrozen_gossip_verified_vote_hashes,
                    &mut latest_validator_votes_for_frozen_banks,
                    &cluster_slots_update_sender,
                    &cost_update_sender,
                    &mut duplicate_slots_to_repair,
                    &ancestor_hashes_replay_update_sender,
                    block_metadata_notifier.clone(),
                    &mut replay_timing,
                    log_messages_bytes_limit,
                    replay_slots_concurrently,
                    &prioritization_fee_cache,
                    &mut purge_repair_slot_counter,
                );
                replay_active_banks_time.stop();

                let forks_root = bank_forks.read().unwrap().root();

                // Process cluster-agreed versions of duplicate slots for which we potentially
                // have the wrong version. Our version was dead or pruned.
                // Signalled by ancestor_hashes_service.
                let mut process_ancestor_hashes_duplicate_slots_time =
                    Measure::start("process_ancestor_hashes_duplicate_slots");
                Self::process_ancestor_hashes_duplicate_slots(
                    &my_pubkey,
                    &blockstore,
                    &ancestor_duplicate_slots_receiver,
                    &mut duplicate_slots_tracker,
                    &gossip_duplicate_confirmed_slots,
                    &mut epoch_slots_frozen_slots,
                    &progress,
                    &mut heaviest_subtree_fork_choice,
                    &bank_forks,
                    &mut duplicate_slots_to_repair,
                    &ancestor_hashes_replay_update_sender,
                    &mut purge_repair_slot_counter,
                );
                process_ancestor_hashes_duplicate_slots_time.stop();

                // Check for any newly confirmed slots detected from gossip.
                let mut process_gossip_duplicate_confirmed_slots_time =
                    Measure::start("process_gossip_duplicate_confirmed_slots");
                Self::process_gossip_duplicate_confirmed_slots(
                    &gossip_duplicate_confirmed_slots_receiver,
                    &blockstore,
                    &mut duplicate_slots_tracker,
                    &mut gossip_duplicate_confirmed_slots,
                    &mut epoch_slots_frozen_slots,
                    &bank_forks,
                    &progress,
                    &mut heaviest_subtree_fork_choice,
                    &mut duplicate_slots_to_repair,
                    &ancestor_hashes_replay_update_sender,
                    &mut purge_repair_slot_counter,
                );
                process_gossip_duplicate_confirmed_slots_time.stop();

                // Ingest any new verified votes from gossip. Important for fork choice
                // and switching proofs because these may be votes that haven't yet been
                // included in a block, so we may not have yet observed these votes just
                // by replaying blocks.
                let mut process_unfrozen_gossip_verified_vote_hashes_time =
                    Measure::start("process_gossip_verified_vote_hashes");
                Self::process_gossip_verified_vote_hashes(
                    &gossip_verified_vote_hash_receiver,
                    &mut unfrozen_gossip_verified_vote_hashes,
                    &heaviest_subtree_fork_choice,
                    &mut latest_validator_votes_for_frozen_banks,
                );
                for _ in gossip_verified_vote_hash_receiver.try_iter() {}
                process_unfrozen_gossip_verified_vote_hashes_time.stop();

                let mut process_popular_pruned_forks_time =
                    Measure::start("process_popular_pruned_forks_time");
                // Check for "popular" (52+% stake aggregated across versions/descendants) forks
                // that are pruned, which would not be detected by normal means.
                // Signalled by `repair_service`.
                Self::process_popular_pruned_forks(
                    &popular_pruned_forks_receiver,
                    &blockstore,
                    &mut duplicate_slots_tracker,
                    &mut epoch_slots_frozen_slots,
                    &bank_forks,
                    &mut heaviest_subtree_fork_choice,
                    &mut duplicate_slots_to_repair,
                    &ancestor_hashes_replay_update_sender,
                    &mut purge_repair_slot_counter,
                );
                process_popular_pruned_forks_time.stop();

                // Check to remove any duplicated slots from fork choice
                let mut process_duplicate_slots_time = Measure::start("process_duplicate_slots");
                if !tpu_has_bank {
                    Self::process_duplicate_slots(
                        &blockstore,
                        &duplicate_slots_receiver,
                        &mut duplicate_slots_tracker,
                        &gossip_duplicate_confirmed_slots,
                        &mut epoch_slots_frozen_slots,
                        &bank_forks,
                        &progress,
                        &mut heaviest_subtree_fork_choice,
                        &mut duplicate_slots_to_repair,
                        &ancestor_hashes_replay_update_sender,
                        &mut purge_repair_slot_counter,
                    );
                }
                process_duplicate_slots_time.stop();

                let mut collect_frozen_banks_time = Measure::start("frozen_banks");
                let mut frozen_banks: Vec<_> = bank_forks
                    .read()
                    .unwrap()
                    .frozen_banks()
                    .into_iter()
                    .filter(|(slot, _)| *slot >= forks_root)
                    .map(|(_, bank)| bank)
                    .collect();
                collect_frozen_banks_time.stop();

                let mut compute_bank_stats_time = Measure::start("compute_bank_stats");
                let newly_computed_slot_stats = Self::compute_bank_stats(
                    &vote_account,
                    &ancestors,
                    &mut frozen_banks,
                    &mut tower,
                    &mut progress,
                    &vote_tracker,
                    &cluster_slots,
                    &bank_forks,
                    &mut heaviest_subtree_fork_choice,
                    &mut latest_validator_votes_for_frozen_banks,
                );
                compute_bank_stats_time.stop();

                let mut compute_slot_stats_time = Measure::start("compute_slot_stats_time");
                for slot in newly_computed_slot_stats {
                    let fork_stats = progress.get_fork_stats(slot).unwrap();
                    let confirmed_forks = Self::confirm_forks(
                        &tower,
                        &fork_stats.voted_stakes,
                        fork_stats.total_stake,
                        &progress,
                        &bank_forks,
                    );

                    Self::mark_slots_confirmed(
                        &confirmed_forks,
                        &blockstore,
                        &bank_forks,
                        &mut progress,
                        &mut duplicate_slots_tracker,
                        &mut heaviest_subtree_fork_choice,
                        &mut epoch_slots_frozen_slots,
                        &mut duplicate_slots_to_repair,
                        &ancestor_hashes_replay_update_sender,
                        &mut purge_repair_slot_counter,
                    );
                }
                compute_slot_stats_time.stop();

                let mut select_forks_time = Measure::start("select_forks_time");
                let (heaviest_bank, heaviest_bank_on_same_voted_fork) =
                    heaviest_subtree_fork_choice.select_forks(
                        &frozen_banks,
                        &tower,
                        &progress,
                        &ancestors,
                        &bank_forks,
                    );
                select_forks_time.stop();

                Self::check_for_vote_only_mode(
                    heaviest_bank.slot(),
                    forks_root,
                    &in_vote_only_mode,
                    &bank_forks,
                );

                let mut select_vote_and_reset_forks_time =
                    Measure::start("select_vote_and_reset_forks");
                let SelectVoteAndResetForkResult {
                    vote_bank,
                    reset_bank,
                    heaviest_fork_failures,
                } = Self::select_vote_and_reset_forks(
                    &heaviest_bank,
                    heaviest_bank_on_same_voted_fork.as_ref(),
                    &ancestors,
                    &descendants,
                    &progress,
                    &mut tower,
                    &latest_validator_votes_for_frozen_banks,
                    &heaviest_subtree_fork_choice,
                );
                select_vote_and_reset_forks_time.stop();

                if vote_bank.is_none() {
                    if let Some(heaviest_bank_on_same_voted_fork) =
                        heaviest_bank_on_same_voted_fork.as_ref()
                    {
                        if let Some(my_latest_landed_vote) =
                            progress.my_latest_landed_vote(heaviest_bank_on_same_voted_fork.slot())
                        {
                            Self::refresh_last_vote(
                                &mut tower,
                                heaviest_bank_on_same_voted_fork,
                                my_latest_landed_vote,
                                &vote_account,
                                &identity_keypair,
                                &authorized_voter_keypairs.read().unwrap(),
                                &mut voted_signatures,
                                has_new_vote_been_rooted,
                                &mut last_vote_refresh_time,
                                &voting_sender,
                                wait_to_vote_slot,
                            );
                        }
                    }
                }

                let mut heaviest_fork_failures_time = Measure::start("heaviest_fork_failures_time");
                if tower.is_recent(heaviest_bank.slot()) && !heaviest_fork_failures.is_empty() {
                    info!(
                        "Couldn't vote on heaviest fork: {:?}, heaviest_fork_failures: {:?}",
                        heaviest_bank.slot(),
                        heaviest_fork_failures
                    );

                    for r in &heaviest_fork_failures {
                        if let HeaviestForkFailures::NoPropagatedConfirmation(slot, ..) = r {
                            if let Some(latest_leader_slot) =
                                progress.get_latest_leader_slot_must_exist(*slot)
                            {
                                progress.log_propagated_stats(latest_leader_slot, &bank_forks);
                            }
                        }
                    }
                }
                heaviest_fork_failures_time.stop();

                let mut voting_time = Measure::start("voting_time");
                // Vote on a fork
                if let Some((ref vote_bank, ref switch_fork_decision)) = vote_bank {
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
                        vote_bank,
                        switch_fork_decision,
                        &bank_forks,
                        &mut tower,
                        &mut progress,
                        &vote_account,
                        &identity_keypair,
                        &authorized_voter_keypairs.read().unwrap(),
                        &blockstore,
                        &leader_schedule_cache,
                        &lockouts_sender,
                        &accounts_background_request_sender,
                        &latest_root_senders,
                        &rpc_subscriptions,
                        &block_commitment_cache,
                        &mut heaviest_subtree_fork_choice,
                        &bank_notification_sender,
                        &mut duplicate_slots_tracker,
                        &mut gossip_duplicate_confirmed_slots,
                        &mut unfrozen_gossip_verified_vote_hashes,
                        &mut voted_signatures,
                        &mut has_new_vote_been_rooted,
                        &mut replay_timing,
                        &voting_sender,
                        &mut epoch_slots_frozen_slots,
                        &drop_bank_sender,
                        wait_to_vote_slot,
                    );
                }
                voting_time.stop();

                let mut reset_bank_time = Measure::start("reset_bank");
                // Reset onto a fork
                if let Some(reset_bank) = reset_bank {
                    if last_reset != reset_bank.last_blockhash() {
                        info!(
                            "vote bank: {:?} reset bank: {:?}",
                            vote_bank
                                .as_ref()
                                .map(|(b, switch_fork_decision)| (b.slot(), switch_fork_decision)),
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

                        if my_pubkey != cluster_info.id() {
                            identity_keypair = cluster_info.keypair().clone();
                            let my_old_pubkey = my_pubkey;
                            my_pubkey = identity_keypair.pubkey();

                            // Load the new identity's tower
                            tower = Tower::restore(tower_storage.as_ref(), &my_pubkey)
                                .and_then(|restored_tower| {
                                    let root_bank = bank_forks.read().unwrap().root_bank();
                                    let slot_history = root_bank.get_slot_history();
                                    restored_tower.adjust_lockouts_after_replay(
                                        root_bank.slot(),
                                        &slot_history,
                                    )
                                })
                                .unwrap_or_else(|err| {
                                    if err.is_file_missing() {
                                        Tower::new_from_bankforks(
                                            &bank_forks.read().unwrap(),
                                            &my_pubkey,
                                            &vote_account,
                                        )
                                    } else {
                                        error!("Failed to load tower for {}: {}", my_pubkey, err);
                                        std::process::exit(1);
                                    }
                                });

                            // Ensure the validator can land votes with the new identity before
                            // becoming leader
                            has_new_vote_been_rooted = !wait_for_vote_to_start_leader;
                            warn!("Identity changed from {} to {}", my_old_pubkey, my_pubkey);
                        }

                        Self::reset_poh_recorder(
                            &my_pubkey,
                            &blockstore,
                            reset_bank.clone(),
                            &poh_recorder,
                            &leader_schedule_cache,
                        );
                        last_reset = reset_bank.last_blockhash();
                        tpu_has_bank = false;

                        if let Some(last_voted_slot) = tower.last_voted_slot() {
                            // If the current heaviest bank is not a descendant of the last voted slot,
                            // there must be a partition
                            partition_info.update(
                                Self::is_partition_detected(
                                    &ancestors,
                                    last_voted_slot,
                                    heaviest_bank.slot(),
                                ),
                                heaviest_bank.slot(),
                                last_voted_slot,
                                reset_bank.slot(),
                                heaviest_fork_failures,
                            );
                        }
                    }
                }
                reset_bank_time.stop();

                let mut start_leader_time = Measure::start("start_leader_time");
                let mut dump_then_repair_correct_slots_time =
                    Measure::start("dump_then_repair_correct_slots_time");
                // Used for correctness check
                let poh_bank = poh_recorder.read().unwrap().bank();
                // Dump any duplicate slots that have been confirmed by the network in
                // anticipation of repairing the confirmed version of the slot.
                //
                // Has to be before `maybe_start_leader()`. Otherwise, `ancestors` and `descendants`
                // will be outdated, and we cannot assume `poh_bank` will be in either of these maps.
                Self::dump_then_repair_correct_slots(
                    &mut duplicate_slots_to_repair,
                    &mut ancestors,
                    &mut descendants,
                    &mut progress,
                    &bank_forks,
                    &blockstore,
                    poh_bank.map(|bank| bank.slot()),
                    &mut purge_repair_slot_counter,
                    &dumped_slots_sender,
                    &my_pubkey,
                    &leader_schedule_cache,
                );
                dump_then_repair_correct_slots_time.stop();

                let mut retransmit_not_propagated_time =
                    Measure::start("retransmit_not_propagated_time");
                Self::retransmit_latest_unpropagated_leader_slot(
                    &poh_recorder,
                    &retransmit_slots_sender,
                    &mut progress,
                );
                retransmit_not_propagated_time.stop();

                // From this point on, its not safe to use ancestors/descendants since maybe_start_leader
                // may add a bank that will not included in either of these maps.
                drop(ancestors);
                drop(descendants);
                if !tpu_has_bank {
                    Self::maybe_start_leader(
                        &my_pubkey,
                        &bank_forks,
                        &poh_recorder,
                        &leader_schedule_cache,
                        &rpc_subscriptions,
                        &mut progress,
                        &retransmit_slots_sender,
                        &mut skipped_slots_info,
                        &banking_tracer,
                        has_new_vote_been_rooted,
                        transaction_status_sender.is_some(),
                    );

                    let poh_bank = poh_recorder.read().unwrap().bank();
                    if let Some(bank) = poh_bank {
                        Self::log_leader_change(
                            &my_pubkey,
                            bank.slot(),
                            &mut current_leader,
                            &my_pubkey,
                        );
                    }
                }
                start_leader_time.stop();

                let mut wait_receive_time = Measure::start("wait_receive_time");
                if !did_complete_bank {
                    // only wait for the signal if we did not just process a bank; maybe there are more slots available

                    let timer = Duration::from_millis(100);
                    let result = ledger_signal_receiver.recv_timeout(timer);
                    match result {
                        Err(RecvTimeoutError::Timeout) => (),
                        Err(_) => break,
                        Ok(_) => trace!("blockstore signal"),
                    };
                }
                wait_receive_time.stop();

                replay_timing.update(
                    collect_frozen_banks_time.as_us(),
                    compute_bank_stats_time.as_us(),
                    select_vote_and_reset_forks_time.as_us(),
                    start_leader_time.as_us(),
                    reset_bank_time.as_us(),
                    voting_time.as_us(),
                    select_forks_time.as_us(),
                    compute_slot_stats_time.as_us(),
                    generate_new_bank_forks_time.as_us(),
                    replay_active_banks_time.as_us(),
                    wait_receive_time.as_us(),
                    heaviest_fork_failures_time.as_us(),
                    u64::from(did_complete_bank),
                    process_ancestor_hashes_duplicate_slots_time.as_us(),
                    process_gossip_duplicate_confirmed_slots_time.as_us(),
                    process_unfrozen_gossip_verified_vote_hashes_time.as_us(),
                    process_popular_pruned_forks_time.as_us(),
                    process_duplicate_slots_time.as_us(),
                    dump_then_repair_correct_slots_time.as_us(),
                    retransmit_not_propagated_time.as_us(),
                );
            }
        };
        let t_replay = Builder::new()
            .name("solReplayStage".to_string())
            .spawn(run_replay)
            .unwrap();

        Ok(Self {
            t_replay,
            commitment_service,
        })
    }

    fn check_for_vote_only_mode(
        heaviest_bank_slot: Slot,
        forks_root: Slot,
        in_vote_only_mode: &AtomicBool,
        bank_forks: &RwLock<BankForks>,
    ) {
        if heaviest_bank_slot.saturating_sub(forks_root) > MAX_ROOT_DISTANCE_FOR_VOTE_ONLY {
            if !in_vote_only_mode.load(Ordering::Relaxed)
                && in_vote_only_mode
                    .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                let bank_forks = bank_forks.read().unwrap();
                datapoint_warn!(
                    "bank_forks-entering-vote-only-mode",
                    ("banks_len", bank_forks.len(), i64),
                    ("heaviest_bank", heaviest_bank_slot, i64),
                    ("root", bank_forks.root(), i64),
                );
            }
        } else if in_vote_only_mode.load(Ordering::Relaxed)
            && in_vote_only_mode
                .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            let bank_forks = bank_forks.read().unwrap();
            datapoint_warn!(
                "bank_forks-exiting-vote-only-mode",
                ("banks_len", bank_forks.len(), i64),
                ("heaviest_bank", heaviest_bank_slot, i64),
                ("root", bank_forks.root(), i64),
            );
        }
    }

    fn maybe_retransmit_unpropagated_slots(
        metric_name: &'static str,
        retransmit_slots_sender: &Sender<Slot>,
        progress: &mut ProgressMap,
        latest_leader_slot: Slot,
    ) {
        let first_leader_group_slot = first_of_consecutive_leader_slots(latest_leader_slot);

        for slot in first_leader_group_slot..=latest_leader_slot {
            let is_propagated = progress.is_propagated(slot);
            if let Some(retransmit_info) = progress.get_retransmit_info_mut(slot) {
                if !is_propagated.expect(
                    "presence of retransmit_info ensures that propagation status is present",
                ) {
                    if retransmit_info.reached_retransmit_threshold() {
                        info!(
                            "Retrying retransmit: latest_leader_slot={} slot={} retransmit_info={:?}",
                            latest_leader_slot,
                            slot,
                            &retransmit_info,
                        );
                        datapoint_info!(
                            metric_name,
                            ("latest_leader_slot", latest_leader_slot, i64),
                            ("slot", slot, i64),
                            ("retry_iteration", retransmit_info.retry_iteration, i64),
                        );
                        let _ = retransmit_slots_sender.send(slot);
                        retransmit_info.increment_retry_iteration();
                    } else {
                        debug!(
                            "Bypass retransmit of slot={} retransmit_info={:?}",
                            slot, &retransmit_info
                        );
                    }
                }
            }
        }
    }

    fn retransmit_latest_unpropagated_leader_slot(
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        retransmit_slots_sender: &Sender<Slot>,
        progress: &mut ProgressMap,
    ) {
        let start_slot = poh_recorder.read().unwrap().start_slot();

        if let (false, Some(latest_leader_slot)) =
            progress.get_leader_propagation_slot_must_exist(start_slot)
        {
            debug!(
                "Slot not propagated: start_slot={} latest_leader_slot={}",
                start_slot, latest_leader_slot
            );
            Self::maybe_retransmit_unpropagated_slots(
                "replay_stage-retransmit-timing-based",
                retransmit_slots_sender,
                progress,
                latest_leader_slot,
            );
        }
    }

    fn is_partition_detected(
        ancestors: &HashMap<Slot, HashSet<Slot>>,
        last_voted_slot: Slot,
        heaviest_slot: Slot,
    ) -> bool {
        last_voted_slot != heaviest_slot
            && !ancestors
                .get(&heaviest_slot)
                .map(|ancestors| ancestors.contains(&last_voted_slot))
                .unwrap_or(true)
    }

    fn initialize_progress_and_fork_choice_with_locked_bank_forks(
        bank_forks: &RwLock<BankForks>,
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
        blockstore: &Blockstore,
    ) -> (ProgressMap, HeaviestSubtreeForkChoice) {
        let (root_bank, frozen_banks, duplicate_slot_hashes) = {
            let bank_forks = bank_forks.read().unwrap();
            let duplicate_slots = blockstore
                .duplicate_slots_iterator(bank_forks.root_bank().slot())
                .unwrap();
            let duplicate_slot_hashes = duplicate_slots.filter_map(|slot| {
                let bank = bank_forks.get(slot)?;
                bank.feature_set
                    .is_active(&feature_set::consume_blockstore_duplicate_proofs::id())
                    .then_some((slot, bank.hash()))
            });
            (
                bank_forks.root_bank(),
                bank_forks.frozen_banks().values().cloned().collect(),
                duplicate_slot_hashes.collect::<Vec<(Slot, Hash)>>(),
            )
        };

        Self::initialize_progress_and_fork_choice(
            &root_bank,
            frozen_banks,
            my_pubkey,
            vote_account,
            duplicate_slot_hashes,
        )
    }

    pub fn initialize_progress_and_fork_choice(
        root_bank: &Bank,
        mut frozen_banks: Vec<Arc<Bank>>,
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
        duplicate_slot_hashes: Vec<(Slot, Hash)>,
    ) -> (ProgressMap, HeaviestSubtreeForkChoice) {
        let mut progress = ProgressMap::default();

        frozen_banks.sort_by_key(|bank| bank.slot());

        // Initialize progress map with any root banks
        for bank in &frozen_banks {
            let prev_leader_slot = progress.get_bank_prev_leader_slot(bank);
            progress.insert(
                bank.slot(),
                ForkProgress::new_from_bank(bank, my_pubkey, vote_account, prev_leader_slot, 0, 0),
            );
        }
        let root = root_bank.slot();
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new_from_frozen_banks(
            (root, root_bank.hash()),
            &frozen_banks,
        );

        for slot_hash in duplicate_slot_hashes {
            heaviest_subtree_fork_choice.mark_fork_invalid_candidate(&slot_hash);
        }

        (progress, heaviest_subtree_fork_choice)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dump_then_repair_correct_slots(
        duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
        ancestors: &mut HashMap<Slot, HashSet<Slot>>,
        descendants: &mut HashMap<Slot, HashSet<Slot>>,
        progress: &mut ProgressMap,
        bank_forks: &RwLock<BankForks>,
        blockstore: &Blockstore,
        poh_bank_slot: Option<Slot>,
        purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
        dumped_slots_sender: &DumpedSlotsSender,
        my_pubkey: &Pubkey,
        leader_schedule_cache: &LeaderScheduleCache,
    ) {
        if duplicate_slots_to_repair.is_empty() {
            return;
        }

        let root_bank = bank_forks.read().unwrap().root_bank();
        let mut dumped = vec![];
        // TODO: handle if alternate version of descendant also got confirmed after ancestor was
        // confirmed, what happens then? Should probably keep track of dumped list and skip things
        // in `duplicate_slots_to_repair` that have already been dumped. Add test.
        duplicate_slots_to_repair.retain(|duplicate_slot, correct_hash| {
            // Should not dump duplicate slots if there is currently a poh bank building
            // on top of that slot, as BankingStage might still be referencing/touching that state
            // concurrently.
            // Luckily for us, because the fork choice rule removes duplicate slots from fork
            // choice, and this function is called after:
            // 1) We have picked a bank to reset to in `select_vote_and_reset_forks()`
            // 2) And also called `reset_poh_recorder()`
            // Then we should have reset to a fork that doesn't include the duplicate block,
            // which means any working bank in PohRecorder that was built on that duplicate fork
            // should have been cleared as well. However, if there is some violation of this guarantee,
            // then log here
            let is_poh_building_on_duplicate_fork = poh_bank_slot
                .map(|poh_bank_slot| {
                    ancestors
                        .get(&poh_bank_slot)
                        .expect("Poh bank should exist in BankForks and thus in ancestors map")
                        .contains(duplicate_slot)
                })
                .unwrap_or(false);

            let did_dump_repair = {
                if !is_poh_building_on_duplicate_fork {
                    let frozen_hash = bank_forks.read().unwrap().bank_hash(*duplicate_slot);
                    if let Some(frozen_hash) = frozen_hash {
                        if frozen_hash == *correct_hash {
                            warn!(
                                "Trying to dump slot {} with correct_hash {}",
                                *duplicate_slot, *correct_hash
                            );
                            return false;
                        } else if frozen_hash == Hash::default()
                            && !progress.is_dead(*duplicate_slot).expect(
                                "If slot exists in BankForks must exist in the progress map",
                            )
                        {
                            warn!(
                                "Trying to dump unfrozen slot {} that is not dead",
                                *duplicate_slot
                            );
                            return false;
                        }
                    } else {
                        warn!(
                            "Dumping slot {} which does not exist in bank forks (possibly pruned)",
                            *duplicate_slot
                        );
                    }

                    // Should not dump slots for which we were the leader
                    if Some(*my_pubkey) == leader_schedule_cache.slot_leader_at(*duplicate_slot, None) {
                        if let Some(bank) = bank_forks.read().unwrap().get(*duplicate_slot) {
                            bank_hash_details::write_bank_hash_details_file(&bank)
                                .map_err(|err| {
                                    warn!("Unable to write bank hash details file: {err}");
                                })
                                .ok();
                        } else {
                            warn!("Unable to get bank for slot {duplicate_slot} from bank forks");
                        }
                        panic!("We are attempting to dump a block that we produced. \
                            This indicates that we are producing duplicate blocks, \
                            or that there is a bug in our runtime/replay code which \
                            causes us to compute different bank hashes than the rest of the cluster. \
                            We froze slot {duplicate_slot} with hash {frozen_hash:?} while the cluster hash is {correct_hash}");
                    }

                    let attempt_no = purge_repair_slot_counter
                        .entry(*duplicate_slot)
                        .and_modify(|x| *x += 1)
                        .or_insert(1);
                    if *attempt_no > MAX_REPAIR_RETRY_LOOP_ATTEMPTS {
                        panic!("We have tried to repair duplicate slot: {duplicate_slot} more than {MAX_REPAIR_RETRY_LOOP_ATTEMPTS} times \
                            and are unable to freeze a block with bankhash {correct_hash}, \
                            instead we have a block with bankhash {frozen_hash:?}. \
                            This is most likely a bug in the runtime. \
                            At this point manual intervention is needed to make progress. Exiting");
                    }

                    Self::purge_unconfirmed_duplicate_slot(
                        *duplicate_slot,
                        ancestors,
                        descendants,
                        progress,
                        &root_bank,
                        bank_forks,
                        blockstore,
                    );

                    dumped.push((*duplicate_slot, *correct_hash));

                    warn!(
                        "Notifying repair service to repair duplicate slot: {}, attempt {}",
                        *duplicate_slot, *attempt_no,
                    );
                    true
                } else {
                    warn!(
                        "PoH bank for slot {} is building on duplicate slot {}",
                        poh_bank_slot.unwrap(),
                        duplicate_slot
                    );
                    false
                }
            };

            // If we dumped/repaired, then no need to keep the slot in the set of pending work
            !did_dump_repair
        });

        // Notify repair of the dumped slots along with the correct hash
        trace!("Dumped {} slots", dumped.len());
        dumped_slots_sender.send(dumped).unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn process_ancestor_hashes_duplicate_slots(
        pubkey: &Pubkey,
        blockstore: &Blockstore,
        ancestor_duplicate_slots_receiver: &AncestorDuplicateSlotsReceiver,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        progress: &ProgressMap,
        fork_choice: &mut HeaviestSubtreeForkChoice,
        bank_forks: &RwLock<BankForks>,
        duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
        ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
        purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    ) {
        let root = bank_forks.read().unwrap().root();
        for AncestorDuplicateSlotToRepair {
            slot_to_repair: (epoch_slots_frozen_slot, epoch_slots_frozen_hash),
            request_type,
        } in ancestor_duplicate_slots_receiver.try_iter()
        {
            warn!(
                "{} ReplayStage notified of duplicate slot from ancestor hashes service but we observed as {}: {:?}",
                pubkey, if request_type.is_pruned() {"pruned"} else {"dead"}, (epoch_slots_frozen_slot, epoch_slots_frozen_hash),
            );
            let epoch_slots_frozen_state = EpochSlotsFrozenState::new_from_state(
                epoch_slots_frozen_slot,
                epoch_slots_frozen_hash,
                gossip_duplicate_confirmed_slots,
                fork_choice,
                || progress.is_dead(epoch_slots_frozen_slot).unwrap_or(false),
                || {
                    bank_forks
                        .read()
                        .unwrap()
                        .get(epoch_slots_frozen_slot)
                        .map(|b| b.hash())
                },
                request_type.is_pruned(),
            );
            check_slot_agrees_with_cluster(
                epoch_slots_frozen_slot,
                root,
                blockstore,
                duplicate_slots_tracker,
                epoch_slots_frozen_slots,
                fork_choice,
                duplicate_slots_to_repair,
                ancestor_hashes_replay_update_sender,
                purge_repair_slot_counter,
                SlotStateUpdate::EpochSlotsFrozen(epoch_slots_frozen_state),
            );
        }
    }

    fn purge_unconfirmed_duplicate_slot(
        duplicate_slot: Slot,
        ancestors: &mut HashMap<Slot, HashSet<Slot>>,
        descendants: &mut HashMap<Slot, HashSet<Slot>>,
        progress: &mut ProgressMap,
        root_bank: &Bank,
        bank_forks: &RwLock<BankForks>,
        blockstore: &Blockstore,
    ) {
        warn!("purging slot {}", duplicate_slot);

        // Doesn't need to be root bank, just needs a common bank to
        // access the status cache and accounts
        let slot_descendants = descendants.get(&duplicate_slot).cloned();
        if slot_descendants.is_none() {
            // Root has already moved past this slot, no need to purge it
            if root_bank.slot() <= duplicate_slot {
                blockstore.clear_unconfirmed_slot(duplicate_slot);
            }

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

        // Grab the Slot and BankId's of the banks we need to purge, then clear the banks
        // from BankForks
        let (slots_to_purge, removed_banks): (Vec<(Slot, BankId)>, Vec<Arc<Bank>>) = {
            let mut w_bank_forks = bank_forks.write().unwrap();
            slot_descendants
                .iter()
                .chain(std::iter::once(&duplicate_slot))
                .map(|slot| {
                    // Clear the banks from BankForks
                    let bank = w_bank_forks
                        .remove(*slot)
                        .expect("BankForks should not have been purged yet");
                    bank_hash_details::write_bank_hash_details_file(&bank)
                        .map_err(|err| {
                            warn!("Unable to write bank hash details file: {err}");
                        })
                        .ok();
                    ((*slot, bank.bank_id()), bank)
                })
                .unzip()
        };

        // Clear the accounts for these slots so that any ongoing RPC scans fail.
        // These have to be atomically cleared together in the same batch, in order
        // to prevent RPC from seeing inconsistent results in scans.
        root_bank.remove_unrooted_slots(&slots_to_purge);

        // Once the slots above have been purged, now it's safe to remove the banks from
        // BankForks, allowing the Bank::drop() purging to run and not race with the
        // `remove_unrooted_slots()` call.
        drop(removed_banks);

        for (slot, slot_id) in slots_to_purge {
            // Clear the slot signatures from status cache for this slot.
            // TODO: What about RPC queries that had already cloned the Bank for this slot
            // and are looking up the signature for this slot?
            root_bank.clear_slot_signatures(slot);

            // Remove cached entries of the programs that were deployed in this slot.
            root_bank
                .loaded_programs_cache
                .write()
                .unwrap()
                .prune_by_deployment_slot(slot);

            if let Some(bank_hash) = blockstore.get_bank_hash(slot) {
                // If a descendant was successfully replayed and chained from a duplicate it must
                // also be a duplicate. In this case we *need* to repair it, so we clear from
                // blockstore.
                warn!(
                    "purging duplicate descendant: {} with slot_id {} and bank hash {}, of slot {}",
                    slot, slot_id, bank_hash, duplicate_slot
                );
                // Clear the slot-related data in blockstore. This will:
                // 1) Clear old shreds allowing new ones to be inserted
                // 2) Clear the "dead" flag allowing ReplayStage to start replaying
                // this slot
                blockstore.clear_unconfirmed_slot(slot);
            } else if slot == duplicate_slot {
                warn!("purging duplicate slot: {} with slot_id {}", slot, slot_id);
                blockstore.clear_unconfirmed_slot(slot);
            } else {
                // If a descendant was unable to replay and chained from a duplicate, it is not
                // necessary to repair it. It is most likely that this block is fine, and will
                // replay on successful repair of the parent. If this block is also a duplicate, it
                // will be handled in the next round of repair/replay - so we just clear the dead
                // flag for now.
                warn!("not purging descendant {} of slot {} as it is dead. resetting dead flag instead", slot, duplicate_slot);
                // Clear the "dead" flag allowing ReplayStage to start replaying
                // this slot once the parent is repaired
                blockstore.remove_dead_slot(slot).unwrap();
            }

            // Clear the progress map of these forks
            let _ = progress.remove(&slot);
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
                .get_mut(a)
                .expect("If exists in ancestor map must exist in descendants map")
                .retain(|d| *d != slot && !slot_descendants.contains(d));
        }
        ancestors
            .remove(&slot)
            .expect("must exist based on earlier check");

        // Purge all the descendants of this slot from both maps
        for descendant in slot_descendants {
            ancestors.remove(descendant).expect("must exist");
            descendants
                .remove(descendant)
                .expect("must exist based on earlier check");
        }
        descendants
            .remove(&slot)
            .expect("must exist based on earlier check");
    }

    #[allow(clippy::too_many_arguments)]
    fn process_popular_pruned_forks(
        popular_pruned_forks_receiver: &PopularPrunedForksReceiver,
        blockstore: &Blockstore,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        bank_forks: &RwLock<BankForks>,
        fork_choice: &mut HeaviestSubtreeForkChoice,
        duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
        ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
        purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    ) {
        let root = bank_forks.read().unwrap().root();
        for new_popular_pruned_slots in popular_pruned_forks_receiver.try_iter() {
            for new_popular_pruned_slot in new_popular_pruned_slots {
                if new_popular_pruned_slot <= root {
                    continue;
                }
                check_slot_agrees_with_cluster(
                    new_popular_pruned_slot,
                    root,
                    blockstore,
                    duplicate_slots_tracker,
                    epoch_slots_frozen_slots,
                    fork_choice,
                    duplicate_slots_to_repair,
                    ancestor_hashes_replay_update_sender,
                    purge_repair_slot_counter,
                    SlotStateUpdate::PopularPrunedFork,
                );
            }
        }
    }

    // Check for any newly confirmed slots by the cluster. This is only detects
    // optimistic and in the future, duplicate slot confirmations on the exact
    // single slots and does not account for votes on their descendants. Used solely
    // for duplicate slot recovery.
    #[allow(clippy::too_many_arguments)]
    fn process_gossip_duplicate_confirmed_slots(
        gossip_duplicate_confirmed_slots_receiver: &GossipDuplicateConfirmedSlotsReceiver,
        blockstore: &Blockstore,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &mut GossipDuplicateConfirmedSlots,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        bank_forks: &RwLock<BankForks>,
        progress: &ProgressMap,
        fork_choice: &mut HeaviestSubtreeForkChoice,
        duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
        ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
        purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    ) {
        let root = bank_forks.read().unwrap().root();
        for new_confirmed_slots in gossip_duplicate_confirmed_slots_receiver.try_iter() {
            for (confirmed_slot, duplicate_confirmed_hash) in new_confirmed_slots {
                if confirmed_slot <= root {
                    continue;
                } else if let Some(prev_hash) = gossip_duplicate_confirmed_slots
                    .insert(confirmed_slot, duplicate_confirmed_hash)
                {
                    assert_eq!(prev_hash, duplicate_confirmed_hash);
                    // Already processed this signal
                    return;
                }

                let duplicate_confirmed_state = DuplicateConfirmedState::new_from_state(
                    duplicate_confirmed_hash,
                    || progress.is_dead(confirmed_slot).unwrap_or(false),
                    || bank_forks.read().unwrap().bank_hash(confirmed_slot),
                );
                check_slot_agrees_with_cluster(
                    confirmed_slot,
                    root,
                    blockstore,
                    duplicate_slots_tracker,
                    epoch_slots_frozen_slots,
                    fork_choice,
                    duplicate_slots_to_repair,
                    ancestor_hashes_replay_update_sender,
                    purge_repair_slot_counter,
                    SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state),
                );
            }
        }
    }

    fn process_gossip_verified_vote_hashes(
        gossip_verified_vote_hash_receiver: &GossipVerifiedVoteHashReceiver,
        unfrozen_gossip_verified_vote_hashes: &mut UnfrozenGossipVerifiedVoteHashes,
        heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice,
        latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
    ) {
        for (pubkey, slot, hash) in gossip_verified_vote_hash_receiver.try_iter() {
            let is_frozen = heaviest_subtree_fork_choice.contains_block(&(slot, hash));
            // cluster_info_vote_listener will ensure it doesn't push duplicates
            unfrozen_gossip_verified_vote_hashes.add_vote(
                pubkey,
                slot,
                hash,
                is_frozen,
                latest_validator_votes_for_frozen_banks,
            )
        }
    }

    // Checks for and handle forks with duplicate slots.
    #[allow(clippy::too_many_arguments)]
    fn process_duplicate_slots(
        blockstore: &Blockstore,
        duplicate_slots_receiver: &DuplicateSlotReceiver,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        bank_forks: &RwLock<BankForks>,
        progress: &ProgressMap,
        fork_choice: &mut HeaviestSubtreeForkChoice,
        duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
        ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
        purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    ) {
        let new_duplicate_slots: Vec<Slot> = duplicate_slots_receiver.try_iter().collect();
        let (root_slot, bank_hashes) = {
            let r_bank_forks = bank_forks.read().unwrap();
            let bank_hashes: Vec<Option<Hash>> = new_duplicate_slots
                .iter()
                .map(|duplicate_slot| r_bank_forks.bank_hash(*duplicate_slot))
                .collect();

            (r_bank_forks.root(), bank_hashes)
        };
        for (duplicate_slot, bank_hash) in
            new_duplicate_slots.into_iter().zip(bank_hashes.into_iter())
        {
            // WindowService should only send the signal once per slot
            let duplicate_state = DuplicateState::new_from_state(
                duplicate_slot,
                gossip_duplicate_confirmed_slots,
                fork_choice,
                || progress.is_dead(duplicate_slot).unwrap_or(false),
                || bank_hash,
            );
            check_slot_agrees_with_cluster(
                duplicate_slot,
                root_slot,
                blockstore,
                duplicate_slots_tracker,
                epoch_slots_frozen_slots,
                fork_choice,
                duplicate_slots_to_repair,
                ancestor_hashes_replay_update_sender,
                purge_repair_slot_counter,
                SlotStateUpdate::Duplicate(duplicate_state),
            );
        }
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
        // Assume `NUM_CONSECUTIVE_LEADER_SLOTS` = 4. Then `skip_propagated_check`
        // below is true if `poh_slot` is within the same `NUM_CONSECUTIVE_LEADER_SLOTS`
        // set of blocks as `latest_leader_slot`.
        //
        // Example 1 (`poh_slot` directly descended from `latest_leader_slot`):
        //
        // [B B B B] [B B B latest_leader_slot] poh_slot
        //
        // Example 2:
        //
        // [B latest_leader_slot B poh_slot]
        //
        // In this example, even if there's a block `B` on another fork between
        // `poh_slot` and `parent_slot`, because they're in the same
        // `NUM_CONSECUTIVE_LEADER_SLOTS` block, we still skip the propagated
        // check because it's still within the propagation grace period.
        if let Some(latest_leader_slot) =
            progress_map.get_latest_leader_slot_must_exist(parent_slot)
        {
            let skip_propagated_check =
                poh_slot - latest_leader_slot < NUM_CONSECUTIVE_LEADER_SLOTS;
            if skip_propagated_check {
                return true;
            }
        }

        // Note that `is_propagated(parent_slot)` doesn't necessarily check
        // propagation of `parent_slot`, it checks propagation of the latest ancestor
        // of `parent_slot` (hence the call to `get_latest_leader_slot()` in the
        // check above)
        progress_map
            .get_leader_propagation_slot_must_exist(parent_slot)
            .0
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

    #[allow(clippy::too_many_arguments)]
    fn maybe_start_leader(
        my_pubkey: &Pubkey,
        bank_forks: &Arc<RwLock<BankForks>>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        rpc_subscriptions: &Arc<RpcSubscriptions>,
        progress_map: &mut ProgressMap,
        retransmit_slots_sender: &Sender<Slot>,
        skipped_slots_info: &mut SkippedSlotsInfo,
        banking_tracer: &Arc<BankingTracer>,
        has_new_vote_been_rooted: bool,
        track_transaction_indexes: bool,
    ) {
        // all the individual calls to poh_recorder.read() are designed to
        // increase granularity, decrease contention

        assert!(!poh_recorder.read().unwrap().has_bank());

        let (poh_slot, parent_slot) = match poh_recorder.read().unwrap().reached_leader_slot() {
            PohLeaderStatus::Reached {
                poh_slot,
                parent_slot,
            } => (poh_slot, parent_slot),
            PohLeaderStatus::NotReached => {
                trace!("{} poh_recorder hasn't reached_leader_slot", my_pubkey);
                return;
            }
        };

        trace!("{} reached_leader_slot", my_pubkey);

        let parent = bank_forks
            .read()
            .unwrap()
            .get(parent_slot)
            .expect("parent_slot doesn't exist in bank forks");

        assert!(parent.is_frozen());

        if !parent.is_startup_verification_complete() {
            info!("startup verification incomplete, so skipping my leader slot");
            return;
        }

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
            if !has_new_vote_been_rooted {
                info!("Haven't landed a vote, so skipping my leader slot");
                return;
            }

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
                let latest_unconfirmed_leader_slot = progress_map.get_latest_leader_slot_must_exist(parent_slot)
                    .expect("In order for propagated check to fail, latest leader must exist in progress map");
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
                if Self::should_retransmit(poh_slot, &mut skipped_slots_info.last_retransmit_slot) {
                    Self::maybe_retransmit_unpropagated_slots(
                        "replay_stage-retransmit",
                        retransmit_slots_sender,
                        progress_map,
                        latest_unconfirmed_leader_slot,
                    );
                }
                return;
            }

            let root_slot = bank_forks.read().unwrap().root();
            datapoint_info!("replay_stage-my_leader_slot", ("slot", poh_slot, i64),);
            info!(
                "new fork:{} parent:{} (leader) root:{}",
                poh_slot, parent_slot, root_slot
            );

            let root_distance = poh_slot - root_slot;
            let vote_only_bank = if root_distance > MAX_ROOT_DISTANCE_FOR_VOTE_ONLY {
                datapoint_info!("vote-only-bank", ("slot", poh_slot, i64));
                true
            } else {
                false
            };

            let tpu_bank = Self::new_bank_from_parent_with_notify(
                parent.clone(),
                poh_slot,
                root_slot,
                my_pubkey,
                rpc_subscriptions,
                NewBankOptions { vote_only_bank },
            );
            // make sure parent is frozen for finalized hashes via the above
            // new()-ing of its child bank
            banking_tracer.hash_event(parent.slot(), &parent.last_blockhash(), &parent.hash());

            let tpu_bank = bank_forks.write().unwrap().insert(tpu_bank);
            poh_recorder
                .write()
                .unwrap()
                .set_bank(tpu_bank, track_transaction_indexes);
        } else {
            error!("{} No next leader found", my_pubkey);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn replay_blockstore_into_bank(
        bank: &Arc<Bank>,
        blockstore: &Blockstore,
        replay_stats: &RwLock<ReplaySlotStats>,
        replay_progress: &RwLock<ConfirmationProgress>,
        transaction_status_sender: Option<&TransactionStatusSender>,
        entry_notification_sender: Option<&EntryNotifierSender>,
        replay_vote_sender: &ReplayVoteSender,
        verify_recyclers: &VerifyRecyclers,
        log_messages_bytes_limit: Option<usize>,
        prioritization_fee_cache: &PrioritizationFeeCache,
    ) -> result::Result<usize, BlockstoreProcessorError> {
        let mut w_replay_stats = replay_stats.write().unwrap();
        let mut w_replay_progress = replay_progress.write().unwrap();
        let tx_count_before = w_replay_progress.num_txs;
        // All errors must lead to marking the slot as dead, otherwise,
        // the `check_slot_agrees_with_cluster()` called by `replay_active_banks()`
        // will break!
        blockstore_processor::confirm_slot(
            blockstore,
            bank,
            &mut w_replay_stats,
            &mut w_replay_progress,
            false,
            transaction_status_sender,
            entry_notification_sender,
            Some(replay_vote_sender),
            verify_recyclers,
            false,
            log_messages_bytes_limit,
            prioritization_fee_cache,
        )?;
        let tx_count_after = w_replay_progress.num_txs;
        let tx_count = tx_count_after - tx_count_before;
        Ok(tx_count)
    }

    #[allow(clippy::too_many_arguments)]
    fn mark_dead_slot(
        blockstore: &Blockstore,
        bank: &Bank,
        root: Slot,
        err: &BlockstoreProcessorError,
        rpc_subscriptions: &Arc<RpcSubscriptions>,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        progress: &mut ProgressMap,
        heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
        duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
        ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
        purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    ) {
        // Do not remove from progress map when marking dead! Needed by
        // `process_gossip_duplicate_confirmed_slots()`

        // Block producer can abandon the block if it detects a better one
        // while producing. Somewhat common and expected in a
        // network with variable network/machine configuration.
        let is_serious = !matches!(
            err,
            BlockstoreProcessorError::InvalidBlock(BlockError::TooFewTicks)
        );
        let slot = bank.slot();
        if is_serious {
            datapoint_error!(
                "replay-stage-mark_dead_slot",
                ("error", format!("error: {err:?}"), String),
                ("slot", slot, i64)
            );
        } else {
            datapoint_info!(
                "replay-stage-mark_dead_slot",
                ("error", format!("error: {err:?}"), String),
                ("slot", slot, i64)
            );
        }
        progress.get_mut(&slot).unwrap().is_dead = true;
        blockstore
            .set_dead_slot(slot)
            .expect("Failed to mark slot as dead in blockstore");

        blockstore.slots_stats.mark_dead(slot);

        rpc_subscriptions.notify_slot_update(SlotUpdate::Dead {
            slot,
            err: format!("error: {err:?}"),
            timestamp: timestamp(),
        });
        let dead_state = DeadState::new_from_state(
            slot,
            duplicate_slots_tracker,
            gossip_duplicate_confirmed_slots,
            heaviest_subtree_fork_choice,
            epoch_slots_frozen_slots,
        );
        check_slot_agrees_with_cluster(
            slot,
            root,
            blockstore,
            duplicate_slots_tracker,
            epoch_slots_frozen_slots,
            heaviest_subtree_fork_choice,
            duplicate_slots_to_repair,
            ancestor_hashes_replay_update_sender,
            purge_repair_slot_counter,
            SlotStateUpdate::Dead(dead_state),
        );

        // If we previously marked this slot as duplicate in blockstore, let the state machine know
        if bank
            .feature_set
            .is_active(&feature_set::consume_blockstore_duplicate_proofs::id())
            && !duplicate_slots_tracker.contains(&slot)
            && blockstore.get_duplicate_slot(slot).is_some()
        {
            let duplicate_state = DuplicateState::new_from_state(
                slot,
                gossip_duplicate_confirmed_slots,
                heaviest_subtree_fork_choice,
                || true,
                || None,
            );
            check_slot_agrees_with_cluster(
                slot,
                root,
                blockstore,
                duplicate_slots_tracker,
                epoch_slots_frozen_slots,
                heaviest_subtree_fork_choice,
                duplicate_slots_to_repair,
                ancestor_hashes_replay_update_sender,
                purge_repair_slot_counter,
                SlotStateUpdate::Duplicate(duplicate_state),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_votable_bank(
        bank: &Arc<Bank>,
        switch_fork_decision: &SwitchForkDecision,
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &mut Tower,
        progress: &mut ProgressMap,
        vote_account_pubkey: &Pubkey,
        identity_keypair: &Keypair,
        authorized_voter_keypairs: &[Arc<Keypair>],
        blockstore: &Blockstore,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        lockouts_sender: &Sender<CommitmentAggregationData>,
        accounts_background_request_sender: &AbsRequestSender,
        latest_root_senders: &[Sender<Slot>],
        rpc_subscriptions: &Arc<RpcSubscriptions>,
        block_commitment_cache: &Arc<RwLock<BlockCommitmentCache>>,
        heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
        bank_notification_sender: &Option<BankNotificationSenderConfig>,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &mut GossipDuplicateConfirmedSlots,
        unfrozen_gossip_verified_vote_hashes: &mut UnfrozenGossipVerifiedVoteHashes,
        vote_signatures: &mut Vec<Signature>,
        has_new_vote_been_rooted: &mut bool,
        replay_timing: &mut ReplayTiming,
        voting_sender: &Sender<VoteOp>,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        drop_bank_sender: &Sender<Vec<Arc<Bank>>>,
        wait_to_vote_slot: Option<Slot>,
    ) {
        if bank.is_empty() {
            datapoint_info!("replay_stage-voted_empty_bank", ("slot", bank.slot(), i64));
        }
        trace!("handle votable bank {}", bank.slot());
        let new_root = tower.record_bank_vote(bank);

        if let Some(new_root) = new_root {
            // get the root bank before squash
            let root_bank = bank_forks
                .read()
                .unwrap()
                .get(new_root)
                .expect("Root bank doesn't exist");
            let mut rooted_banks = root_bank.parents();
            let oldest_parent = rooted_banks.last().map(|last| last.parent_slot());
            rooted_banks.push(root_bank.clone());
            let rooted_slots: Vec<_> = rooted_banks.iter().map(|bank| bank.slot()).collect();
            // The following differs from  rooted_slots by including the parent slot of the oldest parent bank.
            let rooted_slots_with_parents = bank_notification_sender
                .as_ref()
                .map_or(false, |sender| sender.should_send_parents)
                .then(|| {
                    let mut new_chain = rooted_slots.clone();
                    new_chain.push(oldest_parent.unwrap_or_else(|| bank.parent_slot()));
                    new_chain
                });

            // Call leader schedule_cache.set_root() before blockstore.set_root() because
            // bank_forks.root is consumed by repair_service to update gossip, so we don't want to
            // get shreds for repair on gossip before we update leader schedule, otherwise they may
            // get dropped.
            leader_schedule_cache.set_root(rooted_banks.last().unwrap());
            blockstore
                .set_roots(rooted_slots.iter())
                .expect("Ledger set roots failed");
            let highest_super_majority_root = Some(
                block_commitment_cache
                    .read()
                    .unwrap()
                    .highest_super_majority_root(),
            );
            Self::handle_new_root(
                new_root,
                bank_forks,
                progress,
                accounts_background_request_sender,
                highest_super_majority_root,
                heaviest_subtree_fork_choice,
                duplicate_slots_tracker,
                gossip_duplicate_confirmed_slots,
                unfrozen_gossip_verified_vote_hashes,
                has_new_vote_been_rooted,
                vote_signatures,
                epoch_slots_frozen_slots,
                drop_bank_sender,
            );

            blockstore.slots_stats.mark_rooted(new_root);

            rpc_subscriptions.notify_roots(rooted_slots);
            if let Some(sender) = bank_notification_sender {
                sender
                    .sender
                    .send(BankNotification::NewRootBank(root_bank))
                    .unwrap_or_else(|err| warn!("bank_notification_sender failed: {:?}", err));

                if let Some(new_chain) = rooted_slots_with_parents {
                    sender
                        .sender
                        .send(BankNotification::NewRootedChain(new_chain))
                        .unwrap_or_else(|err| warn!("bank_notification_sender failed: {:?}", err));
                }
            }
            latest_root_senders.iter().for_each(|s| {
                if let Err(e) = s.send(new_root) {
                    trace!("latest root send failed: {:?}", e);
                }
            });
            info!("new root {}", new_root);
        }

        let mut update_commitment_cache_time = Measure::start("update_commitment_cache");
        Self::update_commitment_cache(
            bank.clone(),
            bank_forks.read().unwrap().root(),
            progress.get_fork_stats(bank.slot()).unwrap().total_stake,
            lockouts_sender,
        );
        update_commitment_cache_time.stop();
        replay_timing.update_commitment_cache_us += update_commitment_cache_time.as_us();

        Self::push_vote(
            bank,
            vote_account_pubkey,
            identity_keypair,
            authorized_voter_keypairs,
            tower,
            switch_fork_decision,
            vote_signatures,
            *has_new_vote_been_rooted,
            replay_timing,
            voting_sender,
            wait_to_vote_slot,
        );
    }

    fn generate_vote_tx(
        node_keypair: &Keypair,
        bank: &Bank,
        vote_account_pubkey: &Pubkey,
        authorized_voter_keypairs: &[Arc<Keypair>],
        vote: VoteTransaction,
        switch_fork_decision: &SwitchForkDecision,
        vote_signatures: &mut Vec<Signature>,
        has_new_vote_been_rooted: bool,
        wait_to_vote_slot: Option<Slot>,
    ) -> Option<Transaction> {
        if !bank.is_startup_verification_complete() {
            info!("startup verification incomplete, so unable to vote");
            return None;
        }

        if authorized_voter_keypairs.is_empty() {
            return None;
        }
        if let Some(slot) = wait_to_vote_slot {
            if bank.slot() < slot {
                return None;
            }
        }
        let vote_account = match bank.get_vote_account(vote_account_pubkey) {
            None => {
                warn!(
                    "Vote account {} does not exist.  Unable to vote",
                    vote_account_pubkey,
                );
                return None;
            }
            Some(vote_account) => vote_account,
        };
        let vote_state = vote_account.vote_state();
        let vote_state = match vote_state.as_ref() {
            Err(_) => {
                warn!(
                    "Vote account {} is unreadable.  Unable to vote",
                    vote_account_pubkey,
                );
                return None;
            }
            Ok(vote_state) => vote_state,
        };

        if vote_state.node_pubkey != node_keypair.pubkey() {
            info!(
                "Vote account node_pubkey mismatch: {} (expected: {}).  Unable to vote",
                vote_state.node_pubkey,
                node_keypair.pubkey()
            );
            return None;
        }

        let Some(authorized_voter_pubkey) = vote_state.get_authorized_voter(bank.epoch()) else {
            warn!(
                "Vote account {} has no authorized voter for epoch {}.  Unable to vote",
                vote_account_pubkey,
                bank.epoch()
            );
            return None;
        };

        let authorized_voter_keypair = match authorized_voter_keypairs
            .iter()
            .find(|keypair| keypair.pubkey() == authorized_voter_pubkey)
        {
            None => {
                warn!("The authorized keypair {} for vote account {} is not available.  Unable to vote",
                      authorized_voter_pubkey, vote_account_pubkey);
                return None;
            }
            Some(authorized_voter_keypair) => authorized_voter_keypair,
        };

        // Send our last few votes along with the new one
        // Compact the vote state update before sending
        let vote = match vote {
            VoteTransaction::VoteStateUpdate(vote_state_update) => {
                VoteTransaction::CompactVoteStateUpdate(vote_state_update)
            }
            vote => vote,
        };
        let vote_ix = switch_fork_decision
            .to_vote_instruction(
                vote,
                vote_account_pubkey,
                &authorized_voter_keypair.pubkey(),
            )
            .expect("Switch threshold failure should not lead to voting");

        let mut vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&node_keypair.pubkey()));

        let blockhash = bank.last_blockhash();
        vote_tx.partial_sign(&[node_keypair], blockhash);
        vote_tx.partial_sign(&[authorized_voter_keypair.as_ref()], blockhash);

        if !has_new_vote_been_rooted {
            vote_signatures.push(vote_tx.signatures[0]);
            if vote_signatures.len() > MAX_VOTE_SIGNATURES {
                vote_signatures.remove(0);
            }
        } else {
            vote_signatures.clear();
        }

        Some(vote_tx)
    }

    #[allow(clippy::too_many_arguments)]
    fn refresh_last_vote(
        tower: &mut Tower,
        heaviest_bank_on_same_fork: &Bank,
        my_latest_landed_vote: Slot,
        vote_account_pubkey: &Pubkey,
        identity_keypair: &Keypair,
        authorized_voter_keypairs: &[Arc<Keypair>],
        vote_signatures: &mut Vec<Signature>,
        has_new_vote_been_rooted: bool,
        last_vote_refresh_time: &mut LastVoteRefreshTime,
        voting_sender: &Sender<VoteOp>,
        wait_to_vote_slot: Option<Slot>,
    ) {
        let last_voted_slot = tower.last_voted_slot();
        if last_voted_slot.is_none() {
            return;
        }

        // Refresh the vote if our latest vote hasn't landed, and the recent blockhash of the
        // last attempt at a vote transaction has expired
        let last_voted_slot = last_voted_slot.unwrap();
        if my_latest_landed_vote > last_voted_slot
            && last_vote_refresh_time.last_print_time.elapsed().as_secs() >= 1
        {
            last_vote_refresh_time.last_print_time = Instant::now();
            info!(
                "Last landed vote for slot {} in bank {} is greater than the current last vote for slot: {} tracked by Tower",
                my_latest_landed_vote,
                heaviest_bank_on_same_fork.slot(),
                last_voted_slot
            );
        }

        // If we are a non voting validator or have an incorrect setup preventing us from
        // generating vote txs, no need to refresh
        let Some(last_vote_tx_blockhash) = tower.last_vote_tx_blockhash() else {
            return;
        };

        if my_latest_landed_vote >= last_voted_slot
            || heaviest_bank_on_same_fork
                .is_hash_valid_for_age(&last_vote_tx_blockhash, MAX_PROCESSING_AGE)
            || {
                // In order to avoid voting on multiple forks all past MAX_PROCESSING_AGE that don't
                // include the last voted blockhash
                last_vote_refresh_time
                    .last_refresh_time
                    .elapsed()
                    .as_millis()
                    < MAX_VOTE_REFRESH_INTERVAL_MILLIS as u128
            }
        {
            return;
        }

        // Update timestamp for refreshed vote
        tower.refresh_last_vote_timestamp(heaviest_bank_on_same_fork.slot());

        let vote_tx = Self::generate_vote_tx(
            identity_keypair,
            heaviest_bank_on_same_fork,
            vote_account_pubkey,
            authorized_voter_keypairs,
            tower.last_vote(),
            &SwitchForkDecision::SameFork,
            vote_signatures,
            has_new_vote_been_rooted,
            wait_to_vote_slot,
        );

        if let Some(vote_tx) = vote_tx {
            let recent_blockhash = vote_tx.message.recent_blockhash;
            tower.refresh_last_vote_tx_blockhash(recent_blockhash);

            // Send the votes to the TPU and gossip for network propagation
            let hash_string = format!("{recent_blockhash}");
            datapoint_info!(
                "refresh_vote",
                ("last_voted_slot", last_voted_slot, i64),
                ("target_bank_slot", heaviest_bank_on_same_fork.slot(), i64),
                ("target_bank_hash", hash_string, String),
            );
            voting_sender
                .send(VoteOp::RefreshVote {
                    tx: vote_tx,
                    last_voted_slot,
                })
                .unwrap_or_else(|err| warn!("Error: {:?}", err));
            last_vote_refresh_time.last_refresh_time = Instant::now();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_vote(
        bank: &Bank,
        vote_account_pubkey: &Pubkey,
        identity_keypair: &Keypair,
        authorized_voter_keypairs: &[Arc<Keypair>],
        tower: &mut Tower,
        switch_fork_decision: &SwitchForkDecision,
        vote_signatures: &mut Vec<Signature>,
        has_new_vote_been_rooted: bool,
        replay_timing: &mut ReplayTiming,
        voting_sender: &Sender<VoteOp>,
        wait_to_vote_slot: Option<Slot>,
    ) {
        let mut generate_time = Measure::start("generate_vote");
        let vote_tx = Self::generate_vote_tx(
            identity_keypair,
            bank,
            vote_account_pubkey,
            authorized_voter_keypairs,
            tower.last_vote(),
            switch_fork_decision,
            vote_signatures,
            has_new_vote_been_rooted,
            wait_to_vote_slot,
        );
        generate_time.stop();
        replay_timing.generate_vote_us += generate_time.as_us();
        if let Some(vote_tx) = vote_tx {
            tower.refresh_last_vote_tx_blockhash(vote_tx.message.recent_blockhash);

            let saved_tower = SavedTower::new(tower, identity_keypair).unwrap_or_else(|err| {
                error!("Unable to create saved tower: {:?}", err);
                std::process::exit(1);
            });

            let tower_slots = tower.tower_slots();
            voting_sender
                .send(VoteOp::PushVote {
                    tx: vote_tx,
                    tower_slots,
                    saved_tower: SavedTowerVersions::from(saved_tower),
                })
                .unwrap_or_else(|err| warn!("Error: {:?}", err));
        }
    }

    fn update_commitment_cache(
        bank: Arc<Bank>,
        root: Slot,
        total_stake: Stake,
        lockouts_sender: &Sender<CommitmentAggregationData>,
    ) {
        if let Err(e) =
            lockouts_sender.send(CommitmentAggregationData::new(bank, root, total_stake))
        {
            trace!("lockouts_sender failed: {:?}", e);
        }
    }

    fn reset_poh_recorder(
        my_pubkey: &Pubkey,
        blockstore: &Blockstore,
        bank: Arc<Bank>,
        poh_recorder: &RwLock<PohRecorder>,
        leader_schedule_cache: &LeaderScheduleCache,
    ) {
        let slot = bank.slot();
        let tick_height = bank.tick_height();

        let next_leader_slot = leader_schedule_cache.next_leader_slot(
            my_pubkey,
            slot,
            &bank,
            Some(blockstore),
            GRACE_TICKS_FACTOR * MAX_GRACE_SLOTS,
        );

        poh_recorder.write().unwrap().reset(bank, next_leader_slot);

        let next_leader_msg = if let Some(next_leader_slot) = next_leader_slot {
            format!("My next leader slot is {}", next_leader_slot.0)
        } else {
            "I am not in the leader schedule yet".to_owned()
        };
        info!(
            "{my_pubkey} reset PoH to tick {tick_height} (within slot {slot}). {next_leader_msg}",
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn replay_active_banks_concurrently(
        blockstore: &Blockstore,
        bank_forks: &RwLock<BankForks>,
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
        progress: &mut ProgressMap,
        transaction_status_sender: Option<&TransactionStatusSender>,
        entry_notification_sender: Option<&EntryNotifierSender>,
        verify_recyclers: &VerifyRecyclers,
        replay_vote_sender: &ReplayVoteSender,
        replay_timing: &mut ReplayTiming,
        log_messages_bytes_limit: Option<usize>,
        active_bank_slots: &[Slot],
        prioritization_fee_cache: &PrioritizationFeeCache,
    ) -> Vec<ReplaySlotFromBlockstore> {
        // Make mutable shared structures thread safe.
        let progress = RwLock::new(progress);
        let longest_replay_time_us = AtomicU64::new(0);

        // Allow for concurrent replaying of slots from different forks.
        let replay_result_vec: Vec<ReplaySlotFromBlockstore> = PAR_THREAD_POOL.install(|| {
            active_bank_slots
                .into_par_iter()
                .map(|bank_slot| {
                    let bank_slot = *bank_slot;
                    let mut replay_result = ReplaySlotFromBlockstore {
                        is_slot_dead: false,
                        bank_slot,
                        replay_result: None,
                    };
                    let my_pubkey = &my_pubkey.clone();
                    trace!(
                        "Replay active bank: slot {}, thread_idx {}",
                        bank_slot,
                        PAR_THREAD_POOL.current_thread_index().unwrap_or_default()
                    );
                    let mut progress_lock = progress.write().unwrap();
                    if progress_lock
                        .get(&bank_slot)
                        .map(|p| p.is_dead)
                        .unwrap_or(false)
                    {
                        // If the fork was marked as dead, don't replay it
                        debug!("bank_slot {:?} is marked dead", bank_slot);
                        replay_result.is_slot_dead = true;
                        return replay_result;
                    }

                    let bank = bank_forks.read().unwrap().get(bank_slot).unwrap();
                    let parent_slot = bank.parent_slot();
                    let (num_blocks_on_fork, num_dropped_blocks_on_fork) = {
                        let stats = progress_lock
                            .get(&parent_slot)
                            .expect("parent of active bank must exist in progress map");
                        let num_blocks_on_fork = stats.num_blocks_on_fork + 1;
                        let new_dropped_blocks = bank.slot() - parent_slot - 1;
                        let num_dropped_blocks_on_fork =
                            stats.num_dropped_blocks_on_fork + new_dropped_blocks;
                        (num_blocks_on_fork, num_dropped_blocks_on_fork)
                    };
                    let prev_leader_slot = progress_lock.get_bank_prev_leader_slot(&bank);

                    let bank_progress = progress_lock.entry(bank.slot()).or_insert_with(|| {
                        ForkProgress::new_from_bank(
                            &bank,
                            my_pubkey,
                            &vote_account.clone(),
                            prev_leader_slot,
                            num_blocks_on_fork,
                            num_dropped_blocks_on_fork,
                        )
                    });

                    let replay_stats = bank_progress.replay_stats.clone();
                    let replay_progress = bank_progress.replay_progress.clone();
                    drop(progress_lock);

                    if bank.collector_id() != my_pubkey {
                        let mut replay_blockstore_time =
                            Measure::start("replay_blockstore_into_bank");
                        let blockstore_result = Self::replay_blockstore_into_bank(
                            &bank,
                            blockstore,
                            &replay_stats,
                            &replay_progress,
                            transaction_status_sender,
                            entry_notification_sender,
                            &replay_vote_sender.clone(),
                            &verify_recyclers.clone(),
                            log_messages_bytes_limit,
                            prioritization_fee_cache,
                        );
                        replay_blockstore_time.stop();
                        replay_result.replay_result = Some(blockstore_result);
                        longest_replay_time_us
                            .fetch_max(replay_blockstore_time.as_us(), Ordering::Relaxed);
                    }
                    replay_result
                })
                .collect()
        });
        // Accumulating time across all slots could inflate this number and make it seem like an
        // overly large amount of time is being spent on blockstore compared to other activities.
        replay_timing.replay_blockstore_us += longest_replay_time_us.load(Ordering::Relaxed);

        replay_result_vec
    }

    #[allow(clippy::too_many_arguments)]
    fn replay_active_bank(
        blockstore: &Blockstore,
        bank_forks: &RwLock<BankForks>,
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
        progress: &mut ProgressMap,
        transaction_status_sender: Option<&TransactionStatusSender>,
        entry_notification_sender: Option<&EntryNotifierSender>,
        verify_recyclers: &VerifyRecyclers,
        replay_vote_sender: &ReplayVoteSender,
        replay_timing: &mut ReplayTiming,
        log_messages_bytes_limit: Option<usize>,
        bank_slot: Slot,
        prioritization_fee_cache: &PrioritizationFeeCache,
    ) -> ReplaySlotFromBlockstore {
        let mut replay_result = ReplaySlotFromBlockstore {
            is_slot_dead: false,
            bank_slot,
            replay_result: None,
        };
        let my_pubkey = &my_pubkey.clone();
        trace!("Replay active bank: slot {}", bank_slot);
        if progress.get(&bank_slot).map(|p| p.is_dead).unwrap_or(false) {
            // If the fork was marked as dead, don't replay it
            debug!("bank_slot {:?} is marked dead", bank_slot);
            replay_result.is_slot_dead = true;
        } else {
            let bank = bank_forks.read().unwrap().get(bank_slot).unwrap();
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

            let bank_progress = progress.entry(bank.slot()).or_insert_with(|| {
                ForkProgress::new_from_bank(
                    &bank,
                    my_pubkey,
                    &vote_account.clone(),
                    prev_leader_slot,
                    num_blocks_on_fork,
                    num_dropped_blocks_on_fork,
                )
            });

            if bank.collector_id() != my_pubkey {
                let mut replay_blockstore_time = Measure::start("replay_blockstore_into_bank");
                let blockstore_result = Self::replay_blockstore_into_bank(
                    &bank,
                    blockstore,
                    &bank_progress.replay_stats,
                    &bank_progress.replay_progress,
                    transaction_status_sender,
                    entry_notification_sender,
                    &replay_vote_sender.clone(),
                    &verify_recyclers.clone(),
                    log_messages_bytes_limit,
                    prioritization_fee_cache,
                );
                replay_blockstore_time.stop();
                replay_result.replay_result = Some(blockstore_result);
                replay_timing.replay_blockstore_us += replay_blockstore_time.as_us();
            }
        }
        replay_result
    }

    #[allow(clippy::too_many_arguments)]
    fn process_replay_results(
        blockstore: &Blockstore,
        bank_forks: &RwLock<BankForks>,
        progress: &mut ProgressMap,
        transaction_status_sender: Option<&TransactionStatusSender>,
        cache_block_meta_sender: Option<&CacheBlockMetaSender>,
        heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
        bank_notification_sender: &Option<BankNotificationSenderConfig>,
        rewards_recorder_sender: &Option<RewardsRecorderSender>,
        rpc_subscriptions: &Arc<RpcSubscriptions>,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        unfrozen_gossip_verified_vote_hashes: &mut UnfrozenGossipVerifiedVoteHashes,
        latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
        cluster_slots_update_sender: &ClusterSlotsUpdateSender,
        cost_update_sender: &Sender<CostUpdate>,
        duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
        ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
        block_metadata_notifier: Option<BlockMetadataNotifierLock>,
        replay_result_vec: &[ReplaySlotFromBlockstore],
        purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    ) -> bool {
        // TODO: See if processing of blockstore replay results and bank completion can be made thread safe.
        let mut did_complete_bank = false;
        let mut tx_count = 0;
        let mut execute_timings = ExecuteTimings::default();
        for replay_result in replay_result_vec {
            if replay_result.is_slot_dead {
                continue;
            }

            let bank_slot = replay_result.bank_slot;
            let bank = &bank_forks.read().unwrap().get(bank_slot).unwrap();
            if let Some(replay_result) = &replay_result.replay_result {
                match replay_result {
                    Ok(replay_tx_count) => tx_count += replay_tx_count,
                    Err(err) => {
                        // Error means the slot needs to be marked as dead
                        Self::mark_dead_slot(
                            blockstore,
                            bank,
                            bank_forks.read().unwrap().root(),
                            err,
                            rpc_subscriptions,
                            duplicate_slots_tracker,
                            gossip_duplicate_confirmed_slots,
                            epoch_slots_frozen_slots,
                            progress,
                            heaviest_subtree_fork_choice,
                            duplicate_slots_to_repair,
                            ancestor_hashes_replay_update_sender,
                            purge_repair_slot_counter,
                        );
                        // If the bank was corrupted, don't try to run the below logic to check if the
                        // bank is completed
                        continue;
                    }
                }
            }

            assert_eq!(bank_slot, bank.slot());
            if bank.is_complete() {
                let mut bank_complete_time = Measure::start("bank_complete_time");
                let bank_progress = progress
                    .get_mut(&bank.slot())
                    .expect("Bank fork progress entry missing for completed bank");

                let replay_stats = bank_progress.replay_stats.clone();
                let r_replay_stats = replay_stats.read().unwrap();
                let replay_progress = bank_progress.replay_progress.clone();
                let r_replay_progress = replay_progress.read().unwrap();
                debug!(
                    "bank {} has completed replay from blockstore, \
                     contribute to update cost with {:?}",
                    bank.slot(),
                    r_replay_stats.batch_execute.totals
                );
                did_complete_bank = true;
                let _ = cluster_slots_update_sender.send(vec![bank_slot]);
                if let Some(transaction_status_sender) = transaction_status_sender {
                    transaction_status_sender.send_transaction_status_freeze_message(bank);
                }
                bank.freeze();
                datapoint_info!(
                    "bank_frozen",
                    ("slot", bank_slot, i64),
                    ("hash", bank.hash().to_string(), String),
                );
                // report cost tracker stats
                cost_update_sender
                    .send(CostUpdate::FrozenBank { bank: bank.clone() })
                    .unwrap_or_else(|err| {
                        warn!("cost_update_sender failed sending bank stats: {:?}", err)
                    });

                assert_ne!(bank.hash(), Hash::default());
                // Needs to be updated before `check_slot_agrees_with_cluster()` so that
                // any updates in `check_slot_agrees_with_cluster()` on fork choice take
                // effect
                heaviest_subtree_fork_choice.add_new_leaf_slot(
                    (bank.slot(), bank.hash()),
                    Some((bank.parent_slot(), bank.parent_hash())),
                );
                bank_progress.fork_stats.bank_hash = Some(bank.hash());
                let bank_frozen_state = BankFrozenState::new_from_state(
                    bank.slot(),
                    bank.hash(),
                    duplicate_slots_tracker,
                    gossip_duplicate_confirmed_slots,
                    heaviest_subtree_fork_choice,
                    epoch_slots_frozen_slots,
                );
                check_slot_agrees_with_cluster(
                    bank.slot(),
                    bank_forks.read().unwrap().root(),
                    blockstore,
                    duplicate_slots_tracker,
                    epoch_slots_frozen_slots,
                    heaviest_subtree_fork_choice,
                    duplicate_slots_to_repair,
                    ancestor_hashes_replay_update_sender,
                    purge_repair_slot_counter,
                    SlotStateUpdate::BankFrozen(bank_frozen_state),
                );
                // If we previously marked this slot as duplicate in blockstore, let the state machine know
                if bank
                    .feature_set
                    .is_active(&feature_set::consume_blockstore_duplicate_proofs::id())
                    && !duplicate_slots_tracker.contains(&bank.slot())
                    && blockstore.get_duplicate_slot(bank.slot()).is_some()
                {
                    let duplicate_state = DuplicateState::new_from_state(
                        bank.slot(),
                        gossip_duplicate_confirmed_slots,
                        heaviest_subtree_fork_choice,
                        || false,
                        || Some(bank.hash()),
                    );
                    check_slot_agrees_with_cluster(
                        bank.slot(),
                        bank_forks.read().unwrap().root(),
                        blockstore,
                        duplicate_slots_tracker,
                        epoch_slots_frozen_slots,
                        heaviest_subtree_fork_choice,
                        duplicate_slots_to_repair,
                        ancestor_hashes_replay_update_sender,
                        purge_repair_slot_counter,
                        SlotStateUpdate::Duplicate(duplicate_state),
                    );
                }
                if let Some(sender) = bank_notification_sender {
                    sender
                        .sender
                        .send(BankNotification::Frozen(bank.clone()))
                        .unwrap_or_else(|err| warn!("bank_notification_sender failed: {:?}", err));
                }
                blockstore_processor::cache_block_meta(bank, cache_block_meta_sender);

                let bank_hash = bank.hash();
                if let Some(new_frozen_voters) =
                    unfrozen_gossip_verified_vote_hashes.remove_slot_hash(bank.slot(), &bank_hash)
                {
                    for pubkey in new_frozen_voters {
                        latest_validator_votes_for_frozen_banks.check_add_vote(
                            pubkey,
                            bank.slot(),
                            Some(bank_hash),
                            false,
                        );
                    }
                }
                Self::record_rewards(bank, rewards_recorder_sender);
                if let Some(ref block_metadata_notifier) = block_metadata_notifier {
                    let block_metadata_notifier = block_metadata_notifier.read().unwrap();
                    let parent_blockhash = bank
                        .parent()
                        .map(|bank| bank.last_blockhash())
                        .unwrap_or_default();
                    block_metadata_notifier.notify_block_metadata(
                        bank.parent_slot(),
                        &parent_blockhash.to_string(),
                        bank.slot(),
                        &bank.last_blockhash().to_string(),
                        &bank.rewards,
                        Some(bank.clock().unix_timestamp),
                        Some(bank.block_height()),
                        bank.executed_transaction_count(),
                        r_replay_progress.num_entries as u64,
                    )
                }
                bank_complete_time.stop();

                r_replay_stats.report_stats(
                    bank.slot(),
                    r_replay_progress.num_txs,
                    r_replay_progress.num_entries,
                    r_replay_progress.num_shreds,
                    bank_complete_time.as_us(),
                );
                execute_timings.accumulate(&r_replay_stats.batch_execute.totals);
            } else {
                trace!(
                    "bank {} not completed tick_height: {}, max_tick_height: {}",
                    bank.slot(),
                    bank.tick_height(),
                    bank.max_tick_height()
                );
            }
        }

        did_complete_bank
    }

    #[allow(clippy::too_many_arguments)]
    fn replay_active_banks(
        blockstore: &Blockstore,
        bank_forks: &RwLock<BankForks>,
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
        progress: &mut ProgressMap,
        transaction_status_sender: Option<&TransactionStatusSender>,
        cache_block_meta_sender: Option<&CacheBlockMetaSender>,
        entry_notification_sender: Option<&EntryNotifierSender>,
        verify_recyclers: &VerifyRecyclers,
        heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
        replay_vote_sender: &ReplayVoteSender,
        bank_notification_sender: &Option<BankNotificationSenderConfig>,
        rewards_recorder_sender: &Option<RewardsRecorderSender>,
        rpc_subscriptions: &Arc<RpcSubscriptions>,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        unfrozen_gossip_verified_vote_hashes: &mut UnfrozenGossipVerifiedVoteHashes,
        latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
        cluster_slots_update_sender: &ClusterSlotsUpdateSender,
        cost_update_sender: &Sender<CostUpdate>,
        duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
        ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
        block_metadata_notifier: Option<BlockMetadataNotifierLock>,
        replay_timing: &mut ReplayTiming,
        log_messages_bytes_limit: Option<usize>,
        replay_slots_concurrently: bool,
        prioritization_fee_cache: &PrioritizationFeeCache,
        purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    ) -> bool /* completed a bank */ {
        let active_bank_slots = bank_forks.read().unwrap().active_bank_slots();
        let num_active_banks = active_bank_slots.len();
        trace!(
            "{} active bank(s) to replay: {:?}",
            num_active_banks,
            active_bank_slots
        );
        if num_active_banks > 0 {
            let replay_result_vec = if num_active_banks > 1 && replay_slots_concurrently {
                Self::replay_active_banks_concurrently(
                    blockstore,
                    bank_forks,
                    my_pubkey,
                    vote_account,
                    progress,
                    transaction_status_sender,
                    entry_notification_sender,
                    verify_recyclers,
                    replay_vote_sender,
                    replay_timing,
                    log_messages_bytes_limit,
                    &active_bank_slots,
                    prioritization_fee_cache,
                )
            } else {
                active_bank_slots
                    .iter()
                    .map(|bank_slot| {
                        Self::replay_active_bank(
                            blockstore,
                            bank_forks,
                            my_pubkey,
                            vote_account,
                            progress,
                            transaction_status_sender,
                            entry_notification_sender,
                            verify_recyclers,
                            replay_vote_sender,
                            replay_timing,
                            log_messages_bytes_limit,
                            *bank_slot,
                            prioritization_fee_cache,
                        )
                    })
                    .collect()
            };

            Self::process_replay_results(
                blockstore,
                bank_forks,
                progress,
                transaction_status_sender,
                cache_block_meta_sender,
                heaviest_subtree_fork_choice,
                bank_notification_sender,
                rewards_recorder_sender,
                rpc_subscriptions,
                duplicate_slots_tracker,
                gossip_duplicate_confirmed_slots,
                epoch_slots_frozen_slots,
                unfrozen_gossip_verified_vote_hashes,
                latest_validator_votes_for_frozen_banks,
                cluster_slots_update_sender,
                cost_update_sender,
                duplicate_slots_to_repair,
                ancestor_hashes_replay_update_sender,
                block_metadata_notifier,
                &replay_result_vec,
                purge_repair_slot_counter,
            )
        } else {
            false
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn compute_bank_stats(
        my_vote_pubkey: &Pubkey,
        ancestors: &HashMap<u64, HashSet<u64>>,
        frozen_banks: &mut [Arc<Bank>],
        tower: &mut Tower,
        progress: &mut ProgressMap,
        vote_tracker: &VoteTracker,
        cluster_slots: &ClusterSlots,
        bank_forks: &RwLock<BankForks>,
        heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
        latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
    ) -> Vec<Slot> {
        frozen_banks.sort_by_key(|bank| bank.slot());
        let mut new_stats = vec![];
        for bank in frozen_banks.iter() {
            let bank_slot = bank.slot();
            // Only time progress map should be missing a bank slot
            // is if this node was the leader for this slot as those banks
            // are not replayed in replay_active_banks()
            {
                let is_computed = progress
                    .get_fork_stats_mut(bank_slot)
                    .expect("All frozen banks must exist in the Progress map")
                    .computed;
                if !is_computed {
                    // Check if our tower is behind, if so (and the feature migration flag is in use)
                    // overwrite with the newer bank.
                    if let Some(vote_account) = bank.get_vote_account(my_vote_pubkey) {
                        if let Ok(mut bank_vote_state) = vote_account.vote_state().cloned() {
                            if bank_vote_state.last_voted_slot()
                                > tower.vote_state.last_voted_slot()
                            {
                                info!(
                                    "Frozen bank vote state slot {:?}
                                    is newer than our local vote state slot {:?},
                                    adopting the bank vote state as our own.
                                    Bank votes: {:?}, root: {:?},
                                    Local votes: {:?}, root: {:?}",
                                    bank_vote_state.last_voted_slot(),
                                    tower.vote_state.last_voted_slot(),
                                    bank_vote_state.votes,
                                    bank_vote_state.root_slot,
                                    tower.vote_state.votes,
                                    tower.vote_state.root_slot
                                );

                                if let Some(local_root) = tower.vote_state.root_slot {
                                    if bank_vote_state
                                        .root_slot
                                        .map(|bank_root| local_root > bank_root)
                                        .unwrap_or(true)
                                    {
                                        // If the local root is larger than this on chain vote state
                                        // root (possible due to supermajority roots being set on
                                        // startup), then we need to adjust the tower
                                        bank_vote_state.root_slot = Some(local_root);
                                        bank_vote_state
                                            .votes
                                            .retain(|lockout| lockout.slot() > local_root);
                                        info!(
                                            "Local root is larger than on chain root,
                                            overwrote bank root {:?} and updated votes {:?}",
                                            bank_vote_state.root_slot, bank_vote_state.votes
                                        );

                                        if let Some(first_vote) = bank_vote_state.votes.front() {
                                            assert!(ancestors
                                                .get(&first_vote.slot())
                                                .expect(
                                                    "Ancestors map must contain an
                                                        entry for all slots on this fork
                                                        greater than `local_root` and less
                                                        than `bank_slot`"
                                                )
                                                .contains(&local_root));
                                        }
                                    }
                                }

                                tower.vote_state.root_slot = bank_vote_state.root_slot;
                                tower.vote_state.votes = bank_vote_state.votes;

                                let last_voted_slot = tower.vote_state.last_voted_slot().unwrap_or(
                                    // If our local root is higher than the highest slot in `bank_vote_state` due to
                                    // supermajority roots, then it's expected that the vote state will be empty.
                                    // In this case we use the root as our last vote. This root cannot be None, because
                                    // `tower.vote_state.last_voted_slot()` is None only if `tower.vote_state.root_slot`
                                    // is Some.
                                    tower
                                        .vote_state
                                        .root_slot
                                        .expect("root_slot cannot be None here"),
                                );
                                // This is safe because `last_voted_slot` is now equal to
                                // `bank_vote_state.last_voted_slot()` or `local_root`.
                                // Since this vote state is contained in `bank`, which we have frozen,
                                // we must have frozen all slots contained in `bank_vote_state`,
                                // and by definition we must have frozen `local_root`.
                                //
                                // If `bank` is a duplicate, since we are able to replay it successfully, any slots
                                // in its vote state must also be part of the duplicate fork, and thus present in our
                                // progress map.
                                //
                                // Finally if both `bank` and `bank_vote_state.last_voted_slot()` are duplicate,
                                // we must have the compatible versions of both duplicates in order to replay `bank`
                                // successfully, so we are once again guaranteed that `bank_vote_state.last_voted_slot()`
                                // is present in progress map.
                                tower.update_last_vote_from_vote_state(
                                    progress
                                        .get_hash(last_voted_slot)
                                        .expect("Must exist for us to have frozen descendant"),
                                );
                                // Since we are updating our tower we need to update associated caches for previously computed
                                // slots as well.
                                for slot in frozen_banks.iter().map(|b| b.slot()) {
                                    Self::cache_tower_stats(progress, tower, slot, ancestors);
                                }
                            }
                        }
                    }
                    let computed_bank_state = Tower::collect_vote_lockouts(
                        my_vote_pubkey,
                        bank_slot,
                        &bank.vote_accounts(),
                        ancestors,
                        |slot| progress.get_hash(slot),
                        latest_validator_votes_for_frozen_banks,
                    );
                    // Notify any listeners of the votes found in this newly computed
                    // bank
                    heaviest_subtree_fork_choice.compute_bank_stats(
                        bank,
                        tower,
                        latest_validator_votes_for_frozen_banks,
                    );
                    let ComputedBankState {
                        voted_stakes,
                        total_stake,
                        lockout_intervals,
                        my_latest_landed_vote,
                        ..
                    } = computed_bank_state;
                    let stats = progress
                        .get_fork_stats_mut(bank_slot)
                        .expect("All frozen banks must exist in the Progress map");
                    stats.total_stake = total_stake;
                    stats.voted_stakes = voted_stakes;
                    stats.lockout_intervals = lockout_intervals;
                    stats.block_height = bank.block_height();
                    stats.my_latest_landed_vote = my_latest_landed_vote;
                    stats.computed = true;
                    new_stats.push(bank_slot);
                    datapoint_info!(
                        "bank_weight",
                        ("slot", bank_slot, i64),
                        // u128 too large for influx, convert to hex
                        ("weight", format!("{:X}", stats.weight), String),
                    );
                    info!(
                        "{} slot_weight: {} {} {} {}",
                        my_vote_pubkey,
                        bank_slot,
                        stats.weight,
                        stats.fork_weight,
                        bank.parent().map(|b| b.slot()).unwrap_or(0)
                    );
                }
            }

            Self::update_propagation_status(
                progress,
                bank_slot,
                bank_forks,
                vote_tracker,
                cluster_slots,
            );

            Self::cache_tower_stats(progress, tower, bank_slot, ancestors);
        }
        new_stats
    }

    fn cache_tower_stats(
        progress: &mut ProgressMap,
        tower: &Tower,
        slot: Slot,
        ancestors: &HashMap<u64, HashSet<u64>>,
    ) {
        let stats = progress
            .get_fork_stats_mut(slot)
            .expect("All frozen banks must exist in the Progress map");

        stats.vote_threshold =
            tower.check_vote_stake_threshold(slot, &stats.voted_stakes, stats.total_stake);
        stats.is_locked_out = tower.is_locked_out(
            slot,
            ancestors
                .get(&slot)
                .expect("Ancestors map should contain slot for is_locked_out() check"),
        );
        stats.has_voted = tower.has_voted(slot);
        stats.is_recent = tower.is_recent(slot);
    }

    fn update_propagation_status(
        progress: &mut ProgressMap,
        slot: Slot,
        bank_forks: &RwLock<BankForks>,
        vote_tracker: &VoteTracker,
        cluster_slots: &ClusterSlots,
    ) {
        // If propagation has already been confirmed, return
        if progress.get_leader_propagation_slot_must_exist(slot).0 {
            return;
        }

        // Otherwise we have to check the votes for confirmation
        let propagated_stats = progress
            .get_propagated_stats_mut(slot)
            .unwrap_or_else(|| panic!("slot={slot} must exist in ProgressMap"));

        if propagated_stats.slot_vote_tracker.is_none() {
            propagated_stats.slot_vote_tracker = vote_tracker.get_slot_vote_tracker(slot);
        }
        let slot_vote_tracker = propagated_stats.slot_vote_tracker.clone();

        if propagated_stats.cluster_slot_pubkeys.is_none() {
            propagated_stats.cluster_slot_pubkeys = cluster_slots.lookup(slot);
        }
        let cluster_slot_pubkeys = propagated_stats.cluster_slot_pubkeys.clone();

        let newly_voted_pubkeys = slot_vote_tracker
            .as_ref()
            .and_then(|slot_vote_tracker| {
                slot_vote_tracker.write().unwrap().get_voted_slot_updates()
            })
            .unwrap_or_default();

        let cluster_slot_pubkeys = cluster_slot_pubkeys
            .map(|v| v.read().unwrap().keys().cloned().collect())
            .unwrap_or_default();

        Self::update_fork_propagated_threshold_from_votes(
            progress,
            newly_voted_pubkeys,
            cluster_slot_pubkeys,
            slot,
            &bank_forks.read().unwrap(),
        );
    }

    fn select_forks_failed_switch_threshold(
        reset_bank: Option<&Bank>,
        progress: &ProgressMap,
        tower: &Tower,
        heaviest_bank_slot: Slot,
        failure_reasons: &mut Vec<HeaviestForkFailures>,
        switch_proof_stake: u64,
        total_stake: u64,
        switch_fork_decision: SwitchForkDecision,
    ) -> SwitchForkDecision {
        let last_vote_unable_to_land = match reset_bank {
            Some(heaviest_bank_on_same_voted_fork) => {
                match tower.last_voted_slot() {
                    Some(last_voted_slot) => {
                        match progress
                            .my_latest_landed_vote(heaviest_bank_on_same_voted_fork.slot())
                        {
                            Some(my_latest_landed_vote) =>
                            // Last vote did not land
                            {
                                my_latest_landed_vote < last_voted_slot
                                    // If we are already voting at the tip, there is nothing we can do.
                                    && last_voted_slot < heaviest_bank_on_same_voted_fork.slot()
                                    // Last vote outside slot hashes of the tip of fork
                                    && !heaviest_bank_on_same_voted_fork
                                        .is_in_slot_hashes_history(&last_voted_slot)
                            }
                            None => false,
                        }
                    }
                    None => false,
                }
            }
            None => false,
        };

        if last_vote_unable_to_land {
            // If we reach here, these assumptions are true:
            // 1. We can't switch because of threshold
            // 2. Our last vote was on a non-duplicate/confirmed slot
            // 3. Our last vote is now outside slot hashes history of the tip of fork
            // So, there was no hope of this last vote ever landing again.

            // In this case, we do want to obey threshold, yet try to register our vote on
            // the current fork, so we choose to vote at the tip of current fork instead.
            // This will not cause longer lockout because lockout doesn't double after 512
            // slots, it might be enough to get majority vote.
            SwitchForkDecision::SameFork
        } else {
            // If we can't switch and our last vote was on a non-duplicate/confirmed slot, then
            // reset to the the next votable bank on the same fork as our last vote,
            // but don't vote.

            // We don't just reset to the heaviest fork when switch threshold fails because
            // a situation like this can occur:

            /* Figure 1:
                        slot 0
                            |
                        slot 1
                        /        \
            slot 2 (last vote)     |
                        |      slot 8 (10%)
                slot 4 (9%)
            */

            // Imagine 90% of validators voted on slot 4, but only 9% landed. If everybody that fails
            // the switch threshold abandons slot 4 to build on slot 8 (because it's *currently* heavier),
            // then there will be no blocks to include the votes for slot 4, and the network halts
            // because 90% of validators can't vote
            info!(
                "Waiting to switch vote to {},
                resetting to slot {:?} for now,
                switch proof stake: {},
                threshold stake: {},
                total stake: {}",
                heaviest_bank_slot,
                reset_bank.as_ref().map(|b| b.slot()),
                switch_proof_stake,
                total_stake as f64 * SWITCH_FORK_THRESHOLD,
                total_stake
            );
            failure_reasons.push(HeaviestForkFailures::FailedSwitchThreshold(
                heaviest_bank_slot,
                switch_proof_stake,
                total_stake,
            ));
            switch_fork_decision
        }
    }

    /// Given a `heaviest_bank` and a `heaviest_bank_on_same_voted_fork`, return
    /// a bank to vote on, a bank to reset to, and a list of switch failure
    /// reasons.
    ///
    /// If `heaviest_bank_on_same_voted_fork` is `None` due to that fork no
    /// longer being valid to vote on, it's possible that a validator will not
    /// be able to reset away from the invalid fork that they last voted on. To
    /// resolve this scenario, validators need to wait until they can create a
    /// switch proof for another fork or until the invalid fork is be marked
    /// valid again if it was confirmed by the cluster.
    /// Until this is resolved, leaders will build each of their
    /// blocks from the last reset bank on the invalid fork.
    pub fn select_vote_and_reset_forks(
        heaviest_bank: &Arc<Bank>,
        // Should only be None if there was no previous vote
        heaviest_bank_on_same_voted_fork: Option<&Arc<Bank>>,
        ancestors: &HashMap<u64, HashSet<u64>>,
        descendants: &HashMap<u64, HashSet<u64>>,
        progress: &ProgressMap,
        tower: &mut Tower,
        latest_validator_votes_for_frozen_banks: &LatestValidatorVotesForFrozenBanks,
        fork_choice: &HeaviestSubtreeForkChoice,
    ) -> SelectVoteAndResetForkResult {
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
        //    switch_threshold succeeds
        let mut failure_reasons = vec![];
        struct CandidateVoteAndResetBanks<'a> {
            // A bank that the validator will vote on given it passes all
            // remaining vote checks
            candidate_vote_bank: Option<&'a Arc<Bank>>,
            // A bank that the validator will reset its PoH to regardless
            // of voting behavior
            reset_bank: Option<&'a Arc<Bank>>,
            switch_fork_decision: SwitchForkDecision,
        }
        let candidate_vote_and_reset_banks = {
            let switch_fork_decision: SwitchForkDecision = tower.check_switch_threshold(
                heaviest_bank.slot(),
                ancestors,
                descendants,
                progress,
                heaviest_bank.total_epoch_stake(),
                heaviest_bank
                    .epoch_vote_accounts(heaviest_bank.epoch())
                    .expect("Bank epoch vote accounts must contain entry for the bank's own epoch"),
                latest_validator_votes_for_frozen_banks,
                fork_choice,
            );

            match switch_fork_decision {
                SwitchForkDecision::FailedSwitchThreshold(switch_proof_stake, total_stake) => {
                    let final_switch_fork_decision = Self::select_forks_failed_switch_threshold(
                        heaviest_bank_on_same_voted_fork.map(|bank| bank.as_ref()),
                        progress,
                        tower,
                        heaviest_bank.slot(),
                        &mut failure_reasons,
                        switch_proof_stake,
                        total_stake,
                        switch_fork_decision,
                    );
                    let candidate_vote_bank = if final_switch_fork_decision.can_vote() {
                        // The only time we would still vote despite `!switch_fork_decision.can_vote()`
                        // is if we switched the vote candidate to `heaviest_bank_on_same_voted_fork`
                        // because we needed to refresh the vote to the tip of our last voted fork.
                        heaviest_bank_on_same_voted_fork
                    } else {
                        // Otherwise,  we should just return the original vote candidate, the heaviest bank
                        // for logging purposes, namely to check if there are any additional voting failures
                        // besides the switch threshold
                        Some(heaviest_bank)
                    };
                    CandidateVoteAndResetBanks {
                        candidate_vote_bank,
                        reset_bank: heaviest_bank_on_same_voted_fork,
                        switch_fork_decision: final_switch_fork_decision,
                    }
                }
                SwitchForkDecision::FailedSwitchDuplicateRollback(latest_duplicate_ancestor) => {
                    // If we can't switch and our last vote was on an unconfirmed, duplicate slot,
                    // then we need to reset to the heaviest bank, even if the heaviest bank is not
                    // a descendant of the last vote (usually for switch threshold failures we reset
                    // to the heaviest descendant of the last vote, but in this case, the last vote
                    // was on a duplicate branch). This is because in the case of *unconfirmed* duplicate
                    // slots, somebody needs to generate an alternative branch to escape a situation
                    // like a 50-50 split  where both partitions have voted on different versions of the
                    // same duplicate slot.

                    // Unlike the situation described in `Figure 1` above, this is safe. To see why,
                    // imagine the same situation described in Figure 1 above occurs, but slot 2 is
                    // a duplicate block. There are now a few cases:
                    //
                    // Note first that DUPLICATE_THRESHOLD + SWITCH_FORK_THRESHOLD + DUPLICATE_LIVENESS_THRESHOLD = 1;
                    //
                    // 1) > DUPLICATE_THRESHOLD of the network voted on some version of slot 2. Because duplicate slots can be confirmed
                    // by gossip, unlike the situation described in `Figure 1`, we don't need those
                    // votes to land in a descendant to confirm slot 2. Once slot 2 is confirmed by
                    // gossip votes, that fork is added back to the fork choice set and falls back into
                    // normal fork choice, which is covered by the `FailedSwitchThreshold` case above
                    // (everyone will resume building on their last voted fork, slot 4, since slot 8
                    // doesn't have for switch threshold)
                    //
                    // 2) <= DUPLICATE_THRESHOLD of the network voted on some version of slot 2, > SWITCH_FORK_THRESHOLD of the network voted
                    // on slot 8. Then everybody abandons the duplicate fork from fork choice and both builds
                    // on slot 8's fork. They can also vote on slot 8's fork because it has sufficient weight
                    // to pass the switching threshold
                    //
                    // 3) <= DUPLICATE_THRESHOLD of the network voted on some version of slot 2, <= SWITCH_FORK_THRESHOLD of the network voted
                    // on slot 8. This means more than DUPLICATE_LIVENESS_THRESHOLD of the network is gone, so we cannot
                    // guarantee progress anyways

                    // Note the heaviest fork is never descended from a known unconfirmed duplicate slot
                    // because the fork choice rule ensures that (marks it as an invalid candidate),
                    // thus it's safe to use as the reset bank.
                    let reset_bank = Some(heaviest_bank);
                    info!(
                        "Waiting to switch vote to {}, resetting to slot {:?} for now, latest duplicate ancestor: {:?}",
                        heaviest_bank.slot(),
                        reset_bank.as_ref().map(|b| b.slot()),
                        latest_duplicate_ancestor,
                    );
                    failure_reasons.push(HeaviestForkFailures::FailedSwitchThreshold(
                        heaviest_bank.slot(),
                        0, // In this case we never actually performed the switch check, 0 for now
                        0,
                    ));
                    CandidateVoteAndResetBanks {
                        candidate_vote_bank: None,
                        reset_bank,
                        switch_fork_decision,
                    }
                }
                _ => CandidateVoteAndResetBanks {
                    candidate_vote_bank: Some(heaviest_bank),
                    reset_bank: Some(heaviest_bank),
                    switch_fork_decision,
                },
            }
        };

        let CandidateVoteAndResetBanks {
            candidate_vote_bank,
            reset_bank,
            switch_fork_decision,
        } = candidate_vote_and_reset_banks;

        if let Some(candidate_vote_bank) = candidate_vote_bank {
            // If there's a bank to potentially vote on, then make the remaining
            // checks
            let (
                is_locked_out,
                vote_threshold,
                propagated_stake,
                is_leader_slot,
                fork_weight,
                total_threshold_stake,
                total_epoch_stake,
            ) = {
                let fork_stats = progress.get_fork_stats(candidate_vote_bank.slot()).unwrap();
                let propagated_stats = &progress
                    .get_propagated_stats(candidate_vote_bank.slot())
                    .unwrap();
                (
                    fork_stats.is_locked_out,
                    fork_stats.vote_threshold,
                    propagated_stats.propagated_validators_stake,
                    propagated_stats.is_leader_slot,
                    fork_stats.weight,
                    fork_stats.total_stake,
                    propagated_stats.total_epoch_stake,
                )
            };

            let propagation_confirmed = is_leader_slot
                || progress
                    .get_leader_propagation_slot_must_exist(candidate_vote_bank.slot())
                    .0;

            if is_locked_out {
                failure_reasons.push(HeaviestForkFailures::LockedOut(candidate_vote_bank.slot()));
            }
            if let ThresholdDecision::FailedThreshold(fork_stake) = vote_threshold {
                failure_reasons.push(HeaviestForkFailures::FailedThreshold(
                    candidate_vote_bank.slot(),
                    fork_stake,
                    total_threshold_stake,
                ));
            }
            if !propagation_confirmed {
                failure_reasons.push(HeaviestForkFailures::NoPropagatedConfirmation(
                    candidate_vote_bank.slot(),
                    propagated_stake,
                    total_epoch_stake,
                ));
            }

            if !is_locked_out
                && vote_threshold.passed()
                && propagation_confirmed
                && switch_fork_decision.can_vote()
            {
                info!("voting: {} {}", candidate_vote_bank.slot(), fork_weight);
                SelectVoteAndResetForkResult {
                    vote_bank: Some((candidate_vote_bank.clone(), switch_fork_decision)),
                    reset_bank: Some(candidate_vote_bank.clone()),
                    heaviest_fork_failures: failure_reasons,
                }
            } else {
                SelectVoteAndResetForkResult {
                    vote_bank: None,
                    reset_bank: reset_bank.cloned(),
                    heaviest_fork_failures: failure_reasons,
                }
            }
        } else if reset_bank.is_some() {
            SelectVoteAndResetForkResult {
                vote_bank: None,
                reset_bank: reset_bank.cloned(),
                heaviest_fork_failures: failure_reasons,
            }
        } else {
            SelectVoteAndResetForkResult {
                vote_bank: None,
                reset_bank: None,
                heaviest_fork_failures: failure_reasons,
            }
        }
    }

    fn update_fork_propagated_threshold_from_votes(
        progress: &mut ProgressMap,
        mut newly_voted_pubkeys: Vec<Pubkey>,
        mut cluster_slot_pubkeys: Vec<Pubkey>,
        fork_tip: Slot,
        bank_forks: &BankForks,
    ) {
        let mut current_leader_slot = progress.get_latest_leader_slot_must_exist(fork_tip);
        let mut did_newly_reach_threshold = false;
        let root = bank_forks.root();
        loop {
            // These cases mean confirmation of propagation on any earlier
            // leader blocks must have been reached
            if current_leader_slot.is_none() || current_leader_slot.unwrap() < root {
                break;
            }

            let leader_propagated_stats = progress
                .get_propagated_stats_mut(current_leader_slot.unwrap())
                .expect("current_leader_slot >= root, so must exist in the progress map");

            // If a descendant has reached propagation threshold, then
            // all its ancestor banks have also reached propagation
            // threshold as well (Validators can't have voted for a
            // descendant without also getting the ancestor block)
            if leader_propagated_stats.is_propagated || {
                // If there's no new validators to record, and there's no
                // newly achieved threshold, then there's no further
                // information to propagate backwards to past leader blocks
                newly_voted_pubkeys.is_empty()
                    && cluster_slot_pubkeys.is_empty()
                    && !did_newly_reach_threshold
            } {
                break;
            }

            // We only iterate through the list of leader slots by traversing
            // the linked list of 'prev_leader_slot`'s outlined in the
            // `progress` map
            assert!(leader_propagated_stats.is_leader_slot);
            let leader_bank = bank_forks
                .get(current_leader_slot.unwrap())
                .expect("Entry in progress map must exist in BankForks")
                .clone();

            did_newly_reach_threshold = Self::update_slot_propagated_threshold_from_votes(
                &mut newly_voted_pubkeys,
                &mut cluster_slot_pubkeys,
                &leader_bank,
                leader_propagated_stats,
                did_newly_reach_threshold,
            ) || did_newly_reach_threshold;

            // Now jump to process the previous leader slot
            current_leader_slot = leader_propagated_stats.prev_leader_slot;
        }
    }

    fn update_slot_propagated_threshold_from_votes(
        newly_voted_pubkeys: &mut Vec<Pubkey>,
        cluster_slot_pubkeys: &mut Vec<Pubkey>,
        leader_bank: &Bank,
        leader_propagated_stats: &mut PropagatedStats,
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
        // because they don't to be ported back any further because earlier
        // parents must have:
        // 1) Also recorded these pubkeys already, or
        // 2) Already reached the propagation threshold, in which case
        //    they no longer need to track the set of propagated validators
        newly_voted_pubkeys.retain(|vote_pubkey| {
            let exists = leader_propagated_stats
                .propagated_validators
                .contains(vote_pubkey);
            leader_propagated_stats.add_vote_pubkey(
                *vote_pubkey,
                leader_bank.epoch_vote_account_stake(vote_pubkey),
            );
            !exists
        });

        cluster_slot_pubkeys.retain(|node_pubkey| {
            let exists = leader_propagated_stats
                .propagated_node_ids
                .contains(node_pubkey);
            leader_propagated_stats.add_node_pubkey(node_pubkey, leader_bank);
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

    #[allow(clippy::too_many_arguments)]
    fn mark_slots_confirmed(
        confirmed_forks: &[(Slot, Hash)],
        blockstore: &Blockstore,
        bank_forks: &RwLock<BankForks>,
        progress: &mut ProgressMap,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        fork_choice: &mut HeaviestSubtreeForkChoice,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
        ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
        purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    ) {
        let root_slot = bank_forks.read().unwrap().root();
        for (slot, frozen_hash) in confirmed_forks.iter() {
            // This case should be guaranteed as false by confirm_forks()
            if let Some(false) = progress.is_supermajority_confirmed(*slot) {
                // Because supermajority confirmation will iterate through and update the
                // subtree in fork choice, only incur this cost if the slot wasn't already
                // confirmed
                progress.set_supermajority_confirmed_slot(*slot);
                // If the slot was confirmed, then it must be frozen. Otherwise, we couldn't
                // have replayed any of its descendants and figured out it was confirmed.
                assert!(*frozen_hash != Hash::default());

                let duplicate_confirmed_state = DuplicateConfirmedState::new_from_state(
                    *frozen_hash,
                    || false,
                    || Some(*frozen_hash),
                );
                check_slot_agrees_with_cluster(
                    *slot,
                    root_slot,
                    blockstore,
                    duplicate_slots_tracker,
                    epoch_slots_frozen_slots,
                    fork_choice,
                    duplicate_slots_to_repair,
                    ancestor_hashes_replay_update_sender,
                    purge_repair_slot_counter,
                    SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state),
                );
            }
        }
    }

    fn confirm_forks(
        tower: &Tower,
        voted_stakes: &VotedStakes,
        total_stake: Stake,
        progress: &ProgressMap,
        bank_forks: &RwLock<BankForks>,
    ) -> Vec<(Slot, Hash)> {
        let mut confirmed_forks = vec![];
        for (slot, prog) in progress.iter() {
            if !prog.fork_stats.is_supermajority_confirmed {
                let bank = bank_forks
                    .read()
                    .unwrap()
                    .get(*slot)
                    .expect("bank in progress must exist in BankForks")
                    .clone();
                let duration = prog
                    .replay_stats
                    .read()
                    .unwrap()
                    .started
                    .elapsed()
                    .as_millis();
                if bank.is_frozen() && tower.is_slot_confirmed(*slot, voted_stakes, total_stake) {
                    info!("validator fork confirmed {} {}ms", *slot, duration);
                    datapoint_info!("validator-confirmation", ("duration_ms", duration, i64));
                    confirmed_forks.push((*slot, bank.hash()));
                } else {
                    debug!(
                        "validator fork not confirmed {} {}ms {:?}",
                        *slot,
                        duration,
                        voted_stakes.get(slot)
                    );
                }
            }
        }
        confirmed_forks
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_new_root(
        new_root: Slot,
        bank_forks: &RwLock<BankForks>,
        progress: &mut ProgressMap,
        accounts_background_request_sender: &AbsRequestSender,
        highest_super_majority_root: Option<Slot>,
        heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
        duplicate_slots_tracker: &mut DuplicateSlotsTracker,
        gossip_duplicate_confirmed_slots: &mut GossipDuplicateConfirmedSlots,
        unfrozen_gossip_verified_vote_hashes: &mut UnfrozenGossipVerifiedVoteHashes,
        has_new_vote_been_rooted: &mut bool,
        voted_signatures: &mut Vec<Signature>,
        epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
        drop_bank_sender: &Sender<Vec<Arc<Bank>>>,
    ) {
        bank_forks.read().unwrap().prune_program_cache(new_root);
        let removed_banks = bank_forks.write().unwrap().set_root(
            new_root,
            accounts_background_request_sender,
            highest_super_majority_root,
        );

        drop_bank_sender
            .send(removed_banks)
            .unwrap_or_else(|err| warn!("bank drop failed: {:?}", err));

        // Dropping the bank_forks write lock and reacquiring as a read lock is
        // safe because updates to bank_forks are only made by a single thread.
        let r_bank_forks = bank_forks.read().unwrap();
        let new_root_bank = &r_bank_forks[new_root];
        if !*has_new_vote_been_rooted {
            for signature in voted_signatures.iter() {
                if new_root_bank.get_signature_status(signature).is_some() {
                    *has_new_vote_been_rooted = true;
                    break;
                }
            }
            if *has_new_vote_been_rooted {
                std::mem::take(voted_signatures);
            }
        }
        progress.handle_new_root(&r_bank_forks);
        heaviest_subtree_fork_choice.set_tree_root((new_root, r_bank_forks.root_bank().hash()));
        *duplicate_slots_tracker = duplicate_slots_tracker.split_off(&new_root);
        // duplicate_slots_tracker now only contains entries >= `new_root`

        *gossip_duplicate_confirmed_slots = gossip_duplicate_confirmed_slots.split_off(&new_root);
        // gossip_confirmed_slots now only contains entries >= `new_root`

        unfrozen_gossip_verified_vote_hashes.set_root(new_root);
        *epoch_slots_frozen_slots = epoch_slots_frozen_slots.split_off(&new_root);
        // epoch_slots_frozen_slots now only contains entries >= `new_root`
    }

    fn generate_new_bank_forks(
        blockstore: &Blockstore,
        bank_forks: &RwLock<BankForks>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        rpc_subscriptions: &Arc<RpcSubscriptions>,
        progress: &mut ProgressMap,
        replay_timing: &mut ReplayTiming,
    ) {
        // Find the next slot that chains to the old slot
        let mut generate_new_bank_forks_read_lock =
            Measure::start("generate_new_bank_forks_read_lock");
        let forks = bank_forks.read().unwrap();
        generate_new_bank_forks_read_lock.stop();

        let frozen_banks = forks.frozen_banks();
        let frozen_bank_slots: Vec<u64> = frozen_banks
            .keys()
            .cloned()
            .filter(|s| *s >= forks.root())
            .collect();
        let mut generate_new_bank_forks_get_slots_since =
            Measure::start("generate_new_bank_forks_get_slots_since");
        let next_slots = blockstore
            .get_slots_since(&frozen_bank_slots)
            .expect("Db error");
        generate_new_bank_forks_get_slots_since.stop();

        // Filter out what we've already seen
        trace!("generate new forks {:?}", {
            let mut next_slots = next_slots.iter().collect::<Vec<_>>();
            next_slots.sort();
            next_slots
        });
        let mut generate_new_bank_forks_loop = Measure::start("generate_new_bank_forks_loop");
        let mut new_banks = HashMap::new();
        for (parent_slot, children) in next_slots {
            let parent_bank = frozen_banks
                .get(&parent_slot)
                .expect("missing parent in bank forks");
            for child_slot in children {
                if forks.get(child_slot).is_some() || new_banks.get(&child_slot).is_some() {
                    trace!("child already active or frozen {}", child_slot);
                    continue;
                }
                let leader = leader_schedule_cache
                    .slot_leader_at(child_slot, Some(parent_bank))
                    .unwrap();
                info!(
                    "new fork:{} parent:{} root:{}",
                    child_slot,
                    parent_slot,
                    forks.root()
                );
                let child_bank = Self::new_bank_from_parent_with_notify(
                    parent_bank.clone(),
                    child_slot,
                    forks.root(),
                    &leader,
                    rpc_subscriptions,
                    NewBankOptions::default(),
                );
                let empty: Vec<Pubkey> = vec![];
                Self::update_fork_propagated_threshold_from_votes(
                    progress,
                    empty,
                    vec![leader],
                    parent_bank.slot(),
                    &forks,
                );
                new_banks.insert(child_slot, child_bank);
            }
        }
        drop(forks);
        generate_new_bank_forks_loop.stop();

        let mut generate_new_bank_forks_write_lock =
            Measure::start("generate_new_bank_forks_write_lock");
        let mut forks = bank_forks.write().unwrap();
        for (_, bank) in new_banks {
            forks.insert(bank);
        }
        generate_new_bank_forks_write_lock.stop();
        saturating_add_assign!(
            replay_timing.generate_new_bank_forks_read_lock_us,
            generate_new_bank_forks_read_lock.as_us()
        );
        saturating_add_assign!(
            replay_timing.generate_new_bank_forks_get_slots_since_us,
            generate_new_bank_forks_get_slots_since.as_us()
        );
        saturating_add_assign!(
            replay_timing.generate_new_bank_forks_loop_us,
            generate_new_bank_forks_loop.as_us()
        );
        saturating_add_assign!(
            replay_timing.generate_new_bank_forks_write_lock_us,
            generate_new_bank_forks_write_lock.as_us()
        );
    }

    fn new_bank_from_parent_with_notify(
        parent: Arc<Bank>,
        slot: u64,
        root_slot: u64,
        leader: &Pubkey,
        rpc_subscriptions: &Arc<RpcSubscriptions>,
        new_bank_options: NewBankOptions,
    ) -> Bank {
        rpc_subscriptions.notify_slot(slot, parent.slot(), root_slot);
        Bank::new_from_parent_with_options(parent, leader, slot, new_bank_options)
    }

    fn record_rewards(bank: &Bank, rewards_recorder_sender: &Option<RewardsRecorderSender>) {
        if let Some(rewards_recorder_sender) = rewards_recorder_sender {
            let rewards = bank.rewards.read().unwrap();
            if !rewards.is_empty() {
                rewards_recorder_sender
                    .send(RewardsMessage::Batch((bank.slot(), rewards.clone())))
                    .unwrap_or_else(|err| warn!("rewards_recorder_sender failed: {:?}", err));
            }
            rewards_recorder_sender
                .send(RewardsMessage::Complete(bank.slot()))
                .unwrap_or_else(|err| warn!("rewards_recorder_sender failed: {:?}", err));
        }
    }

    pub fn get_unlock_switch_vote_slot(cluster_type: ClusterType) -> Slot {
        match cluster_type {
            ClusterType::Development => 0,
            ClusterType::Devnet => 0,
            // Epoch 63
            ClusterType::Testnet => 21_692_256,
            // 400_000 slots into epoch 61
            ClusterType::MainnetBeta => 26_752_000,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.commitment_service.join()?;
        self.t_replay.join().map(|_| ())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use {
        super::*,
        crate::{
            consensus::{
                progress_map::{ValidatorStakeInfo, RETRANSMIT_BASE_DELAY_MS},
                tower_storage::NullTowerStorage,
                tree_diff::TreeDiff,
                Tower,
            },
            replay_stage::ReplayStage,
            vote_simulator::{self, VoteSimulator},
        },
        crossbeam_channel::unbounded,
        itertools::Itertools,
        solana_entry::entry::{self, Entry},
        solana_gossip::{cluster_info::Node, crds::Cursor},
        solana_ledger::{
            blockstore::{entries_to_test_shreds, make_slot_entries, BlockstoreError},
            create_new_tmp_ledger,
            genesis_utils::{create_genesis_config, create_genesis_config_with_leader},
            get_tmp_ledger_path,
            shred::{Shred, ShredFlags, LEGACY_SHRED_DATA_CAPACITY},
        },
        solana_rpc::{
            optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank,
            rpc::{create_test_transaction_entries, populate_blockstore_for_tests},
        },
        solana_runtime::{
            accounts_background_service::AbsRequestSender,
            commitment::BlockCommitment,
            genesis_utils::{GenesisConfigInfo, ValidatorVoteKeypairs},
        },
        solana_sdk::{
            clock::NUM_CONSECUTIVE_LEADER_SLOTS,
            genesis_config,
            hash::{hash, Hash},
            instruction::InstructionError,
            poh_config::PohConfig,
            signature::{Keypair, Signer},
            system_transaction,
            transaction::TransactionError,
        },
        solana_streamer::socket::SocketAddrSpace,
        solana_transaction_status::VersionedTransactionWithStatusMeta,
        solana_vote_program::{
            vote_state::{self, VoteStateVersions},
            vote_transaction,
        },
        std::{
            fs::remove_dir_all,
            iter,
            sync::{atomic::AtomicU64, Arc, RwLock},
        },
        trees::{tr, Tree},
    };

    #[test]
    fn test_is_partition_detected() {
        let (VoteSimulator { bank_forks, .. }, _) = setup_default_forks(1, None::<GenerateVotes>);
        let ancestors = bank_forks.read().unwrap().ancestors();
        // Last vote 1 is an ancestor of the heaviest slot 3, no partition
        assert!(!ReplayStage::is_partition_detected(&ancestors, 1, 3));
        // Last vote 1 is an ancestor of the from heaviest slot 1, no partition
        assert!(!ReplayStage::is_partition_detected(&ancestors, 3, 3));
        // Last vote 2 is not an ancestor of the heaviest slot 3,
        // partition detected!
        assert!(ReplayStage::is_partition_detected(&ancestors, 2, 3));
        // Last vote 4 is not an ancestor of the heaviest slot 3,
        // partition detected!
        assert!(ReplayStage::is_partition_detected(&ancestors, 4, 3));
    }

    pub struct ReplayBlockstoreComponents {
        pub blockstore: Arc<Blockstore>,
        validator_node_to_vote_keys: HashMap<Pubkey, Pubkey>,
        pub(crate) my_pubkey: Pubkey,
        cluster_info: ClusterInfo,
        pub(crate) leader_schedule_cache: Arc<LeaderScheduleCache>,
        poh_recorder: RwLock<PohRecorder>,
        tower: Tower,
        rpc_subscriptions: Arc<RpcSubscriptions>,
        pub vote_simulator: VoteSimulator,
    }

    pub fn replay_blockstore_components(
        forks: Option<Tree<Slot>>,
        num_validators: usize,
        generate_votes: Option<GenerateVotes>,
    ) -> ReplayBlockstoreComponents {
        // Setup blockstore
        let (vote_simulator, blockstore) = setup_forks_from_tree(
            forks.unwrap_or_else(|| tr(0)),
            num_validators,
            generate_votes,
        );

        let VoteSimulator {
            ref validator_keypairs,
            ref bank_forks,
            ..
        } = vote_simulator;

        let blockstore = Arc::new(blockstore);
        let validator_node_to_vote_keys: HashMap<Pubkey, Pubkey> = validator_keypairs
            .iter()
            .map(|(_, keypairs)| {
                (
                    keypairs.node_keypair.pubkey(),
                    keypairs.vote_keypair.pubkey(),
                )
            })
            .collect();

        // ClusterInfo
        let my_keypairs = validator_keypairs.values().next().unwrap();
        let my_pubkey = my_keypairs.node_keypair.pubkey();
        let cluster_info = ClusterInfo::new(
            Node::new_localhost_with_pubkey(&my_pubkey).info,
            Arc::new(my_keypairs.node_keypair.insecure_clone()),
            SocketAddrSpace::Unspecified,
        );
        assert_eq!(my_pubkey, cluster_info.id());

        // Leader schedule cache
        let root_bank = bank_forks.read().unwrap().root_bank();
        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&root_bank));

        // PohRecorder
        let working_bank = bank_forks.read().unwrap().working_bank();
        let poh_recorder = RwLock::new(
            PohRecorder::new(
                working_bank.tick_height(),
                working_bank.last_blockhash(),
                working_bank.clone(),
                None,
                working_bank.ticks_per_slot(),
                &Pubkey::default(),
                blockstore.clone(),
                &leader_schedule_cache,
                &PohConfig::default(),
                Arc::new(AtomicBool::new(false)),
            )
            .0,
        );

        // Tower
        let my_vote_pubkey = my_keypairs.vote_keypair.pubkey();
        let tower = Tower::new_from_bankforks(
            &bank_forks.read().unwrap(),
            &cluster_info.id(),
            &my_vote_pubkey,
        );

        // RpcSubscriptions
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(bank_forks);
        let exit = Arc::new(AtomicBool::new(false));
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            exit,
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::default())),
            optimistically_confirmed_bank,
        ));

        ReplayBlockstoreComponents {
            blockstore,
            validator_node_to_vote_keys,
            my_pubkey,
            cluster_info,
            leader_schedule_cache,
            poh_recorder,
            tower,
            rpc_subscriptions,
            vote_simulator,
        }
    }

    #[test]
    fn test_child_slots_of_same_parent() {
        let ReplayBlockstoreComponents {
            blockstore,
            validator_node_to_vote_keys,
            vote_simulator,
            leader_schedule_cache,
            rpc_subscriptions,
            ..
        } = replay_blockstore_components(None, 1, None::<GenerateVotes>);

        let VoteSimulator {
            mut progress,
            bank_forks,
            ..
        } = vote_simulator;

        // Insert a non-root bank so that the propagation logic will update this
        // bank
        let bank1 = Bank::new_from_parent(
            bank_forks.read().unwrap().get(0).unwrap(),
            &leader_schedule_cache.slot_leader_at(1, None).unwrap(),
            1,
        );
        progress.insert(
            1,
            ForkProgress::new_from_bank(
                &bank1,
                bank1.collector_id(),
                validator_node_to_vote_keys
                    .get(bank1.collector_id())
                    .unwrap(),
                Some(0),
                0,
                0,
            ),
        );
        assert!(progress.get_propagated_stats(1).unwrap().is_leader_slot);
        bank1.freeze();
        bank_forks.write().unwrap().insert(bank1);

        // Insert shreds for slot NUM_CONSECUTIVE_LEADER_SLOTS,
        // chaining to slot 1
        let (shreds, _) = make_slot_entries(
            NUM_CONSECUTIVE_LEADER_SLOTS, // slot
            1,                            // parent_slot
            8,                            // num_entries
            true,                         // merkle_variant
        );
        blockstore.insert_shreds(shreds, None, false).unwrap();
        assert!(bank_forks
            .read()
            .unwrap()
            .get(NUM_CONSECUTIVE_LEADER_SLOTS)
            .is_none());
        let mut replay_timing = ReplayTiming::default();
        ReplayStage::generate_new_bank_forks(
            &blockstore,
            &bank_forks,
            &leader_schedule_cache,
            &rpc_subscriptions,
            &mut progress,
            &mut replay_timing,
        );
        assert!(bank_forks
            .read()
            .unwrap()
            .get(NUM_CONSECUTIVE_LEADER_SLOTS)
            .is_some());

        // Insert shreds for slot 2 * NUM_CONSECUTIVE_LEADER_SLOTS,
        // chaining to slot 1
        let (shreds, _) = make_slot_entries(
            2 * NUM_CONSECUTIVE_LEADER_SLOTS,
            1,
            8,
            true, // merkle_variant
        );
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
            &rpc_subscriptions,
            &mut progress,
            &mut replay_timing,
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

        // // There are 20 equally staked accounts, of which 3 have built
        // banks above or at bank 1. Because 3/20 < SUPERMINORITY_THRESHOLD,
        // we should see 3 validators in bank 1's propagated_validator set.
        let expected_leader_slots = vec![
            1,
            NUM_CONSECUTIVE_LEADER_SLOTS,
            2 * NUM_CONSECUTIVE_LEADER_SLOTS,
        ];
        for slot in expected_leader_slots {
            let leader = leader_schedule_cache.slot_leader_at(slot, None).unwrap();
            let vote_key = validator_node_to_vote_keys.get(&leader).unwrap();
            assert!(progress
                .get_propagated_stats(1)
                .unwrap()
                .propagated_validators
                .contains(vote_key));
        }
    }

    #[test]
    fn test_handle_new_root() {
        let genesis_config = create_genesis_config(10_000).genesis_config;
        let bank0 = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank0);

        let root = 3;
        let root_bank = Bank::new_from_parent(
            bank_forks.read().unwrap().get(0).unwrap(),
            &Pubkey::default(),
            root,
        );
        root_bank.freeze();
        let root_hash = root_bank.hash();
        bank_forks.write().unwrap().insert(root_bank);

        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new((root, root_hash));

        let mut progress = ProgressMap::default();
        for i in 0..=root {
            progress.insert(i, ForkProgress::new(Hash::default(), None, None, 0, 0));
        }

        let mut duplicate_slots_tracker: DuplicateSlotsTracker =
            vec![root - 1, root, root + 1].into_iter().collect();
        let mut gossip_duplicate_confirmed_slots: GossipDuplicateConfirmedSlots =
            vec![root - 1, root, root + 1]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect();
        let mut unfrozen_gossip_verified_vote_hashes: UnfrozenGossipVerifiedVoteHashes =
            UnfrozenGossipVerifiedVoteHashes {
                votes_per_slot: vec![root - 1, root, root + 1]
                    .into_iter()
                    .map(|s| (s, HashMap::new()))
                    .collect(),
            };
        let mut epoch_slots_frozen_slots: EpochSlotsFrozenSlots = vec![root - 1, root, root + 1]
            .into_iter()
            .map(|slot| (slot, Hash::default()))
            .collect();
        let (drop_bank_sender, _drop_bank_receiver) = unbounded();
        ReplayStage::handle_new_root(
            root,
            &bank_forks,
            &mut progress,
            &AbsRequestSender::default(),
            None,
            &mut heaviest_subtree_fork_choice,
            &mut duplicate_slots_tracker,
            &mut gossip_duplicate_confirmed_slots,
            &mut unfrozen_gossip_verified_vote_hashes,
            &mut true,
            &mut Vec::new(),
            &mut epoch_slots_frozen_slots,
            &drop_bank_sender,
        );
        assert_eq!(bank_forks.read().unwrap().root(), root);
        assert_eq!(progress.len(), 1);
        assert!(progress.get(&root).is_some());
        // root - 1 is filtered out
        assert_eq!(
            duplicate_slots_tracker.into_iter().collect::<Vec<Slot>>(),
            vec![root, root + 1]
        );
        assert_eq!(
            gossip_duplicate_confirmed_slots
                .keys()
                .cloned()
                .collect::<Vec<Slot>>(),
            vec![root, root + 1]
        );
        assert_eq!(
            unfrozen_gossip_verified_vote_hashes
                .votes_per_slot
                .keys()
                .cloned()
                .collect::<Vec<Slot>>(),
            vec![root, root + 1]
        );
        assert_eq!(
            epoch_slots_frozen_slots.into_keys().collect::<Vec<Slot>>(),
            vec![root, root + 1]
        );
    }

    #[test]
    fn test_handle_new_root_ahead_of_highest_super_majority_root() {
        let genesis_config = create_genesis_config(10_000).genesis_config;
        let bank0 = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank0);
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
        root_bank.freeze();
        let root_hash = root_bank.hash();
        bank_forks.write().unwrap().insert(root_bank);
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new((root, root_hash));
        let mut progress = ProgressMap::default();
        for i in 0..=root {
            progress.insert(i, ForkProgress::new(Hash::default(), None, None, 0, 0));
        }
        let (drop_bank_sender, _drop_bank_receiver) = unbounded();
        ReplayStage::handle_new_root(
            root,
            &bank_forks,
            &mut progress,
            &AbsRequestSender::default(),
            Some(confirmed_root),
            &mut heaviest_subtree_fork_choice,
            &mut DuplicateSlotsTracker::default(),
            &mut GossipDuplicateConfirmedSlots::default(),
            &mut UnfrozenGossipVerifiedVoteHashes::default(),
            &mut true,
            &mut Vec::new(),
            &mut EpochSlotsFrozenSlots::default(),
            &drop_bank_sender,
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
            entries_to_test_shreds(
                &[entry],
                slot,
                slot.saturating_sub(1), // parent_slot
                false,                  // is_full_slot
                0,                      // version
                true,                   // merkle_variant
            )
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
                    genesis_keypair,
                    &keypair2.pubkey(),
                    2,
                    blockhash,
                )],
            );
            entries_to_test_shreds(
                &[entry],
                slot,
                slot.saturating_sub(1), // parent_slot
                false,                  // is_full_slot
                0,                      // version
                true,                   // merkle_variant
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidEntryHash);
        } else {
            panic!();
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
                &[too_few_hashes_tick],
                slot,
                slot.saturating_sub(1),
                false,
                0,
                true, // merkle_variant
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidTickHashCount);
        } else {
            panic!();
        }
    }

    #[test]
    fn test_dead_fork_invalid_slot_tick_count() {
        solana_logger::setup();
        // Too many ticks per slot
        let res = check_dead_fork(|_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            entries_to_test_shreds(
                &entry::create_ticks(bank.ticks_per_slot() + 1, hashes_per_tick, blockhash),
                slot,
                slot.saturating_sub(1),
                false,
                0,
                true, // merkle_variant
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::TooManyTicks);
        } else {
            panic!();
        }

        // Too few ticks per slot
        let res = check_dead_fork(|_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            entries_to_test_shreds(
                &entry::create_ticks(bank.ticks_per_slot() - 1, hashes_per_tick, blockhash),
                slot,
                slot.saturating_sub(1),
                true,
                0,
                true, // merkle_variant
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::TooFewTicks);
        } else {
            panic!();
        }
    }

    #[test]
    fn test_dead_fork_invalid_last_tick() {
        let res = check_dead_fork(|_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            entries_to_test_shreds(
                &entry::create_ticks(bank.ticks_per_slot(), hashes_per_tick, blockhash),
                slot,
                slot.saturating_sub(1),
                false,
                0,
                true, // merkle_variant
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::InvalidLastTick);
        } else {
            panic!();
        }
    }

    #[test]
    fn test_dead_fork_trailing_entry() {
        let keypair = Keypair::new();
        let res = check_dead_fork(|funded_keypair, bank| {
            let blockhash = bank.last_blockhash();
            let slot = bank.slot();
            let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
            let mut entries =
                entry::create_ticks(bank.ticks_per_slot(), hashes_per_tick, blockhash);
            let last_entry_hash = entries.last().unwrap().hash;
            let tx = system_transaction::transfer(funded_keypair, &keypair.pubkey(), 2, blockhash);
            let trailing_entry = entry::next_entry(&last_entry_hash, 1, vec![tx]);
            entries.push(trailing_entry);
            entries_to_test_shreds(
                &entries,
                slot,
                slot.saturating_sub(1), // parent_slot
                true,                   // is_full_slot
                0,                      // version
                true,                   // merkle_variant
            )
        });

        if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
            assert_eq!(block_error, BlockError::TrailingEntry);
        } else {
            panic!();
        }
    }

    #[test]
    fn test_dead_fork_entry_deserialize_failure() {
        // Insert entry that causes deserialization failure
        let res = check_dead_fork(|_, bank| {
            let gibberish = [0xa5u8; LEGACY_SHRED_DATA_CAPACITY];
            let parent_offset = bank.slot() - bank.parent_slot();
            let shred = Shred::new_from_data(
                bank.slot(),
                0, // index,
                parent_offset as u16,
                &gibberish,
                ShredFlags::DATA_COMPLETE_SHRED,
                0, // reference_tick
                0, // version
                0, // fec_set_index
            );
            vec![shred]
        });

        assert_matches!(
            res,
            Err(BlockstoreProcessorError::FailedToLoadEntries(
                BlockstoreError::InvalidShredData(_)
            ),)
        );
    }

    // Given a shred and a fatal expected error, check that replaying that shred causes causes the fork to be
    // marked as dead. Returns the error for caller to verify.
    fn check_dead_fork<F>(shred_to_insert: F) -> result::Result<(), BlockstoreProcessorError>
    where
        F: Fn(&Keypair, Arc<Bank>) -> Vec<Shred>,
    {
        let ledger_path = get_tmp_ledger_path!();
        let (replay_vote_sender, _replay_vote_receiver) = unbounded();
        let res = {
            let ReplayBlockstoreComponents {
                blockstore,
                vote_simulator,
                ..
            } = replay_blockstore_components(Some(tr(0)), 1, None);
            let VoteSimulator {
                mut progress,
                bank_forks,
                mut heaviest_subtree_fork_choice,
                validator_keypairs,
                ..
            } = vote_simulator;

            let bank0 = bank_forks.read().unwrap().get(0).unwrap();
            assert!(bank0.is_frozen());
            assert_eq!(bank0.tick_height(), bank0.max_tick_height());
            let bank1 = Bank::new_from_parent(bank0, &Pubkey::default(), 1);
            bank_forks.write().unwrap().insert(bank1);
            let bank1 = bank_forks.read().unwrap().get(1).unwrap();
            let bank1_progress = progress
                .entry(bank1.slot())
                .or_insert_with(|| ForkProgress::new(bank1.last_blockhash(), None, None, 0, 0));
            let shreds = shred_to_insert(
                &validator_keypairs.values().next().unwrap().node_keypair,
                bank1.clone(),
            );
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
            let exit = Arc::new(AtomicBool::new(false));
            let res = ReplayStage::replay_blockstore_into_bank(
                &bank1,
                &blockstore,
                &bank1_progress.replay_stats,
                &bank1_progress.replay_progress,
                None,
                None,
                &replay_vote_sender,
                &VerifyRecyclers::default(),
                None,
                &PrioritizationFeeCache::new(0u64),
            );
            let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
            let max_complete_rewards_slot = Arc::new(AtomicU64::default());
            let rpc_subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
                exit,
                max_complete_transaction_status_slot,
                max_complete_rewards_slot,
                bank_forks.clone(),
                block_commitment_cache,
                OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
            ));
            let (ancestor_hashes_replay_update_sender, _ancestor_hashes_replay_update_receiver) =
                unbounded();
            if let Err(err) = &res {
                ReplayStage::mark_dead_slot(
                    &blockstore,
                    &bank1,
                    0,
                    err,
                    &rpc_subscriptions,
                    &mut DuplicateSlotsTracker::default(),
                    &GossipDuplicateConfirmedSlots::new(),
                    &mut EpochSlotsFrozenSlots::default(),
                    &mut progress,
                    &mut heaviest_subtree_fork_choice,
                    &mut DuplicateSlotsToRepair::default(),
                    &ancestor_hashes_replay_update_sender,
                    &mut PurgeRepairSlotCounter::default(),
                );
            }

            // Check that the erroring bank was marked as dead in the progress map
            assert!(progress
                .get(&bank1.slot())
                .map(|b| b.is_dead)
                .unwrap_or(false));

            // Check that the erroring bank was marked as dead in blockstore
            assert!(blockstore.is_dead(bank1.slot()));
            res.map(|_| ())
        };
        let _ignored = remove_dir_all(ledger_path);
        res
    }

    #[test]
    fn test_replay_commitment_cache() {
        fn leader_vote(vote_slot: Slot, bank: &Bank, pubkey: &Pubkey) {
            let mut leader_vote_account = bank.get_account(pubkey).unwrap();
            let mut vote_state = vote_state::from(&leader_vote_account).unwrap();
            vote_state::process_slot_vote_unchecked(&mut vote_state, vote_slot);
            let versioned = VoteStateVersions::new_current(vote_state);
            vote_state::to(&versioned, &mut leader_vote_account).unwrap();
            bank.store_account(pubkey, &leader_vote_account);
        }

        let leader_pubkey = solana_sdk::pubkey::new_rand();
        let leader_lamports = 3;
        let genesis_config_info =
            create_genesis_config_with_leader(50, &leader_pubkey, leader_lamports);
        let mut genesis_config = genesis_config_info.genesis_config;
        let leader_voting_pubkey = genesis_config_info.voting_keypair.pubkey();
        genesis_config.epoch_schedule.warmup = false;
        genesis_config.ticks_per_slot = 4;
        let bank0 = Bank::new_for_tests(&genesis_config);
        for _ in 0..genesis_config.ticks_per_slot {
            bank0.register_tick(&Hash::default());
        }
        bank0.freeze();
        let arc_bank0 = Arc::new(bank0);
        let bank_forks = BankForks::new_from_banks(&[arc_bank0], 0);

        let exit = Arc::new(AtomicBool::new(false));
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            exit.clone(),
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks.clone(),
            block_commitment_cache.clone(),
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
        ));
        let (lockouts_sender, _) = AggregateCommitmentService::new(
            exit,
            block_commitment_cache.clone(),
            rpc_subscriptions,
        );

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

        for i in 1..=3 {
            let prev_bank = bank_forks.read().unwrap().get(i - 1).unwrap();
            let slot = prev_bank.slot() + 1;
            let bank = Bank::new_from_parent(prev_bank, &Pubkey::default(), slot);
            let _res = bank.transfer(
                10,
                &genesis_config_info.mint_keypair,
                &solana_sdk::pubkey::new_rand(),
            );
            for _ in 0..genesis_config.ticks_per_slot {
                bank.register_tick(&Hash::default());
            }
            bank_forks.write().unwrap().insert(bank);
            let arc_bank = bank_forks.read().unwrap().get(i).unwrap();
            leader_vote(i - 1, &arc_bank, &leader_voting_pubkey);
            ReplayStage::update_commitment_cache(
                arc_bank.clone(),
                0,
                leader_lamports,
                &lockouts_sender,
            );
            arc_bank.freeze();
        }

        for _ in 0..10 {
            let done = {
                let bcc = block_commitment_cache.read().unwrap();
                bcc.get_block_commitment(0).is_some()
                    && bcc.get_block_commitment(1).is_some()
                    && bcc.get_block_commitment(2).is_some()
            };
            if done {
                break;
            } else {
                thread::sleep(Duration::from_millis(200));
            }
        }

        let mut expected0 = BlockCommitment::default();
        expected0.increase_confirmation_stake(3, leader_lamports);
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

    #[test]
    fn test_write_persist_transaction_status() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(solana_sdk::native_token::sol_to_lamports(1000.0));
        genesis_config.rent.lamports_per_byte_year = 50;
        genesis_config.rent.exemption_threshold = 2.0;
        let (ledger_path, _) = create_new_tmp_ledger!(&genesis_config);
        {
            let blockstore = Blockstore::open(&ledger_path)
                .expect("Expected to successfully open database ledger");
            let blockstore = Arc::new(blockstore);

            let keypair1 = Keypair::new();
            let keypair2 = Keypair::new();
            let keypair3 = Keypair::new();

            let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
            bank0
                .transfer(
                    bank0.get_minimum_balance_for_rent_exemption(0),
                    &mint_keypair,
                    &keypair2.pubkey(),
                )
                .unwrap();

            let bank1 = Arc::new(Bank::new_from_parent(bank0, &Pubkey::default(), 1));
            let slot = bank1.slot();

            let (entries, test_signatures) = create_test_transaction_entries(
                vec![&mint_keypair, &keypair1, &keypair2, &keypair3],
                bank1.clone(),
            );
            populate_blockstore_for_tests(
                entries,
                bank1,
                blockstore.clone(),
                Arc::new(AtomicU64::default()),
            );

            let mut test_signatures_iter = test_signatures.into_iter();
            let confirmed_block = blockstore.get_rooted_block(slot, false).unwrap();
            let actual_tx_results: Vec<_> = confirmed_block
                .transactions
                .into_iter()
                .map(|VersionedTransactionWithStatusMeta { transaction, meta }| {
                    (transaction.signatures[0], meta.status)
                })
                .collect();
            let expected_tx_results = vec![
                (test_signatures_iter.next().unwrap(), Ok(())),
                (
                    test_signatures_iter.next().unwrap(),
                    Err(TransactionError::InstructionError(
                        0,
                        InstructionError::Custom(1),
                    )),
                ),
            ];
            assert_eq!(actual_tx_results, expected_tx_results);
            assert!(test_signatures_iter.next().is_none());
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_compute_bank_stats_confirmed() {
        let vote_keypairs = ValidatorVoteKeypairs::new_rand();
        let my_node_pubkey = vote_keypairs.node_keypair.pubkey();
        let my_vote_pubkey = vote_keypairs.vote_keypair.pubkey();
        let keypairs: HashMap<_, _> = vec![(my_node_pubkey, vote_keypairs)].into_iter().collect();

        let (bank_forks, mut progress, mut heaviest_subtree_fork_choice) =
            vote_simulator::initialize_state(&keypairs, 10_000);
        let mut latest_validator_votes_for_frozen_banks =
            LatestValidatorVotesForFrozenBanks::default();
        let bank0 = bank_forks.read().unwrap().get(0).unwrap();
        let my_keypairs = keypairs.get(&my_node_pubkey).unwrap();
        let vote_tx = vote_transaction::new_vote_transaction(
            vec![0],
            bank0.hash(),
            bank0.last_blockhash(),
            &my_keypairs.node_keypair,
            &my_keypairs.vote_keypair,
            &my_keypairs.vote_keypair,
            None,
        );

        let bank1 = Bank::new_from_parent(bank0.clone(), &my_node_pubkey, 1);
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
        let mut tower = Tower::new_for_tests(0, 0.67);
        let newly_computed = ReplayStage::compute_bank_stats(
            &my_vote_pubkey,
            &ancestors,
            &mut frozen_banks,
            &mut tower,
            &mut progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &bank_forks,
            &mut heaviest_subtree_fork_choice,
            &mut latest_validator_votes_for_frozen_banks,
        );

        // bank 0 has no votes, should not send any votes on the channel
        assert_eq!(newly_computed, vec![0]);
        // The only vote is in bank 1, and bank_forks does not currently contain
        // bank 1, so no slot should be confirmed.
        {
            let fork_progress = progress.get(&0).unwrap();
            let confirmed_forks = ReplayStage::confirm_forks(
                &tower,
                &fork_progress.fork_stats.voted_stakes,
                fork_progress.fork_stats.total_stake,
                &progress,
                &bank_forks,
            );

            assert!(confirmed_forks.is_empty());
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
            &my_vote_pubkey,
            &ancestors,
            &mut frozen_banks,
            &mut tower,
            &mut progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &bank_forks,
            &mut heaviest_subtree_fork_choice,
            &mut latest_validator_votes_for_frozen_banks,
        );

        // Bank 1 had one vote
        assert_eq!(newly_computed, vec![1]);
        {
            let fork_progress = progress.get(&1).unwrap();
            let confirmed_forks = ReplayStage::confirm_forks(
                &tower,
                &fork_progress.fork_stats.voted_stakes,
                fork_progress.fork_stats.total_stake,
                &progress,
                &bank_forks,
            );
            // No new stats should have been computed
            assert_eq!(confirmed_forks, vec![(0, bank0.hash())]);
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
            &my_vote_pubkey,
            &ancestors,
            &mut frozen_banks,
            &mut tower,
            &mut progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &bank_forks,
            &mut heaviest_subtree_fork_choice,
            &mut latest_validator_votes_for_frozen_banks,
        );
        // No new stats should have been computed
        assert!(newly_computed.is_empty());
    }

    #[test]
    fn test_same_weight_select_lower_slot() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(1);
        let mut tower = Tower::default();

        // Create the tree of banks in a BankForks object
        let forks = tr(0) / (tr(1)) / (tr(2));
        vote_simulator.fill_bank_forks(forks, &HashMap::new(), true);
        let mut frozen_banks: Vec<_> = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();
        let heaviest_subtree_fork_choice = &mut vote_simulator.heaviest_subtree_fork_choice;
        let mut latest_validator_votes_for_frozen_banks =
            LatestValidatorVotesForFrozenBanks::default();
        let ancestors = vote_simulator.bank_forks.read().unwrap().ancestors();

        let my_vote_pubkey = vote_simulator.vote_pubkeys[0];
        ReplayStage::compute_bank_stats(
            &my_vote_pubkey,
            &ancestors,
            &mut frozen_banks,
            &mut tower,
            &mut vote_simulator.progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &vote_simulator.bank_forks,
            heaviest_subtree_fork_choice,
            &mut latest_validator_votes_for_frozen_banks,
        );

        let bank1 = vote_simulator.bank_forks.read().unwrap().get(1).unwrap();
        let bank2 = vote_simulator.bank_forks.read().unwrap().get(2).unwrap();
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(1, bank1.hash()))
                .unwrap(),
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(2, bank2.hash()))
                .unwrap()
        );

        let (heaviest_bank, _) = heaviest_subtree_fork_choice.select_forks(
            &frozen_banks,
            &tower,
            &vote_simulator.progress,
            &ancestors,
            &vote_simulator.bank_forks,
        );

        // Should pick the lower of the two equally weighted banks
        assert_eq!(heaviest_bank.slot(), 1);
    }

    #[test]
    fn test_child_bank_heavier() {
        // Init state
        let mut vote_simulator = VoteSimulator::new(1);
        let my_node_pubkey = vote_simulator.node_pubkeys[0];
        let mut tower = Tower::default();

        // Create the tree of banks in a BankForks object
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3))));

        // Set the voting behavior
        let mut cluster_votes = HashMap::new();
        let votes = vec![2];
        cluster_votes.insert(my_node_pubkey, votes.clone());
        vote_simulator.fill_bank_forks(forks, &cluster_votes, true);

        // Fill banks with votes
        for vote in votes {
            assert!(vote_simulator
                .simulate_vote(vote, &my_node_pubkey, &mut tower,)
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

        let my_vote_pubkey = vote_simulator.vote_pubkeys[0];
        ReplayStage::compute_bank_stats(
            &my_vote_pubkey,
            &vote_simulator.bank_forks.read().unwrap().ancestors(),
            &mut frozen_banks,
            &mut tower,
            &mut vote_simulator.progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &vote_simulator.bank_forks,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
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
        for bank in frozen_banks {
            // The only leaf should always be chosen over parents
            assert_eq!(
                vote_simulator
                    .heaviest_subtree_fork_choice
                    .best_slot(&(bank.slot(), bank.hash()))
                    .unwrap()
                    .0,
                3
            );
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
            let vote_keypairs = ValidatorVoteKeypairs::new_rand();
            (vote_keypairs.node_keypair.pubkey(), vote_keypairs)
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
        let (bank_forks, _, _) = vote_simulator::initialize_state(all_keypairs, stake);
        let root_bank = bank_forks.read().unwrap().root_bank();
        let mut propagated_stats = PropagatedStats {
            total_epoch_stake: stake * all_keypairs.len() as u64,
            ..PropagatedStats::default()
        };

        let child_reached_threshold = false;
        for i in 0..std::cmp::max(new_vote_pubkeys.len(), new_node_pubkeys.len()) {
            propagated_stats.is_propagated = false;
            let len = std::cmp::min(i, new_vote_pubkeys.len());
            let mut voted_pubkeys = new_vote_pubkeys[..len].to_vec();
            let len = std::cmp::min(i, new_node_pubkeys.len());
            let mut node_pubkeys = new_node_pubkeys[..len].to_vec();
            let did_newly_reach_threshold =
                ReplayStage::update_slot_propagated_threshold_from_votes(
                    &mut voted_pubkeys,
                    &mut node_pubkeys,
                    &root_bank,
                    &mut propagated_stats,
                    child_reached_threshold,
                );

            // Only the i'th voted pubkey should be new (everything else was
            // inserted in previous iteration of the loop), so those redundant
            // pubkeys should have been filtered out
            let remaining_vote_pubkeys = {
                if i == 0 || i >= new_vote_pubkeys.len() {
                    vec![]
                } else {
                    vec![new_vote_pubkeys[i - 1]]
                }
            };
            let remaining_node_pubkeys = {
                if i == 0 || i >= new_node_pubkeys.len() {
                    vec![]
                } else {
                    vec![new_node_pubkeys[i - 1]]
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
        let mut empty: Vec<Pubkey> = vec![];
        let genesis_config = create_genesis_config(100_000_000).genesis_config;
        let root_bank = Bank::new_for_tests(&genesis_config);
        let stake = 10_000;
        // Simulate a child slot seeing threshold (`child_reached_threshold` = true),
        // then the parent should also be marked as having reached threshold,
        // even if there are no new pubkeys to add (`newly_voted_pubkeys.is_empty()`)
        let mut propagated_stats = PropagatedStats {
            total_epoch_stake: stake * 10,
            ..PropagatedStats::default()
        };
        propagated_stats.total_epoch_stake = stake * 10;
        let child_reached_threshold = true;
        let mut newly_voted_pubkeys: Vec<Pubkey> = vec![];

        assert!(ReplayStage::update_slot_propagated_threshold_from_votes(
            &mut newly_voted_pubkeys,
            &mut empty,
            &root_bank,
            &mut propagated_stats,
            child_reached_threshold,
        ));

        // If propagation already happened (propagated_stats.is_propagated = true),
        // always returns false
        propagated_stats = PropagatedStats {
            total_epoch_stake: stake * 10,
            ..PropagatedStats::default()
        };
        propagated_stats.is_propagated = true;
        newly_voted_pubkeys = vec![];
        assert!(!ReplayStage::update_slot_propagated_threshold_from_votes(
            &mut newly_voted_pubkeys,
            &mut empty,
            &root_bank,
            &mut propagated_stats,
            child_reached_threshold,
        ));

        let child_reached_threshold = false;
        assert!(!ReplayStage::update_slot_propagated_threshold_from_votes(
            &mut newly_voted_pubkeys,
            &mut empty,
            &root_bank,
            &mut propagated_stats,
            child_reached_threshold,
        ));
    }

    #[test]
    fn test_update_propagation_status() {
        // Create genesis stakers
        let vote_keypairs = ValidatorVoteKeypairs::new_rand();
        let node_pubkey = vote_keypairs.node_keypair.pubkey();
        let vote_pubkey = vote_keypairs.vote_keypair.pubkey();
        let keypairs: HashMap<_, _> = vec![(node_pubkey, vote_keypairs)].into_iter().collect();
        let stake = 10_000;
        let (bank_forks_arc, mut progress_map, _) =
            vote_simulator::initialize_state(&keypairs, stake);
        let mut bank_forks = bank_forks_arc.write().unwrap();

        let bank0 = bank_forks.get(0).unwrap();
        bank_forks.insert(Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 9));
        let bank9 = bank_forks.get(9).unwrap();
        bank_forks.insert(Bank::new_from_parent(bank9, &Pubkey::default(), 10));
        bank_forks.set_root(9, &AbsRequestSender::default(), None);
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
        assert!(!progress_map.get_leader_propagation_slot_must_exist(10).0);

        drop(bank_forks);

        let vote_tracker = VoteTracker::default();
        vote_tracker.insert_vote(10, vote_pubkey);
        ReplayStage::update_propagation_status(
            &mut progress_map,
            10,
            &bank_forks_arc,
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
            .get_voted_slot_updates()
            .is_none());

        // The voter should be recorded
        assert!(propagated_stats
            .propagated_validators
            .contains(&vote_pubkey));

        assert_eq!(propagated_stats.propagated_validators_stake, stake);
    }

    #[test]
    fn test_chain_update_propagation_status() {
        let keypairs: HashMap<_, _> = iter::repeat_with(|| {
            let vote_keypairs = ValidatorVoteKeypairs::new_rand();
            (vote_keypairs.node_keypair.pubkey(), vote_keypairs)
        })
        .take(10)
        .collect();

        let vote_pubkeys: Vec<_> = keypairs
            .values()
            .map(|keys| keys.vote_keypair.pubkey())
            .collect();

        let stake_per_validator = 10_000;
        let (bank_forks_arc, mut progress_map, _) =
            vote_simulator::initialize_state(&keypairs, stake_per_validator);
        let mut bank_forks = bank_forks_arc.write().unwrap();
        progress_map
            .get_propagated_stats_mut(0)
            .unwrap()
            .is_leader_slot = true;
        bank_forks.set_root(0, &AbsRequestSender::default(), None);
        let total_epoch_stake = bank_forks.root_bank().total_epoch_stake();

        // Insert new ForkProgress representing a slot for all slots 1..=num_banks. Only
        // make even numbered ones leader slots
        for i in 1..=10 {
            let parent_bank = bank_forks.get(i - 1).unwrap().clone();
            let prev_leader_slot = ((i - 1) / 2) * 2;
            bank_forks.insert(Bank::new_from_parent(parent_bank, &Pubkey::default(), i));
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

        let vote_tracker = VoteTracker::default();
        for vote_pubkey in &vote_pubkeys {
            // Insert a vote for the last bank for each voter
            vote_tracker.insert_vote(10, *vote_pubkey);
        }

        drop(bank_forks);

        // The last bank should reach propagation threshold, and propagate it all
        // the way back through earlier leader banks
        ReplayStage::update_propagation_status(
            &mut progress_map,
            10,
            &bank_forks_arc,
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
            let vote_keypairs = ValidatorVoteKeypairs::new_rand();
            (vote_keypairs.node_keypair.pubkey(), vote_keypairs)
        })
        .take(num_validators)
        .collect();

        let vote_pubkeys: Vec<_> = keypairs
            .values()
            .map(|keys| keys.vote_keypair.pubkey())
            .collect();

        let stake_per_validator = 10_000;
        let (bank_forks_arc, mut progress_map, _) =
            vote_simulator::initialize_state(&keypairs, stake_per_validator);
        let mut bank_forks = bank_forks_arc.write().unwrap();
        progress_map
            .get_propagated_stats_mut(0)
            .unwrap()
            .is_leader_slot = true;
        bank_forks.set_root(0, &AbsRequestSender::default(), None);

        let total_epoch_stake = num_validators as u64 * stake_per_validator;

        // Insert new ForkProgress representing a slot for all slots 1..=num_banks. Only
        // make even numbered ones leader slots
        for i in 1..=10 {
            let parent_bank = bank_forks.get(i - 1).unwrap().clone();
            let prev_leader_slot = i - 1;
            bank_forks.insert(Bank::new_from_parent(parent_bank, &Pubkey::default(), i));
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
            fork_progress.propagated_stats.propagated_validators =
                vote_pubkeys[0..end_range].iter().copied().collect();
            fork_progress.propagated_stats.propagated_validators_stake =
                end_range as u64 * stake_per_validator;
            progress_map.insert(i, fork_progress);
        }

        let vote_tracker = VoteTracker::default();
        // Insert a new vote
        vote_tracker.insert_vote(10, vote_pubkeys[2]);

        drop(bank_forks);
        // The last bank should reach propagation threshold, and propagate it all
        // the way back through earlier leader banks
        ReplayStage::update_propagation_status(
            &mut progress_map,
            10,
            &bank_forks_arc,
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
        let parent_slot = poh_slot - NUM_CONSECUTIVE_LEADER_SLOTS;

        // If there is no previous leader slot (previous leader slot is None),
        // should succeed
        progress_map.insert(
            parent_slot,
            ForkProgress::new(Hash::default(), None, None, 0, 0),
        );
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));

        // Now if we make the parent was itself the leader, then requires propagation
        // confirmation check because the parent is at least NUM_CONSECUTIVE_LEADER_SLOTS
        // slots from the `poh_slot`
        progress_map.insert(
            parent_slot,
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
            .get_mut(&parent_slot)
            .unwrap()
            .propagated_stats
            .is_propagated = true;
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));
        // Now, set up the progress map to show that the `previous_leader_slot` of 5 is
        // `parent_slot - 1` (not equal to the actual parent!), so `parent_slot - 1` needs
        // to see propagation confirmation before we can start a leader for block 5
        let previous_leader_slot = parent_slot - 1;
        progress_map.insert(
            parent_slot,
            ForkProgress::new(Hash::default(), Some(previous_leader_slot), None, 0, 0),
        );
        progress_map.insert(
            previous_leader_slot,
            ForkProgress::new(
                Hash::default(),
                None,
                Some(ValidatorStakeInfo::default()),
                0,
                0,
            ),
        );

        // `previous_leader_slot` has not seen propagation threshold, so should fail
        assert!(!ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));

        // If we set the is_propagated = true for the `previous_leader_slot`, should
        // allow the block to be generated
        progress_map
            .get_mut(&previous_leader_slot)
            .unwrap()
            .propagated_stats
            .is_propagated = true;
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));

        // If the root is now set to `parent_slot`, this filters out `previous_leader_slot` from the progress map,
        // which implies confirmation
        let bank0 = Bank::new_for_tests(&genesis_config::create_genesis_config(10000).0);
        let parent_slot_bank =
            Bank::new_from_parent(Arc::new(bank0), &Pubkey::default(), parent_slot);
        let bank_forks = BankForks::new_rw_arc(parent_slot_bank);
        let mut bank_forks = bank_forks.write().unwrap();
        let bank5 =
            Bank::new_from_parent(bank_forks.get(parent_slot).unwrap(), &Pubkey::default(), 5);
        bank_forks.insert(bank5);

        // Should purge only `previous_leader_slot` from the progress map
        progress_map.handle_new_root(&bank_forks);

        // Should succeed
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));
    }

    #[test]
    fn test_check_propagation_skip_propagation_check() {
        let mut progress_map = ProgressMap::default();
        let poh_slot = 4;
        let mut parent_slot = poh_slot - 1;

        // Set up the progress map to show that the last leader slot of 4 is 3,
        // which means 3 and 4 are consecutive leader slots
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

        // If the previous leader slot has not seen propagation threshold, but
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

        // Now insert another parent slot 2 for which this validator is also the leader
        parent_slot = poh_slot - NUM_CONSECUTIVE_LEADER_SLOTS + 1;
        progress_map.insert(
            parent_slot,
            ForkProgress::new(
                Hash::default(),
                None,
                Some(ValidatorStakeInfo::default()),
                0,
                0,
            ),
        );

        // Even though `parent_slot` and `poh_slot` are separated by another block,
        // because they're within `NUM_CONSECUTIVE` blocks of each other, the propagation
        // check is still skipped
        assert!(ReplayStage::check_propagation_for_start_leader(
            poh_slot,
            parent_slot,
            &progress_map,
        ));

        // Once the distance becomes >= NUM_CONSECUTIVE_LEADER_SLOTS, then we need to
        // enforce the propagation check
        parent_slot = poh_slot - NUM_CONSECUTIVE_LEADER_SLOTS;
        progress_map.insert(
            parent_slot,
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
    }

    #[test]
    fn test_purge_unconfirmed_duplicate_slot() {
        let (vote_simulator, blockstore) = setup_default_forks(2, None::<GenerateVotes>);
        let VoteSimulator {
            bank_forks,
            node_pubkeys,
            mut progress,
            validator_keypairs,
            ..
        } = vote_simulator;

        // Create bank 7
        let root_bank = bank_forks.read().unwrap().root_bank();
        let bank7 = Bank::new_from_parent(
            bank_forks.read().unwrap().get(6).unwrap(),
            &Pubkey::default(),
            7,
        );
        bank_forks.write().unwrap().insert(bank7);
        blockstore.add_tree(tr(6) / tr(7), false, false, 3, Hash::default());
        let bank7 = bank_forks.read().unwrap().get(7).unwrap();
        let mut descendants = bank_forks.read().unwrap().descendants();
        let mut ancestors = bank_forks.read().unwrap().ancestors();

        // Process a transfer on bank 7
        let sender = node_pubkeys[0];
        let receiver = node_pubkeys[1];
        let old_balance = bank7.get_balance(&sender);
        let transfer_amount = old_balance / 2;
        let transfer_sig = bank7
            .transfer(
                transfer_amount,
                &validator_keypairs.get(&sender).unwrap().node_keypair,
                &receiver,
            )
            .unwrap();

        // Process a vote for slot 0 in bank 5
        let validator0_keypairs = &validator_keypairs.get(&sender).unwrap();
        let bank0 = bank_forks.read().unwrap().get(0).unwrap();
        let vote_tx = vote_transaction::new_vote_transaction(
            vec![0],
            bank0.hash(),
            bank0.last_blockhash(),
            &validator0_keypairs.node_keypair,
            &validator0_keypairs.vote_keypair,
            &validator0_keypairs.vote_keypair,
            None,
        );
        bank7.process_transaction(&vote_tx).unwrap();
        assert!(bank7.get_signature_status(&vote_tx.signatures[0]).is_some());

        // Both signatures should exist in status cache
        assert!(bank7.get_signature_status(&vote_tx.signatures[0]).is_some());
        assert!(bank7.get_signature_status(&transfer_sig).is_some());

        // Give all slots a bank hash but mark slot 7 dead
        for i in 0..=6 {
            blockstore.insert_bank_hash(i, Hash::new_unique(), false);
        }
        blockstore
            .set_dead_slot(7)
            .expect("Failed to mark slot as dead in blockstore");

        // Purging slot 5 should purge only slots 5 and its descendant 6. Since 7 is already dead,
        // it gets reset but not removed
        ReplayStage::purge_unconfirmed_duplicate_slot(
            5,
            &mut ancestors,
            &mut descendants,
            &mut progress,
            &root_bank,
            &bank_forks,
            &blockstore,
        );
        for i in 5..=7 {
            assert!(bank_forks.read().unwrap().get(i).is_none());
            assert!(progress.get(&i).is_none());
        }
        for i in 0..=4 {
            assert!(bank_forks.read().unwrap().get(i).is_some());
            assert!(progress.get(&i).is_some());
        }

        // Blockstore should have been cleared
        for slot in &[5, 6] {
            assert!(!blockstore.is_full(*slot));
            assert!(!blockstore.is_dead(*slot));
            assert!(blockstore.get_slot_entries(*slot, 0).unwrap().is_empty());
        }

        // Slot 7 was marked dead before, should no longer be marked
        assert!(!blockstore.is_dead(7));
        assert!(!blockstore.get_slot_entries(7, 0).unwrap().is_empty());

        // Should not be able to find signature in slot 5 for previously
        // processed transactions
        assert!(bank7.get_signature_status(&vote_tx.signatures[0]).is_none());
        assert!(bank7.get_signature_status(&transfer_sig).is_none());

        // Getting balance should return the old balance (accounts were cleared)
        assert_eq!(bank7.get_balance(&sender), old_balance);

        // Purging slot 4 should purge only slot 4
        let mut descendants = bank_forks.read().unwrap().descendants();
        let mut ancestors = bank_forks.read().unwrap().ancestors();
        ReplayStage::purge_unconfirmed_duplicate_slot(
            4,
            &mut ancestors,
            &mut descendants,
            &mut progress,
            &root_bank,
            &bank_forks,
            &blockstore,
        );
        for i in 4..=6 {
            assert!(bank_forks.read().unwrap().get(i).is_none());
            assert!(progress.get(&i).is_none());
            assert!(blockstore.get_slot_entries(i, 0).unwrap().is_empty());
        }
        for i in 0..=3 {
            assert!(bank_forks.read().unwrap().get(i).is_some());
            assert!(progress.get(&i).is_some());
            assert!(!blockstore.get_slot_entries(i, 0).unwrap().is_empty());
        }

        // Purging slot 1 should purge both forks 2 and 3 but leave 7 untouched as it is dead
        let mut descendants = bank_forks.read().unwrap().descendants();
        let mut ancestors = bank_forks.read().unwrap().ancestors();
        ReplayStage::purge_unconfirmed_duplicate_slot(
            1,
            &mut ancestors,
            &mut descendants,
            &mut progress,
            &root_bank,
            &bank_forks,
            &blockstore,
        );
        for i in 1..=6 {
            assert!(bank_forks.read().unwrap().get(i).is_none());
            assert!(progress.get(&i).is_none());
            assert!(blockstore.get_slot_entries(i, 0).unwrap().is_empty());
        }
        assert!(bank_forks.read().unwrap().get(0).is_some());
        assert!(progress.get(&0).is_some());

        // Slot 7 untouched
        assert!(!blockstore.is_dead(7));
        assert!(!blockstore.get_slot_entries(7, 0).unwrap().is_empty());
    }

    #[test]
    fn test_purge_unconfirmed_duplicate_slots_and_reattach() {
        let ReplayBlockstoreComponents {
            blockstore,
            validator_node_to_vote_keys,
            vote_simulator,
            leader_schedule_cache,
            rpc_subscriptions,
            ..
        } = replay_blockstore_components(
            Some(tr(0) / (tr(1) / (tr(2) / (tr(4))) / (tr(3) / (tr(5) / (tr(6)))))),
            1,
            None::<GenerateVotes>,
        );

        let VoteSimulator {
            bank_forks,
            mut progress,
            ..
        } = vote_simulator;

        let mut replay_timing = ReplayTiming::default();

        // Create bank 7 and insert to blockstore and bank forks
        let root_bank = bank_forks.read().unwrap().root_bank();
        let bank7 = Bank::new_from_parent(
            bank_forks.read().unwrap().get(6).unwrap(),
            &Pubkey::default(),
            7,
        );
        bank_forks.write().unwrap().insert(bank7);
        blockstore.add_tree(tr(6) / tr(7), false, false, 3, Hash::default());
        let mut descendants = bank_forks.read().unwrap().descendants();
        let mut ancestors = bank_forks.read().unwrap().ancestors();

        // Mark earlier slots as frozen, but we have the wrong version of slots 3 and 5, so slot 6 is dead and
        // slot 7 is unreplayed
        for i in 0..=5 {
            blockstore.insert_bank_hash(i, Hash::new_unique(), false);
        }
        blockstore
            .set_dead_slot(6)
            .expect("Failed to mark slot 6 as dead in blockstore");

        // Purge slot 3 as it is duplicate, this should also purge slot 5 but not touch 6 and 7
        ReplayStage::purge_unconfirmed_duplicate_slot(
            3,
            &mut ancestors,
            &mut descendants,
            &mut progress,
            &root_bank,
            &bank_forks,
            &blockstore,
        );
        for slot in &[3, 5, 6, 7] {
            assert!(bank_forks.read().unwrap().get(*slot).is_none());
            assert!(progress.get(slot).is_none());
        }
        for slot in &[3, 5] {
            assert!(!blockstore.is_full(*slot));
            assert!(!blockstore.is_dead(*slot));
            assert!(blockstore.get_slot_entries(*slot, 0).unwrap().is_empty());
        }
        for slot in 6..=7 {
            assert!(!blockstore.is_dead(slot));
            assert!(!blockstore.get_slot_entries(slot, 0).unwrap().is_empty())
        }

        // Simulate repair fixing slot 3 and 5
        let (shreds, _) = make_slot_entries(
            3,    // slot
            1,    // parent_slot
            8,    // num_entries
            true, // merkle_variant
        );
        blockstore.insert_shreds(shreds, None, false).unwrap();
        let (shreds, _) = make_slot_entries(
            5,    // slot
            3,    // parent_slot
            8,    // num_entries
            true, // merkle_variant
        );
        blockstore.insert_shreds(shreds, None, false).unwrap();

        // 3 should now be an active bank
        ReplayStage::generate_new_bank_forks(
            &blockstore,
            &bank_forks,
            &leader_schedule_cache,
            &rpc_subscriptions,
            &mut progress,
            &mut replay_timing,
        );
        assert_eq!(bank_forks.read().unwrap().active_bank_slots(), vec![3]);

        // Freeze 3
        {
            let bank3 = bank_forks.read().unwrap().get(3).unwrap();
            progress.insert(
                3,
                ForkProgress::new_from_bank(
                    &bank3,
                    bank3.collector_id(),
                    validator_node_to_vote_keys
                        .get(bank3.collector_id())
                        .unwrap(),
                    Some(1),
                    0,
                    0,
                ),
            );
            bank3.freeze();
        }
        // 5 Should now be an active bank
        ReplayStage::generate_new_bank_forks(
            &blockstore,
            &bank_forks,
            &leader_schedule_cache,
            &rpc_subscriptions,
            &mut progress,
            &mut replay_timing,
        );
        assert_eq!(bank_forks.read().unwrap().active_bank_slots(), vec![5]);

        // Freeze 5
        {
            let bank5 = bank_forks.read().unwrap().get(5).unwrap();
            progress.insert(
                5,
                ForkProgress::new_from_bank(
                    &bank5,
                    bank5.collector_id(),
                    validator_node_to_vote_keys
                        .get(bank5.collector_id())
                        .unwrap(),
                    Some(3),
                    0,
                    0,
                ),
            );
            bank5.freeze();
        }
        // 6 should now be an active bank even though we haven't repaired it because it
        // wasn't dumped
        ReplayStage::generate_new_bank_forks(
            &blockstore,
            &bank_forks,
            &leader_schedule_cache,
            &rpc_subscriptions,
            &mut progress,
            &mut replay_timing,
        );
        assert_eq!(bank_forks.read().unwrap().active_bank_slots(), vec![6]);

        // Freeze 6 now that we have the correct version of 5.
        {
            let bank6 = bank_forks.read().unwrap().get(6).unwrap();
            progress.insert(
                6,
                ForkProgress::new_from_bank(
                    &bank6,
                    bank6.collector_id(),
                    validator_node_to_vote_keys
                        .get(bank6.collector_id())
                        .unwrap(),
                    Some(5),
                    0,
                    0,
                ),
            );
            bank6.freeze();
        }
        // 7 should be found as an active bank
        ReplayStage::generate_new_bank_forks(
            &blockstore,
            &bank_forks,
            &leader_schedule_cache,
            &rpc_subscriptions,
            &mut progress,
            &mut replay_timing,
        );
        assert_eq!(bank_forks.read().unwrap().active_bank_slots(), vec![7]);
    }

    #[test]
    fn test_purge_ancestors_descendants() {
        let (VoteSimulator { bank_forks, .. }, _) = setup_default_forks(1, None::<GenerateVotes>);

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
        // and regenerating the `ancestor` `descendant` maps
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
        bank_forks
            .write()
            .unwrap()
            .set_root(3, &AbsRequestSender::default(), None);
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

    #[test]
    fn test_leader_snapshot_restart_propagation() {
        let ReplayBlockstoreComponents {
            validator_node_to_vote_keys,
            leader_schedule_cache,
            vote_simulator,
            ..
        } = replay_blockstore_components(None, 1, None::<GenerateVotes>);

        let VoteSimulator {
            mut progress,
            bank_forks,
            ..
        } = vote_simulator;

        let root_bank = bank_forks.read().unwrap().root_bank();
        let my_pubkey = leader_schedule_cache
            .slot_leader_at(root_bank.slot(), Some(&root_bank))
            .unwrap();

        // Check that we are the leader of the root bank
        assert!(
            progress
                .get_propagated_stats(root_bank.slot())
                .unwrap()
                .is_leader_slot
        );
        let ancestors = bank_forks.read().unwrap().ancestors();

        // Freeze bank so it shows up in frozen banks
        root_bank.freeze();
        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();

        // Compute bank stats, make sure vote is propagated back to starting root bank
        let vote_tracker = VoteTracker::default();

        // Add votes
        for vote_key in validator_node_to_vote_keys.values() {
            vote_tracker.insert_vote(root_bank.slot(), *vote_key);
        }

        assert!(
            !progress
                .get_leader_propagation_slot_must_exist(root_bank.slot())
                .0
        );

        // Update propagation status
        let mut tower = Tower::new_for_tests(0, 0.67);
        ReplayStage::compute_bank_stats(
            &validator_node_to_vote_keys[&my_pubkey],
            &ancestors,
            &mut frozen_banks,
            &mut tower,
            &mut progress,
            &vote_tracker,
            &ClusterSlots::default(),
            &bank_forks,
            &mut HeaviestSubtreeForkChoice::new_from_bank_forks(bank_forks.clone()),
            &mut LatestValidatorVotesForFrozenBanks::default(),
        );

        // Check status is true
        assert!(
            progress
                .get_leader_propagation_slot_must_exist(root_bank.slot())
                .0
        );
    }

    #[test]
    fn test_unconfirmed_duplicate_slots_and_lockouts_for_non_heaviest_fork() {
        /*
            Build fork structure:

                 slot 0
                   |
                 slot 1
                 /    \
            slot 2    |
               |      |
            slot 3    |
               |      |
            slot 4    |
                    slot 5
        */
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3) / (tr(4)))) / tr(5));

        let mut vote_simulator = VoteSimulator::new(1);
        vote_simulator.fill_bank_forks(forks, &HashMap::<Pubkey, Vec<u64>>::new(), true);
        let (bank_forks, mut progress) = (vote_simulator.bank_forks, vote_simulator.progress);
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(
            Blockstore::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let mut tower = Tower::new_for_tests(8, 2.0 / 3.0);

        // All forks have same weight so heaviest bank to vote/reset on should be the tip of
        // the fork with the lower slot
        let (vote_fork, reset_fork, _) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            None,
        );
        assert_eq!(vote_fork.unwrap(), 4);
        assert_eq!(reset_fork.unwrap(), 4);

        // Record the vote for 5 which is not on the heaviest fork.
        tower.record_bank_vote(&bank_forks.read().unwrap().get(5).unwrap());

        // 4 should be the heaviest slot, but should not be votable
        // because of lockout. 5 is the heaviest slot on the same fork as the last vote.
        let (vote_fork, reset_fork, _) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            None,
        );
        assert!(vote_fork.is_none());
        assert_eq!(reset_fork, Some(5));

        // Mark 5 as duplicate
        blockstore.store_duplicate_slot(5, vec![], vec![]).unwrap();
        let mut duplicate_slots_tracker = DuplicateSlotsTracker::default();
        let mut purge_repair_slot_counter = PurgeRepairSlotCounter::default();
        let mut gossip_duplicate_confirmed_slots = GossipDuplicateConfirmedSlots::default();
        let mut epoch_slots_frozen_slots = EpochSlotsFrozenSlots::default();
        let bank5_hash = bank_forks.read().unwrap().bank_hash(5).unwrap();
        assert_ne!(bank5_hash, Hash::default());
        let duplicate_state = DuplicateState::new_from_state(
            5,
            &gossip_duplicate_confirmed_slots,
            &vote_simulator.heaviest_subtree_fork_choice,
            || progress.is_dead(5).unwrap_or(false),
            || Some(bank5_hash),
        );
        let (ancestor_hashes_replay_update_sender, _ancestor_hashes_replay_update_receiver) =
            unbounded();
        check_slot_agrees_with_cluster(
            5,
            bank_forks.read().unwrap().root(),
            &blockstore,
            &mut duplicate_slots_tracker,
            &mut epoch_slots_frozen_slots,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut DuplicateSlotsToRepair::default(),
            &ancestor_hashes_replay_update_sender,
            &mut purge_repair_slot_counter,
            SlotStateUpdate::Duplicate(duplicate_state),
        );

        // 4 should be the heaviest slot, but should not be votable
        // because of lockout. 5 is no longer valid due to it being a duplicate.
        let (vote_fork, reset_fork, _) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            None,
        );
        assert!(vote_fork.is_none());
        assert!(reset_fork.is_none());

        // If slot 5 is marked as confirmed, it becomes the heaviest bank on same slot again
        let mut duplicate_slots_to_repair = DuplicateSlotsToRepair::default();
        gossip_duplicate_confirmed_slots.insert(5, bank5_hash);
        let duplicate_confirmed_state = DuplicateConfirmedState::new_from_state(
            bank5_hash,
            || progress.is_dead(5).unwrap_or(false),
            || Some(bank5_hash),
        );
        check_slot_agrees_with_cluster(
            5,
            bank_forks.read().unwrap().root(),
            &blockstore,
            &mut duplicate_slots_tracker,
            &mut epoch_slots_frozen_slots,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut duplicate_slots_to_repair,
            &ancestor_hashes_replay_update_sender,
            &mut purge_repair_slot_counter,
            SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state),
        );
        // The confirmed hash is detected in `progress`, which means
        // it's confirmation on the replayed block. This means we have
        // the right version of the block, so `duplicate_slots_to_repair`
        // should be empty
        assert!(duplicate_slots_to_repair.is_empty());
        let (vote_fork, reset_fork, _) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            None,
        );
        // Should now pick 5 as the heaviest fork from last vote again.
        assert!(vote_fork.is_none());
        assert_eq!(reset_fork.unwrap(), 5);
    }

    #[test]
    fn test_unconfirmed_duplicate_slots_and_lockouts() {
        /*
            Build fork structure:

                 slot 0
                   |
                 slot 1
                 /    \
            slot 2    |
               |      |
            slot 3    |
               |      |
            slot 4    |
                    slot 5
                      |
                    slot 6
        */
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3) / (tr(4)))) / (tr(5) / (tr(6))));

        // Make enough validators for vote switch threshold later
        let mut vote_simulator = VoteSimulator::new(2);
        let validator_votes: HashMap<Pubkey, Vec<u64>> = vec![
            (vote_simulator.node_pubkeys[0], vec![5]),
            (vote_simulator.node_pubkeys[1], vec![2]),
        ]
        .into_iter()
        .collect();
        vote_simulator.fill_bank_forks(forks, &validator_votes, true);

        let (bank_forks, mut progress) = (vote_simulator.bank_forks, vote_simulator.progress);
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(
            Blockstore::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let mut tower = Tower::new_for_tests(8, 0.67);

        // All forks have same weight so heaviest bank to vote/reset on should be the tip of
        // the fork with the lower slot
        let (vote_fork, reset_fork, _) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            None,
        );
        assert_eq!(vote_fork.unwrap(), 4);
        assert_eq!(reset_fork.unwrap(), 4);

        // Record the vote for 4
        tower.record_bank_vote(&bank_forks.read().unwrap().get(4).unwrap());

        // Mark 4 as duplicate, 3 should be the heaviest slot, but should not be votable
        // because of lockout
        blockstore.store_duplicate_slot(4, vec![], vec![]).unwrap();
        let mut duplicate_slots_tracker = DuplicateSlotsTracker::default();
        let mut gossip_duplicate_confirmed_slots = GossipDuplicateConfirmedSlots::default();
        let mut epoch_slots_frozen_slots = EpochSlotsFrozenSlots::default();
        let bank4_hash = bank_forks.read().unwrap().bank_hash(4).unwrap();
        assert_ne!(bank4_hash, Hash::default());
        let duplicate_state = DuplicateState::new_from_state(
            4,
            &gossip_duplicate_confirmed_slots,
            &vote_simulator.heaviest_subtree_fork_choice,
            || progress.is_dead(4).unwrap_or(false),
            || Some(bank4_hash),
        );
        let (ancestor_hashes_replay_update_sender, _ancestor_hashes_replay_update_receiver) =
            unbounded();
        check_slot_agrees_with_cluster(
            4,
            bank_forks.read().unwrap().root(),
            &blockstore,
            &mut duplicate_slots_tracker,
            &mut epoch_slots_frozen_slots,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut DuplicateSlotsToRepair::default(),
            &ancestor_hashes_replay_update_sender,
            &mut PurgeRepairSlotCounter::default(),
            SlotStateUpdate::Duplicate(duplicate_state),
        );

        let (vote_fork, reset_fork, _) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            None,
        );
        assert!(vote_fork.is_none());
        assert_eq!(reset_fork, Some(3));

        // Now mark 2, an ancestor of 4, as duplicate
        blockstore.store_duplicate_slot(2, vec![], vec![]).unwrap();
        let bank2_hash = bank_forks.read().unwrap().bank_hash(2).unwrap();
        assert_ne!(bank2_hash, Hash::default());
        let duplicate_state = DuplicateState::new_from_state(
            2,
            &gossip_duplicate_confirmed_slots,
            &vote_simulator.heaviest_subtree_fork_choice,
            || progress.is_dead(2).unwrap_or(false),
            || Some(bank2_hash),
        );
        check_slot_agrees_with_cluster(
            2,
            bank_forks.read().unwrap().root(),
            &blockstore,
            &mut duplicate_slots_tracker,
            &mut epoch_slots_frozen_slots,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut DuplicateSlotsToRepair::default(),
            &ancestor_hashes_replay_update_sender,
            &mut PurgeRepairSlotCounter::default(),
            SlotStateUpdate::Duplicate(duplicate_state),
        );

        let (vote_fork, reset_fork, _) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            None,
        );

        // Should now pick the next heaviest fork that is not a descendant of 2, which is 6.
        // However the lockout from vote 4 should still apply, so 6 should not be votable
        assert!(vote_fork.is_none());
        assert_eq!(reset_fork.unwrap(), 6);

        // If slot 4 is marked as confirmed, then this confirms slot 2 and 4, and
        // then slot 4 is now the heaviest bank again
        let mut duplicate_slots_to_repair = DuplicateSlotsToRepair::default();
        gossip_duplicate_confirmed_slots.insert(4, bank4_hash);
        let duplicate_confirmed_state = DuplicateConfirmedState::new_from_state(
            bank4_hash,
            || progress.is_dead(4).unwrap_or(false),
            || Some(bank4_hash),
        );
        check_slot_agrees_with_cluster(
            4,
            bank_forks.read().unwrap().root(),
            &blockstore,
            &mut duplicate_slots_tracker,
            &mut epoch_slots_frozen_slots,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut duplicate_slots_to_repair,
            &ancestor_hashes_replay_update_sender,
            &mut PurgeRepairSlotCounter::default(),
            SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state),
        );
        // The confirmed hash is detected in `progress`, which means
        // it's confirmation on the replayed block. This means we have
        // the right version of the block, so `duplicate_slots_to_repair`
        // should be empty
        assert!(duplicate_slots_to_repair.is_empty());
        let (vote_fork, reset_fork, _) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            None,
        );
        // Should now pick the heaviest fork 4 again, but lockouts apply so fork 4
        // is not votable, which avoids voting for 4 again.
        assert!(vote_fork.is_none());
        assert_eq!(reset_fork.unwrap(), 4);
    }

    #[test]
    fn test_dump_then_repair_correct_slots() {
        // Create the tree of banks in a BankForks object
        let forks = tr(0) / (tr(1)) / (tr(2));

        let ReplayBlockstoreComponents {
            ref mut vote_simulator,
            ref blockstore,
            ref leader_schedule_cache,
            ..
        } = replay_blockstore_components(Some(forks), 1, None);

        let VoteSimulator {
            ref mut progress,
            ref bank_forks,
            ..
        } = vote_simulator;

        let (mut ancestors, mut descendants) = {
            let r_bank_forks = bank_forks.read().unwrap();
            (r_bank_forks.ancestors(), r_bank_forks.descendants())
        };

        // Insert different versions of both 1 and 2. Both slots 1 and 2 should
        // then be purged
        let mut duplicate_slots_to_repair = DuplicateSlotsToRepair::default();
        duplicate_slots_to_repair.insert(1, Hash::new_unique());
        duplicate_slots_to_repair.insert(2, Hash::new_unique());
        let mut purge_repair_slot_counter = PurgeRepairSlotCounter::default();
        let (dumped_slots_sender, dumped_slots_receiver) = unbounded();
        let should_be_dumped = duplicate_slots_to_repair
            .iter()
            .map(|(&s, &h)| (s, h))
            .collect_vec();

        ReplayStage::dump_then_repair_correct_slots(
            &mut duplicate_slots_to_repair,
            &mut ancestors,
            &mut descendants,
            progress,
            bank_forks,
            blockstore,
            None,
            &mut purge_repair_slot_counter,
            &dumped_slots_sender,
            &Pubkey::new_unique(),
            leader_schedule_cache,
        );
        assert_eq!(should_be_dumped, dumped_slots_receiver.recv().ok().unwrap());

        let r_bank_forks = bank_forks.read().unwrap();
        for slot in 0..=2 {
            let bank = r_bank_forks.get(slot);
            let ancestor_result = ancestors.get(&slot);
            let descendants_result = descendants.get(&slot);
            if slot == 0 {
                assert!(bank.is_some());
                assert!(ancestor_result.is_some());
                assert!(descendants_result.is_some());
            } else {
                assert!(bank.is_none());
                assert!(ancestor_result.is_none());
                assert!(descendants_result.is_none());
            }
        }
        assert_eq!(2, purge_repair_slot_counter.len());
        assert_eq!(1, *purge_repair_slot_counter.get(&1).unwrap());
        assert_eq!(1, *purge_repair_slot_counter.get(&2).unwrap());
    }

    fn setup_vote_then_rollback(
        first_vote: Slot,
        num_validators: usize,
        generate_votes: Option<GenerateVotes>,
    ) -> ReplayBlockstoreComponents {
        /*
            Build fork structure:

                 slot 0
                   |
                 slot 1
                 /    \
            slot 2    |
               |      |
            slot 3    |
               |      |
            slot 4    |
               |      |
            slot 5    |
                    slot 6
                      |
                    slot 7
        */
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3) / (tr(4) / (tr(5))))) / (tr(6) / (tr(7))));

        let mut replay_components =
            replay_blockstore_components(Some(forks), num_validators, generate_votes);

        let ReplayBlockstoreComponents {
            ref mut tower,
            ref blockstore,
            ref mut vote_simulator,
            ref leader_schedule_cache,
            ..
        } = replay_components;

        let VoteSimulator {
            ref mut progress,
            ref bank_forks,
            ref mut heaviest_subtree_fork_choice,
            ..
        } = vote_simulator;

        tower.record_bank_vote(&bank_forks.read().unwrap().get(first_vote).unwrap());

        // Simulate another version of slot 2 was duplicate confirmed
        let our_bank2_hash = bank_forks.read().unwrap().bank_hash(2).unwrap();
        let duplicate_confirmed_bank2_hash = Hash::new_unique();
        let mut gossip_duplicate_confirmed_slots = GossipDuplicateConfirmedSlots::default();
        gossip_duplicate_confirmed_slots.insert(2, duplicate_confirmed_bank2_hash);
        let mut duplicate_slots_tracker = DuplicateSlotsTracker::default();
        let mut duplicate_slots_to_repair = DuplicateSlotsToRepair::default();
        let mut epoch_slots_frozen_slots = EpochSlotsFrozenSlots::default();

        // Mark fork choice branch as invalid so select forks below doesn't panic
        // on a nonexistent `heaviest_bank_on_same_fork` after we dump the duplciate fork.
        let duplicate_confirmed_state = DuplicateConfirmedState::new_from_state(
            duplicate_confirmed_bank2_hash,
            || progress.is_dead(2).unwrap_or(false),
            || Some(our_bank2_hash),
        );
        let (ancestor_hashes_replay_update_sender, _ancestor_hashes_replay_update_receiver) =
            unbounded();
        check_slot_agrees_with_cluster(
            2,
            bank_forks.read().unwrap().root(),
            blockstore,
            &mut duplicate_slots_tracker,
            &mut epoch_slots_frozen_slots,
            heaviest_subtree_fork_choice,
            &mut duplicate_slots_to_repair,
            &ancestor_hashes_replay_update_sender,
            &mut PurgeRepairSlotCounter::default(),
            SlotStateUpdate::DuplicateConfirmed(duplicate_confirmed_state),
        );
        assert_eq!(
            *duplicate_slots_to_repair.get(&2).unwrap(),
            duplicate_confirmed_bank2_hash
        );
        let mut ancestors = bank_forks.read().unwrap().ancestors();
        let mut descendants = bank_forks.read().unwrap().descendants();
        let old_descendants_of_2 = descendants.get(&2).unwrap().clone();
        let (dumped_slots_sender, _dumped_slots_receiver) = unbounded();

        ReplayStage::dump_then_repair_correct_slots(
            &mut duplicate_slots_to_repair,
            &mut ancestors,
            &mut descendants,
            progress,
            bank_forks,
            blockstore,
            None,
            &mut PurgeRepairSlotCounter::default(),
            &dumped_slots_sender,
            &Pubkey::new_unique(),
            leader_schedule_cache,
        );

        // Check everything was purged properly
        for purged_slot in std::iter::once(&2).chain(old_descendants_of_2.iter()) {
            assert!(!ancestors.contains_key(purged_slot));
            assert!(!descendants.contains_key(purged_slot));
        }

        replay_components
    }

    fn run_test_duplicate_rollback_then_vote(first_vote: Slot) -> SelectVoteAndResetForkResult {
        let replay_components = setup_vote_then_rollback(
            first_vote,
            2,
            Some(Box::new(|node_keys| {
                // Simulate everyone else voting on 6, so we have enough to
                // make a switch to the other fork
                node_keys.into_iter().map(|k| (k, vec![6])).collect()
            })),
        );

        let ReplayBlockstoreComponents {
            mut tower,
            vote_simulator,
            ..
        } = replay_components;

        let VoteSimulator {
            mut progress,
            bank_forks,
            mut heaviest_subtree_fork_choice,
            mut latest_validator_votes_for_frozen_banks,
            ..
        } = vote_simulator;

        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();

        let ancestors = bank_forks.read().unwrap().ancestors();
        let descendants = bank_forks.read().unwrap().descendants();

        ReplayStage::compute_bank_stats(
            &Pubkey::new_unique(),
            &ancestors,
            &mut frozen_banks,
            &mut tower,
            &mut progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &bank_forks,
            &mut heaviest_subtree_fork_choice,
            &mut latest_validator_votes_for_frozen_banks,
        );

        // Try to switch to vote to the heaviest slot 6, then return the vote results
        let (heaviest_bank, heaviest_bank_on_same_fork) = heaviest_subtree_fork_choice
            .select_forks(&frozen_banks, &tower, &progress, &ancestors, &bank_forks);
        assert_eq!(heaviest_bank.slot(), 7);
        assert!(heaviest_bank_on_same_fork.is_none());
        ReplayStage::select_vote_and_reset_forks(
            &heaviest_bank,
            heaviest_bank_on_same_fork.as_ref(),
            &ancestors,
            &descendants,
            &progress,
            &mut tower,
            &latest_validator_votes_for_frozen_banks,
            &heaviest_subtree_fork_choice,
        )
    }

    #[test]
    fn test_duplicate_rollback_then_vote_locked_out() {
        let SelectVoteAndResetForkResult {
            vote_bank,
            reset_bank,
            heaviest_fork_failures,
        } = run_test_duplicate_rollback_then_vote(5);

        // If we vote on 5 first then try to vote on 7, we should be locked out,
        // despite the rollback
        assert!(vote_bank.is_none());
        assert_eq!(reset_bank.unwrap().slot(), 7);
        assert_eq!(
            heaviest_fork_failures,
            vec![HeaviestForkFailures::LockedOut(7)]
        );
    }

    #[test]
    fn test_duplicate_rollback_then_vote_success() {
        let SelectVoteAndResetForkResult {
            vote_bank,
            reset_bank,
            heaviest_fork_failures,
        } = run_test_duplicate_rollback_then_vote(4);

        // If we vote on 4 first then try to vote on 7, we should succeed
        assert_matches!(
            vote_bank
                .map(|(bank, switch_decision)| (bank.slot(), switch_decision))
                .unwrap(),
            (7, SwitchForkDecision::SwitchProof(_))
        );
        assert_eq!(reset_bank.unwrap().slot(), 7);
        assert!(heaviest_fork_failures.is_empty());
    }

    fn run_test_duplicate_rollback_then_vote_on_other_duplicate(
        first_vote: Slot,
    ) -> SelectVoteAndResetForkResult {
        let replay_components = setup_vote_then_rollback(first_vote, 10, None::<GenerateVotes>);

        let ReplayBlockstoreComponents {
            mut tower,
            mut vote_simulator,
            ..
        } = replay_components;

        // Simulate repairing an alternate version of slot 2, 3 and 4 that we just dumped. Because
        // we're including votes this time for slot 1, it should generate a different
        // version of 2.
        let cluster_votes: HashMap<Pubkey, Vec<Slot>> = vote_simulator
            .node_pubkeys
            .iter()
            .map(|k| (*k, vec![1, 2]))
            .collect();

        // Create new versions of slots 2, 3, 4, 5, with parent slot 1
        vote_simulator.create_and_vote_new_branch(
            1,
            5,
            &cluster_votes,
            &HashSet::new(),
            &Pubkey::new_unique(),
            &mut tower,
        );

        let VoteSimulator {
            mut progress,
            bank_forks,
            mut heaviest_subtree_fork_choice,
            mut latest_validator_votes_for_frozen_banks,
            ..
        } = vote_simulator;

        // Check that the new branch with slot 2 is different than the original version.
        let bank_1_hash = bank_forks.read().unwrap().bank_hash(1).unwrap();
        let children_of_1 = (&heaviest_subtree_fork_choice)
            .children(&(1, bank_1_hash))
            .unwrap();
        let duplicate_versions_of_2 = children_of_1.filter(|(slot, _hash)| *slot == 2).count();
        assert_eq!(duplicate_versions_of_2, 2);

        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();

        let ancestors = bank_forks.read().unwrap().ancestors();
        let descendants = bank_forks.read().unwrap().descendants();

        ReplayStage::compute_bank_stats(
            &Pubkey::new_unique(),
            &ancestors,
            &mut frozen_banks,
            &mut tower,
            &mut progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &bank_forks,
            &mut heaviest_subtree_fork_choice,
            &mut latest_validator_votes_for_frozen_banks,
        );
        // Try to switch to vote to the heaviest slot 5, then return the vote results
        let (heaviest_bank, heaviest_bank_on_same_fork) = heaviest_subtree_fork_choice
            .select_forks(&frozen_banks, &tower, &progress, &ancestors, &bank_forks);
        assert_eq!(heaviest_bank.slot(), 5);
        assert!(heaviest_bank_on_same_fork.is_none());
        ReplayStage::select_vote_and_reset_forks(
            &heaviest_bank,
            heaviest_bank_on_same_fork.as_ref(),
            &ancestors,
            &descendants,
            &progress,
            &mut tower,
            &latest_validator_votes_for_frozen_banks,
            &heaviest_subtree_fork_choice,
        )
    }

    #[test]
    fn test_duplicate_rollback_then_vote_on_other_duplicate_success() {
        let SelectVoteAndResetForkResult {
            vote_bank,
            reset_bank,
            heaviest_fork_failures,
        } = run_test_duplicate_rollback_then_vote_on_other_duplicate(3);

        // If we vote on 2 first then try to vote on 5, we should succeed
        assert_matches!(
            vote_bank
                .map(|(bank, switch_decision)| (bank.slot(), switch_decision))
                .unwrap(),
            (5, SwitchForkDecision::SwitchProof(_))
        );
        assert_eq!(reset_bank.unwrap().slot(), 5);
        assert!(heaviest_fork_failures.is_empty());
    }

    #[test]
    fn test_duplicate_rollback_then_vote_on_other_duplicate_same_slot_locked_out() {
        let SelectVoteAndResetForkResult {
            vote_bank,
            reset_bank,
            heaviest_fork_failures,
        } = run_test_duplicate_rollback_then_vote_on_other_duplicate(5);

        // If we vote on 5 first then try to vote on another version of 5,
        // lockout should fail
        assert!(vote_bank.is_none());
        assert_eq!(reset_bank.unwrap().slot(), 5);
        assert_eq!(
            heaviest_fork_failures,
            vec![HeaviestForkFailures::LockedOut(5)]
        );
    }

    #[test]
    #[ignore]
    fn test_duplicate_rollback_then_vote_on_other_duplicate_different_slot_locked_out() {
        let SelectVoteAndResetForkResult {
            vote_bank,
            reset_bank,
            heaviest_fork_failures,
        } = run_test_duplicate_rollback_then_vote_on_other_duplicate(4);

        // If we vote on 4 first then try to vote on 5 descended from another version
        // of 4, lockout should fail
        assert!(vote_bank.is_none());
        assert_eq!(reset_bank.unwrap().slot(), 5);
        assert_eq!(
            heaviest_fork_failures,
            vec![HeaviestForkFailures::LockedOut(5)]
        );
    }

    #[test]
    fn test_gossip_vote_doesnt_affect_fork_choice() {
        let (
            VoteSimulator {
                bank_forks,
                mut heaviest_subtree_fork_choice,
                mut latest_validator_votes_for_frozen_banks,
                vote_pubkeys,
                ..
            },
            _,
        ) = setup_default_forks(1, None::<GenerateVotes>);

        let vote_pubkey = vote_pubkeys[0];
        let mut unfrozen_gossip_verified_vote_hashes = UnfrozenGossipVerifiedVoteHashes::default();
        let (gossip_verified_vote_hash_sender, gossip_verified_vote_hash_receiver) = unbounded();

        // Best slot is 4
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 4);

        // Cast a vote for slot 3 on one fork
        let vote_slot = 3;
        let vote_bank = bank_forks.read().unwrap().get(vote_slot).unwrap();
        gossip_verified_vote_hash_sender
            .send((vote_pubkey, vote_slot, vote_bank.hash()))
            .expect("Send should succeed");
        ReplayStage::process_gossip_verified_vote_hashes(
            &gossip_verified_vote_hash_receiver,
            &mut unfrozen_gossip_verified_vote_hashes,
            &heaviest_subtree_fork_choice,
            &mut latest_validator_votes_for_frozen_banks,
        );

        // Pick the best fork. Gossip votes shouldn't affect fork choice
        heaviest_subtree_fork_choice.compute_bank_stats(
            &vote_bank,
            &Tower::default(),
            &mut latest_validator_votes_for_frozen_banks,
        );

        // Best slot is still 4
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 4);
    }

    #[test]
    fn test_replay_stage_refresh_last_vote() {
        let ReplayBlockstoreComponents {
            cluster_info,
            poh_recorder,
            mut tower,
            my_pubkey,
            vote_simulator,
            ..
        } = replay_blockstore_components(None, 10, None::<GenerateVotes>);
        let tower_storage = NullTowerStorage::default();

        let VoteSimulator {
            mut validator_keypairs,
            bank_forks,
            ..
        } = vote_simulator;

        let mut last_vote_refresh_time = LastVoteRefreshTime {
            last_refresh_time: Instant::now(),
            last_print_time: Instant::now(),
        };
        let has_new_vote_been_rooted = false;
        let mut voted_signatures = vec![];

        let identity_keypair = cluster_info.keypair().clone();
        let my_vote_keypair = vec![Arc::new(
            validator_keypairs.remove(&my_pubkey).unwrap().vote_keypair,
        )];
        let my_vote_pubkey = my_vote_keypair[0].pubkey();
        let bank0 = bank_forks.read().unwrap().get(0).unwrap();

        bank0.set_initial_accounts_hash_verification_completed();

        let (voting_sender, voting_receiver) = unbounded();

        // Simulate landing a vote for slot 0 landing in slot 1
        let bank1 = Arc::new(Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 1));
        bank1.fill_bank_with_ticks_for_tests();
        tower.record_bank_vote(&bank0);
        ReplayStage::push_vote(
            &bank0,
            &my_vote_pubkey,
            &identity_keypair,
            &my_vote_keypair,
            &mut tower,
            &SwitchForkDecision::SameFork,
            &mut voted_signatures,
            has_new_vote_been_rooted,
            &mut ReplayTiming::default(),
            &voting_sender,
            None,
        );
        let vote_info = voting_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        crate::voting_service::VotingService::handle_vote(
            &cluster_info,
            &poh_recorder,
            &tower_storage,
            vote_info,
        );

        let mut cursor = Cursor::default();
        let votes = cluster_info.get_votes(&mut cursor);
        assert_eq!(votes.len(), 1);
        let vote_tx = &votes[0];
        assert_eq!(vote_tx.message.recent_blockhash, bank0.last_blockhash());
        assert_eq!(
            tower.last_vote_tx_blockhash().unwrap(),
            bank0.last_blockhash()
        );
        assert_eq!(tower.last_voted_slot().unwrap(), 0);
        bank1.process_transaction(vote_tx).unwrap();
        bank1.freeze();

        // Trying to refresh the vote for bank 0 in bank 1 or bank 2 won't succeed because
        // the last vote has landed already
        let bank2 = Arc::new(Bank::new_from_parent(bank1.clone(), &Pubkey::default(), 2));
        bank2.fill_bank_with_ticks_for_tests();
        bank2.freeze();
        for refresh_bank in &[&bank1, &bank2] {
            ReplayStage::refresh_last_vote(
                &mut tower,
                refresh_bank,
                Tower::last_voted_slot_in_bank(refresh_bank, &my_vote_pubkey).unwrap(),
                &my_vote_pubkey,
                &identity_keypair,
                &my_vote_keypair,
                &mut voted_signatures,
                has_new_vote_been_rooted,
                &mut last_vote_refresh_time,
                &voting_sender,
                None,
            );

            // No new votes have been submitted to gossip
            let votes = cluster_info.get_votes(&mut cursor);
            assert!(votes.is_empty());
            // Tower's latest vote tx blockhash hasn't changed either
            assert_eq!(
                tower.last_vote_tx_blockhash().unwrap(),
                bank0.last_blockhash()
            );
            assert_eq!(tower.last_voted_slot().unwrap(), 0);
        }

        // Simulate submitting a new vote for bank 1 to the network, but the vote
        // not landing
        tower.record_bank_vote(&bank1);
        ReplayStage::push_vote(
            &bank1,
            &my_vote_pubkey,
            &identity_keypair,
            &my_vote_keypair,
            &mut tower,
            &SwitchForkDecision::SameFork,
            &mut voted_signatures,
            has_new_vote_been_rooted,
            &mut ReplayTiming::default(),
            &voting_sender,
            None,
        );
        let vote_info = voting_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        crate::voting_service::VotingService::handle_vote(
            &cluster_info,
            &poh_recorder,
            &tower_storage,
            vote_info,
        );
        let votes = cluster_info.get_votes(&mut cursor);
        assert_eq!(votes.len(), 1);
        let vote_tx = &votes[0];
        assert_eq!(vote_tx.message.recent_blockhash, bank1.last_blockhash());
        assert_eq!(
            tower.last_vote_tx_blockhash().unwrap(),
            bank1.last_blockhash()
        );
        assert_eq!(tower.last_voted_slot().unwrap(), 1);

        // Trying to refresh the vote for bank 1 in bank 2 won't succeed because
        // the last vote has not expired yet
        ReplayStage::refresh_last_vote(
            &mut tower,
            &bank2,
            Tower::last_voted_slot_in_bank(&bank2, &my_vote_pubkey).unwrap(),
            &my_vote_pubkey,
            &identity_keypair,
            &my_vote_keypair,
            &mut voted_signatures,
            has_new_vote_been_rooted,
            &mut last_vote_refresh_time,
            &voting_sender,
            None,
        );

        // No new votes have been submitted to gossip
        let votes = cluster_info.get_votes(&mut cursor);
        assert!(votes.is_empty());
        assert_eq!(
            tower.last_vote_tx_blockhash().unwrap(),
            bank1.last_blockhash()
        );
        assert_eq!(tower.last_voted_slot().unwrap(), 1);

        // Create a bank where the last vote transaction will have expired
        let expired_bank = {
            let mut parent_bank = bank2.clone();
            for _ in 0..MAX_PROCESSING_AGE {
                let slot = parent_bank.slot() + 1;
                parent_bank =
                    Arc::new(Bank::new_from_parent(parent_bank, &Pubkey::default(), slot));
                parent_bank.fill_bank_with_ticks_for_tests();
                parent_bank.freeze();
            }
            parent_bank
        };

        // Now trying to refresh the vote for slot 1 will succeed because the recent blockhash
        // of the last vote transaction has expired
        last_vote_refresh_time.last_refresh_time = last_vote_refresh_time
            .last_refresh_time
            .checked_sub(Duration::from_millis(
                MAX_VOTE_REFRESH_INTERVAL_MILLIS as u64 + 1,
            ))
            .unwrap();
        let clone_refresh_time = last_vote_refresh_time.last_refresh_time;
        ReplayStage::refresh_last_vote(
            &mut tower,
            &expired_bank,
            Tower::last_voted_slot_in_bank(&expired_bank, &my_vote_pubkey).unwrap(),
            &my_vote_pubkey,
            &identity_keypair,
            &my_vote_keypair,
            &mut voted_signatures,
            has_new_vote_been_rooted,
            &mut last_vote_refresh_time,
            &voting_sender,
            None,
        );
        let vote_info = voting_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        crate::voting_service::VotingService::handle_vote(
            &cluster_info,
            &poh_recorder,
            &tower_storage,
            vote_info,
        );

        assert!(last_vote_refresh_time.last_refresh_time > clone_refresh_time);
        let votes = cluster_info.get_votes(&mut cursor);
        assert_eq!(votes.len(), 1);
        let vote_tx = &votes[0];
        assert_eq!(
            vote_tx.message.recent_blockhash,
            expired_bank.last_blockhash()
        );
        assert_eq!(
            tower.last_vote_tx_blockhash().unwrap(),
            expired_bank.last_blockhash()
        );
        assert_eq!(tower.last_voted_slot().unwrap(), 1);

        // Processing the vote transaction should be valid
        let expired_bank_child_slot = expired_bank.slot() + 1;
        let expired_bank_child = Arc::new(Bank::new_from_parent(
            expired_bank.clone(),
            &Pubkey::default(),
            expired_bank_child_slot,
        ));
        expired_bank_child.process_transaction(vote_tx).unwrap();
        let vote_account = expired_bank_child
            .get_vote_account(&my_vote_pubkey)
            .unwrap();
        assert_eq!(
            vote_account.vote_state().as_ref().unwrap().tower(),
            vec![0, 1]
        );
        expired_bank_child.fill_bank_with_ticks_for_tests();
        expired_bank_child.freeze();

        // Trying to refresh the vote on a sibling bank where:
        // 1) The vote for slot 1 hasn't landed
        // 2) The latest refresh vote transaction's recent blockhash (the sibling's hash) doesn't exist
        // This will still not refresh because `MAX_VOTE_REFRESH_INTERVAL_MILLIS` has not expired yet
        let expired_bank_sibling = Arc::new(Bank::new_from_parent(
            bank2,
            &Pubkey::default(),
            expired_bank_child.slot() + 1,
        ));
        expired_bank_sibling.fill_bank_with_ticks_for_tests();
        expired_bank_sibling.freeze();
        // Set the last refresh to now, shouldn't refresh because the last refresh just happened.
        last_vote_refresh_time.last_refresh_time = Instant::now();
        ReplayStage::refresh_last_vote(
            &mut tower,
            &expired_bank_sibling,
            Tower::last_voted_slot_in_bank(&expired_bank_sibling, &my_vote_pubkey).unwrap(),
            &my_vote_pubkey,
            &identity_keypair,
            &my_vote_keypair,
            &mut voted_signatures,
            has_new_vote_been_rooted,
            &mut last_vote_refresh_time,
            &voting_sender,
            None,
        );

        let votes = cluster_info.get_votes(&mut cursor);
        assert!(votes.is_empty());
        assert_eq!(
            vote_tx.message.recent_blockhash,
            expired_bank.last_blockhash()
        );
        assert_eq!(
            tower.last_vote_tx_blockhash().unwrap(),
            expired_bank.last_blockhash()
        );
        assert_eq!(tower.last_voted_slot().unwrap(), 1);
    }

    #[allow(clippy::too_many_arguments)]
    fn send_vote_in_new_bank(
        parent_bank: Arc<Bank>,
        my_slot: Slot,
        my_vote_keypair: &[Arc<Keypair>],
        tower: &mut Tower,
        identity_keypair: &Keypair,
        voted_signatures: &mut Vec<Signature>,
        has_new_vote_been_rooted: bool,
        voting_sender: &Sender<VoteOp>,
        voting_receiver: &Receiver<VoteOp>,
        cluster_info: &ClusterInfo,
        poh_recorder: &RwLock<PohRecorder>,
        tower_storage: &dyn TowerStorage,
        make_it_landing: bool,
        cursor: &mut Cursor,
        bank_forks: &RwLock<BankForks>,
        progress: &mut ProgressMap,
    ) -> Arc<Bank> {
        let my_vote_pubkey = &my_vote_keypair[0].pubkey();
        tower.record_bank_vote(&parent_bank);
        ReplayStage::push_vote(
            &parent_bank,
            my_vote_pubkey,
            identity_keypair,
            my_vote_keypair,
            tower,
            &SwitchForkDecision::SameFork,
            voted_signatures,
            has_new_vote_been_rooted,
            &mut ReplayTiming::default(),
            voting_sender,
            None,
        );
        let vote_info = voting_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        crate::voting_service::VotingService::handle_vote(
            cluster_info,
            poh_recorder,
            tower_storage,
            vote_info,
        );

        let votes = cluster_info.get_votes(cursor);
        assert_eq!(votes.len(), 1);
        let vote_tx = &votes[0];
        assert_eq!(
            vote_tx.message.recent_blockhash,
            parent_bank.last_blockhash()
        );
        assert_eq!(
            tower.last_vote_tx_blockhash().unwrap(),
            parent_bank.last_blockhash()
        );
        assert_eq!(tower.last_voted_slot().unwrap(), parent_bank.slot());
        let bank = Bank::new_from_parent(parent_bank, &Pubkey::default(), my_slot);
        bank.fill_bank_with_ticks_for_tests();
        if make_it_landing {
            bank.process_transaction(vote_tx).unwrap();
        }
        bank.freeze();
        progress.entry(my_slot).or_insert_with(|| {
            ForkProgress::new_from_bank(
                &bank,
                &identity_keypair.pubkey(),
                my_vote_pubkey,
                None,
                0,
                0,
            )
        });
        bank_forks.write().unwrap().insert(bank);
        bank_forks.read().unwrap().get(my_slot).unwrap()
    }

    #[test]
    fn test_replay_stage_last_vote_outside_slot_hashes() {
        solana_logger::setup();
        let ReplayBlockstoreComponents {
            cluster_info,
            poh_recorder,
            mut tower,
            my_pubkey,
            vote_simulator,
            ..
        } = replay_blockstore_components(None, 10, None::<GenerateVotes>);
        let tower_storage = NullTowerStorage::default();

        let VoteSimulator {
            mut validator_keypairs,
            bank_forks,
            mut heaviest_subtree_fork_choice,
            mut latest_validator_votes_for_frozen_banks,
            mut progress,
            ..
        } = vote_simulator;

        let has_new_vote_been_rooted = false;
        let mut voted_signatures = vec![];

        let identity_keypair = cluster_info.keypair().clone();
        let my_vote_keypair = vec![Arc::new(
            validator_keypairs.remove(&my_pubkey).unwrap().vote_keypair,
        )];
        let my_vote_pubkey = my_vote_keypair[0].pubkey();
        let bank0 = bank_forks.read().unwrap().get(0).unwrap();

        bank0.set_initial_accounts_hash_verification_completed();

        // Add a new fork starting from 0 with bigger slot number, we assume it has a bigger
        // weight, but we cannot switch because of lockout.
        let other_fork_slot = 1;
        let other_fork_bank =
            Bank::new_from_parent(bank0.clone(), &Pubkey::default(), other_fork_slot);
        other_fork_bank.fill_bank_with_ticks_for_tests();
        other_fork_bank.freeze();
        progress.entry(other_fork_slot).or_insert_with(|| {
            ForkProgress::new_from_bank(
                &other_fork_bank,
                &identity_keypair.pubkey(),
                &my_vote_keypair[0].pubkey(),
                None,
                0,
                0,
            )
        });
        bank_forks.write().unwrap().insert(other_fork_bank);

        let (voting_sender, voting_receiver) = unbounded();
        let mut cursor = Cursor::default();

        let mut new_bank = send_vote_in_new_bank(
            bank0,
            2,
            &my_vote_keypair,
            &mut tower,
            &identity_keypair,
            &mut voted_signatures,
            has_new_vote_been_rooted,
            &voting_sender,
            &voting_receiver,
            &cluster_info,
            &poh_recorder,
            &tower_storage,
            true,
            &mut cursor,
            &bank_forks,
            &mut progress,
        );
        new_bank = send_vote_in_new_bank(
            new_bank.clone(),
            new_bank.slot() + 1,
            &my_vote_keypair,
            &mut tower,
            &identity_keypair,
            &mut voted_signatures,
            has_new_vote_been_rooted,
            &voting_sender,
            &voting_receiver,
            &cluster_info,
            &poh_recorder,
            &tower_storage,
            false,
            &mut cursor,
            &bank_forks,
            &mut progress,
        );
        // Create enough banks on the fork so last vote is outside SlotHash, make sure
        // we now vote at the tip of the fork.
        let last_voted_slot = tower.last_voted_slot().unwrap();
        while new_bank.is_in_slot_hashes_history(&last_voted_slot) {
            let new_slot = new_bank.slot() + 1;
            let bank = Bank::new_from_parent(new_bank, &Pubkey::default(), new_slot);
            bank.fill_bank_with_ticks_for_tests();
            bank.freeze();
            progress.entry(new_slot).or_insert_with(|| {
                ForkProgress::new_from_bank(
                    &bank,
                    &identity_keypair.pubkey(),
                    &my_vote_keypair[0].pubkey(),
                    None,
                    0,
                    0,
                )
            });
            bank_forks.write().unwrap().insert(bank);
            new_bank = bank_forks.read().unwrap().get(new_slot).unwrap();
        }
        let tip_of_voted_fork = new_bank.slot();

        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();
        ReplayStage::compute_bank_stats(
            &my_vote_pubkey,
            &bank_forks.read().unwrap().ancestors(),
            &mut frozen_banks,
            &mut tower,
            &mut progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            &bank_forks,
            &mut heaviest_subtree_fork_choice,
            &mut latest_validator_votes_for_frozen_banks,
        );
        assert_eq!(tower.last_voted_slot(), Some(last_voted_slot));
        assert_eq!(progress.my_latest_landed_vote(tip_of_voted_fork), Some(0));
        let other_fork_bank = &bank_forks.read().unwrap().get(other_fork_slot).unwrap();
        let SelectVoteAndResetForkResult { vote_bank, .. } =
            ReplayStage::select_vote_and_reset_forks(
                other_fork_bank,
                Some(&new_bank),
                &bank_forks.read().unwrap().ancestors(),
                &bank_forks.read().unwrap().descendants(),
                &progress,
                &mut tower,
                &latest_validator_votes_for_frozen_banks,
                &heaviest_subtree_fork_choice,
            );
        assert!(vote_bank.is_some());
        assert_eq!(vote_bank.unwrap().0.slot(), tip_of_voted_fork);

        // If last vote is already equal to heaviest_bank_on_same_voted_fork,
        // we should not vote.
        let last_voted_bank = &bank_forks.read().unwrap().get(last_voted_slot).unwrap();
        let SelectVoteAndResetForkResult { vote_bank, .. } =
            ReplayStage::select_vote_and_reset_forks(
                other_fork_bank,
                Some(last_voted_bank),
                &bank_forks.read().unwrap().ancestors(),
                &bank_forks.read().unwrap().descendants(),
                &progress,
                &mut tower,
                &latest_validator_votes_for_frozen_banks,
                &heaviest_subtree_fork_choice,
            );
        assert!(vote_bank.is_none());

        // If last vote is still inside slot hashes history of heaviest_bank_on_same_voted_fork,
        // we should not vote.
        let last_voted_bank_plus_1 = &bank_forks.read().unwrap().get(last_voted_slot + 1).unwrap();
        let SelectVoteAndResetForkResult { vote_bank, .. } =
            ReplayStage::select_vote_and_reset_forks(
                other_fork_bank,
                Some(last_voted_bank_plus_1),
                &bank_forks.read().unwrap().ancestors(),
                &bank_forks.read().unwrap().descendants(),
                &progress,
                &mut tower,
                &latest_validator_votes_for_frozen_banks,
                &heaviest_subtree_fork_choice,
            );
        assert!(vote_bank.is_none());

        // create a new bank and make last_voted_slot land, we should not vote.
        progress
            .entry(new_bank.slot())
            .and_modify(|s| s.fork_stats.my_latest_landed_vote = Some(last_voted_slot));
        assert!(!new_bank.is_in_slot_hashes_history(&last_voted_slot));
        let SelectVoteAndResetForkResult { vote_bank, .. } =
            ReplayStage::select_vote_and_reset_forks(
                other_fork_bank,
                Some(&new_bank),
                &bank_forks.read().unwrap().ancestors(),
                &bank_forks.read().unwrap().descendants(),
                &progress,
                &mut tower,
                &latest_validator_votes_for_frozen_banks,
                &heaviest_subtree_fork_choice,
            );
        assert!(vote_bank.is_none());
    }

    #[test]
    fn test_retransmit_latest_unpropagated_leader_slot() {
        let ReplayBlockstoreComponents {
            validator_node_to_vote_keys,
            leader_schedule_cache,
            poh_recorder,
            vote_simulator,
            ..
        } = replay_blockstore_components(None, 10, None::<GenerateVotes>);

        let VoteSimulator {
            mut progress,
            ref bank_forks,
            ..
        } = vote_simulator;

        let poh_recorder = Arc::new(poh_recorder);
        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();

        let bank1 = Bank::new_from_parent(
            bank_forks.read().unwrap().get(0).unwrap(),
            &leader_schedule_cache.slot_leader_at(1, None).unwrap(),
            1,
        );
        progress.insert(
            1,
            ForkProgress::new_from_bank(
                &bank1,
                bank1.collector_id(),
                validator_node_to_vote_keys
                    .get(bank1.collector_id())
                    .unwrap(),
                Some(0),
                0,
                0,
            ),
        );
        assert!(progress.get_propagated_stats(1).unwrap().is_leader_slot);
        bank1.freeze();
        bank_forks.write().unwrap().insert(bank1);

        progress.get_retransmit_info_mut(0).unwrap().retry_time = Instant::now();
        ReplayStage::retransmit_latest_unpropagated_leader_slot(
            &poh_recorder,
            &retransmit_slots_sender,
            &mut progress,
        );
        let res = retransmit_slots_receiver.recv_timeout(Duration::from_millis(10));
        assert_matches!(res, Err(_));
        assert_eq!(
            progress.get_retransmit_info(0).unwrap().retry_iteration,
            0,
            "retransmit should not advance retry_iteration before time has been set"
        );

        ReplayStage::retransmit_latest_unpropagated_leader_slot(
            &poh_recorder,
            &retransmit_slots_sender,
            &mut progress,
        );
        let res = retransmit_slots_receiver.recv_timeout(Duration::from_millis(10));
        assert!(
            res.is_err(),
            "retry_iteration=0, elapsed < 2^0 * RETRANSMIT_BASE_DELAY_MS"
        );

        progress.get_retransmit_info_mut(0).unwrap().retry_time = Instant::now()
            .checked_sub(Duration::from_millis(RETRANSMIT_BASE_DELAY_MS + 1))
            .unwrap();
        ReplayStage::retransmit_latest_unpropagated_leader_slot(
            &poh_recorder,
            &retransmit_slots_sender,
            &mut progress,
        );
        let res = retransmit_slots_receiver.recv_timeout(Duration::from_millis(10));
        assert!(
            res.is_ok(),
            "retry_iteration=0, elapsed > RETRANSMIT_BASE_DELAY_MS"
        );
        assert_eq!(
            progress.get_retransmit_info(0).unwrap().retry_iteration,
            1,
            "retransmit should advance retry_iteration"
        );

        ReplayStage::retransmit_latest_unpropagated_leader_slot(
            &poh_recorder,
            &retransmit_slots_sender,
            &mut progress,
        );
        let res = retransmit_slots_receiver.recv_timeout(Duration::from_millis(10));
        assert!(
            res.is_err(),
            "retry_iteration=1, elapsed < 2^1 * RETRY_BASE_DELAY_MS"
        );

        progress.get_retransmit_info_mut(0).unwrap().retry_time = Instant::now()
            .checked_sub(Duration::from_millis(RETRANSMIT_BASE_DELAY_MS + 1))
            .unwrap();
        ReplayStage::retransmit_latest_unpropagated_leader_slot(
            &poh_recorder,
            &retransmit_slots_sender,
            &mut progress,
        );
        let res = retransmit_slots_receiver.recv_timeout(Duration::from_millis(10));
        assert!(
            res.is_err(),
            "retry_iteration=1, elapsed < 2^1 * RETRANSMIT_BASE_DELAY_MS"
        );

        progress.get_retransmit_info_mut(0).unwrap().retry_time = Instant::now()
            .checked_sub(Duration::from_millis(2 * RETRANSMIT_BASE_DELAY_MS + 1))
            .unwrap();
        ReplayStage::retransmit_latest_unpropagated_leader_slot(
            &poh_recorder,
            &retransmit_slots_sender,
            &mut progress,
        );
        let res = retransmit_slots_receiver.recv_timeout(Duration::from_millis(10));
        assert!(
            res.is_ok(),
            "retry_iteration=1, elapsed > 2^1 * RETRANSMIT_BASE_DELAY_MS"
        );
        assert_eq!(
            progress.get_retransmit_info(0).unwrap().retry_iteration,
            2,
            "retransmit should advance retry_iteration"
        );

        // increment to retry iteration 3
        progress
            .get_retransmit_info_mut(0)
            .unwrap()
            .increment_retry_iteration();

        progress.get_retransmit_info_mut(0).unwrap().retry_time = Instant::now()
            .checked_sub(Duration::from_millis(2 * RETRANSMIT_BASE_DELAY_MS + 1))
            .unwrap();
        ReplayStage::retransmit_latest_unpropagated_leader_slot(
            &poh_recorder,
            &retransmit_slots_sender,
            &mut progress,
        );
        let res = retransmit_slots_receiver.recv_timeout(Duration::from_millis(10));
        assert!(
            res.is_err(),
            "retry_iteration=3, elapsed < 2^3 * RETRANSMIT_BASE_DELAY_MS"
        );

        progress.get_retransmit_info_mut(0).unwrap().retry_time = Instant::now()
            .checked_sub(Duration::from_millis(8 * RETRANSMIT_BASE_DELAY_MS + 1))
            .unwrap();
        ReplayStage::retransmit_latest_unpropagated_leader_slot(
            &poh_recorder,
            &retransmit_slots_sender,
            &mut progress,
        );
        let res = retransmit_slots_receiver.recv_timeout(Duration::from_millis(10));
        assert!(
            res.is_ok(),
            "retry_iteration=3, elapsed > 2^3 * RETRANSMIT_BASE_DELAY"
        );
        assert_eq!(
            progress.get_retransmit_info(0).unwrap().retry_iteration,
            4,
            "retransmit should advance retry_iteration"
        );
    }

    fn receive_slots(retransmit_slots_receiver: &Receiver<Slot>) -> Vec<Slot> {
        let mut slots = Vec::default();
        while let Ok(slot) = retransmit_slots_receiver.recv_timeout(Duration::from_millis(10)) {
            slots.push(slot);
        }
        slots
    }

    #[test]
    fn test_maybe_retransmit_unpropagated_slots() {
        let ReplayBlockstoreComponents {
            validator_node_to_vote_keys,
            leader_schedule_cache,
            vote_simulator,
            ..
        } = replay_blockstore_components(None, 10, None::<GenerateVotes>);

        let VoteSimulator {
            mut progress,
            ref bank_forks,
            ..
        } = vote_simulator;

        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let retry_time = Instant::now()
            .checked_sub(Duration::from_millis(RETRANSMIT_BASE_DELAY_MS + 1))
            .unwrap();
        progress.get_retransmit_info_mut(0).unwrap().retry_time = retry_time;

        let mut prev_index = 0;
        for i in (1..10).chain(11..15) {
            let bank = Bank::new_from_parent(
                bank_forks.read().unwrap().get(prev_index).unwrap(),
                &leader_schedule_cache.slot_leader_at(i, None).unwrap(),
                i,
            );
            progress.insert(
                i,
                ForkProgress::new_from_bank(
                    &bank,
                    bank.collector_id(),
                    validator_node_to_vote_keys
                        .get(bank.collector_id())
                        .unwrap(),
                    Some(0),
                    0,
                    0,
                ),
            );
            assert!(progress.get_propagated_stats(i).unwrap().is_leader_slot);
            bank.freeze();
            bank_forks.write().unwrap().insert(bank);
            prev_index = i;
            progress.get_retransmit_info_mut(i).unwrap().retry_time = retry_time;
        }

        // expect single slot when latest_leader_slot is the start of a consecutive range
        let latest_leader_slot = 0;
        ReplayStage::maybe_retransmit_unpropagated_slots(
            "test",
            &retransmit_slots_sender,
            &mut progress,
            latest_leader_slot,
        );
        let received_slots = receive_slots(&retransmit_slots_receiver);
        assert_eq!(received_slots, vec![0]);

        // expect range of slots from start of consecutive slots
        let latest_leader_slot = 6;
        ReplayStage::maybe_retransmit_unpropagated_slots(
            "test",
            &retransmit_slots_sender,
            &mut progress,
            latest_leader_slot,
        );
        let received_slots = receive_slots(&retransmit_slots_receiver);
        assert_eq!(received_slots, vec![4, 5, 6]);

        // expect range of slots skipping a discontinuity in the range
        let latest_leader_slot = 11;
        ReplayStage::maybe_retransmit_unpropagated_slots(
            "test",
            &retransmit_slots_sender,
            &mut progress,
            latest_leader_slot,
        );
        let received_slots = receive_slots(&retransmit_slots_receiver);
        assert_eq!(received_slots, vec![8, 9, 11]);
    }

    #[test]
    #[should_panic(expected = "We are attempting to dump a block that we produced")]
    fn test_dump_own_slots_fails() {
        // Create the tree of banks in a BankForks object
        let forks = tr(0) / (tr(1)) / (tr(2));

        let ReplayBlockstoreComponents {
            ref mut vote_simulator,
            ref blockstore,
            ref my_pubkey,
            ref leader_schedule_cache,
            ..
        } = replay_blockstore_components(Some(forks), 1, None);

        let VoteSimulator {
            ref mut progress,
            ref bank_forks,
            ..
        } = vote_simulator;

        let (mut ancestors, mut descendants) = {
            let r_bank_forks = bank_forks.read().unwrap();
            (r_bank_forks.ancestors(), r_bank_forks.descendants())
        };

        // Insert different versions of both 1 and 2. Although normally these slots would be dumped,
        // because we were the leader for these slots we should panic
        let mut duplicate_slots_to_repair = DuplicateSlotsToRepair::default();
        duplicate_slots_to_repair.insert(1, Hash::new_unique());
        duplicate_slots_to_repair.insert(2, Hash::new_unique());
        let mut purge_repair_slot_counter = PurgeRepairSlotCounter::default();
        let (dumped_slots_sender, _) = unbounded();

        ReplayStage::dump_then_repair_correct_slots(
            &mut duplicate_slots_to_repair,
            &mut ancestors,
            &mut descendants,
            progress,
            bank_forks,
            blockstore,
            None,
            &mut purge_repair_slot_counter,
            &dumped_slots_sender,
            my_pubkey,
            leader_schedule_cache,
        );
    }

    fn run_compute_and_select_forks(
        bank_forks: &RwLock<BankForks>,
        progress: &mut ProgressMap,
        tower: &mut Tower,
        heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
        latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
        my_vote_pubkey: Option<Pubkey>,
    ) -> (Option<Slot>, Option<Slot>, Vec<HeaviestForkFailures>) {
        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();
        let ancestors = &bank_forks.read().unwrap().ancestors();
        let descendants = &bank_forks.read().unwrap().descendants();
        ReplayStage::compute_bank_stats(
            &my_vote_pubkey.unwrap_or_default(),
            &bank_forks.read().unwrap().ancestors(),
            &mut frozen_banks,
            tower,
            progress,
            &VoteTracker::default(),
            &ClusterSlots::default(),
            bank_forks,
            heaviest_subtree_fork_choice,
            latest_validator_votes_for_frozen_banks,
        );
        let (heaviest_bank, heaviest_bank_on_same_fork) = heaviest_subtree_fork_choice
            .select_forks(&frozen_banks, tower, progress, ancestors, bank_forks);
        let SelectVoteAndResetForkResult {
            vote_bank,
            reset_bank,
            heaviest_fork_failures,
        } = ReplayStage::select_vote_and_reset_forks(
            &heaviest_bank,
            heaviest_bank_on_same_fork.as_ref(),
            ancestors,
            descendants,
            progress,
            tower,
            latest_validator_votes_for_frozen_banks,
            heaviest_subtree_fork_choice,
        );
        (
            vote_bank.map(|(b, _)| b.slot()),
            reset_bank.map(|b| b.slot()),
            heaviest_fork_failures,
        )
    }

    type GenerateVotes = Box<dyn Fn(Vec<Pubkey>) -> HashMap<Pubkey, Vec<Slot>>>;

    pub fn setup_forks_from_tree(
        tree: Tree<Slot>,
        num_keys: usize,
        generate_votes: Option<GenerateVotes>,
    ) -> (VoteSimulator, Blockstore) {
        let mut vote_simulator = VoteSimulator::new(num_keys);
        let pubkeys: Vec<Pubkey> = vote_simulator
            .validator_keypairs
            .values()
            .map(|k| k.node_keypair.pubkey())
            .collect();
        let cluster_votes = generate_votes
            .map(|generate_votes| generate_votes(pubkeys))
            .unwrap_or_default();
        vote_simulator.fill_bank_forks(tree.clone(), &cluster_votes, true);
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        blockstore.add_tree(tree, false, true, 2, Hash::default());
        (vote_simulator, blockstore)
    }

    fn setup_default_forks(
        num_keys: usize,
        generate_votes: Option<GenerateVotes>,
    ) -> (VoteSimulator, Blockstore) {
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

        let tree = tr(0) / (tr(1) / (tr(2) / (tr(4))) / (tr(3) / (tr(5) / (tr(6)))));
        setup_forks_from_tree(tree, num_keys, generate_votes)
    }

    fn check_map_eq<K: Eq + std::hash::Hash + std::fmt::Debug, T: PartialEq + std::fmt::Debug>(
        map1: &HashMap<K, T>,
        map2: &HashMap<K, T>,
    ) -> bool {
        map1.len() == map2.len() && map1.iter().all(|(k, v)| map2.get(k).unwrap() == v)
    }

    #[test]
    fn test_check_for_vote_only_mode() {
        let in_vote_only_mode = AtomicBool::new(false);
        let genesis_config = create_genesis_config(10_000).genesis_config;
        let bank0 = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank0);
        ReplayStage::check_for_vote_only_mode(1000, 0, &in_vote_only_mode, &bank_forks);
        assert!(in_vote_only_mode.load(Ordering::Relaxed));
        ReplayStage::check_for_vote_only_mode(10, 0, &in_vote_only_mode, &bank_forks);
        assert!(!in_vote_only_mode.load(Ordering::Relaxed));
    }

    #[test]
    fn test_tower_sync_from_bank_failed_switch() {
        solana_logger::setup_with_default(
            "error,solana_core::replay_stage=info,solana_core::consensus=info",
        );
        /*
            Fork structure:

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

            We had some point voted 0 - 6, while the rest of the network voted 0 - 4.
            We are sitting with an oudated tower that has voted until 1. We see that 4 is the heaviest slot,
            however in the past we have voted up to 6. We must acknowledge the vote state present at 6,
            adopt it as our own and *not* vote on 2 or 4, to respect slashing rules as there is
            not enough stake to switch
        */

        let generate_votes = |pubkeys: Vec<Pubkey>| {
            pubkeys
                .into_iter()
                .zip(iter::once(vec![0, 1, 3, 5, 6]).chain(iter::repeat(vec![0, 1, 2, 4]).take(2)))
                .collect()
        };
        let (mut vote_simulator, _blockstore) =
            setup_default_forks(3, Some(Box::new(generate_votes)));
        let (bank_forks, mut progress) = (vote_simulator.bank_forks, vote_simulator.progress);
        let bank_hash = |slot| bank_forks.read().unwrap().bank_hash(slot).unwrap();
        let my_vote_pubkey = vote_simulator.vote_pubkeys[0];
        let mut tower = Tower::default();
        tower.node_pubkey = vote_simulator.node_pubkeys[0];
        tower.record_vote(0, bank_hash(0));
        tower.record_vote(1, bank_hash(1));

        let (vote_fork, reset_fork, failures) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            Some(my_vote_pubkey),
        );

        assert_eq!(vote_fork, None);
        assert_eq!(reset_fork, Some(6));
        assert_eq!(
            failures,
            vec![
                HeaviestForkFailures::FailedSwitchThreshold(4, 0, 30000),
                HeaviestForkFailures::LockedOut(4)
            ]
        );

        let (vote_fork, reset_fork, failures) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            Some(my_vote_pubkey),
        );

        assert_eq!(vote_fork, None);
        assert_eq!(reset_fork, Some(6));
        assert_eq!(
            failures,
            vec![
                HeaviestForkFailures::FailedSwitchThreshold(4, 0, 30000),
                HeaviestForkFailures::LockedOut(4)
            ]
        );
    }

    #[test]
    fn test_tower_sync_from_bank_failed_lockout() {
        solana_logger::setup_with_default(
            "error,solana_core::replay_stage=info,solana_core::consensus=info",
        );
        /*
            Fork structure:

                 slot 0
                   |
                 slot 1
                 /    \
            slot 3    |
               |    slot 2
            slot 4    |
                    slot 5
                      |
                    slot 6

            We had some point voted 0 - 6, while the rest of the network voted 0 - 4.
            We are sitting with an oudated tower that has voted until 1. We see that 4 is the heaviest slot,
            however in the past we have voted up to 6. We must acknowledge the vote state present at 6,
            adopt it as our own and *not* vote on 3 or 4, to respect slashing rules as we are locked
            out on 4, even though there is enough stake to switch. However we should still reset onto
            4.
        */

        let generate_votes = |pubkeys: Vec<Pubkey>| {
            pubkeys
                .into_iter()
                .zip(iter::once(vec![0, 1, 2, 5, 6]).chain(iter::repeat(vec![0, 1, 3, 4]).take(2)))
                .collect()
        };
        let tree = tr(0) / (tr(1) / (tr(3) / (tr(4))) / (tr(2) / (tr(5) / (tr(6)))));
        let (mut vote_simulator, _blockstore) =
            setup_forks_from_tree(tree, 3, Some(Box::new(generate_votes)));
        let (bank_forks, mut progress) = (vote_simulator.bank_forks, vote_simulator.progress);
        let bank_hash = |slot| bank_forks.read().unwrap().bank_hash(slot).unwrap();
        let my_vote_pubkey = vote_simulator.vote_pubkeys[0];
        let mut tower = Tower::default();
        tower.node_pubkey = vote_simulator.node_pubkeys[0];
        tower.record_vote(0, bank_hash(0));
        tower.record_vote(1, bank_hash(1));

        let (vote_fork, reset_fork, failures) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            Some(my_vote_pubkey),
        );

        assert_eq!(vote_fork, None);
        assert_eq!(reset_fork, Some(4));
        assert_eq!(failures, vec![HeaviestForkFailures::LockedOut(4),]);

        let (vote_fork, reset_fork, failures) = run_compute_and_select_forks(
            &bank_forks,
            &mut progress,
            &mut tower,
            &mut vote_simulator.heaviest_subtree_fork_choice,
            &mut vote_simulator.latest_validator_votes_for_frozen_banks,
            Some(my_vote_pubkey),
        );

        assert_eq!(vote_fork, None);
        assert_eq!(reset_fork, Some(4));
        assert_eq!(failures, vec![HeaviestForkFailures::LockedOut(4),]);
    }
}
