//! The `validator` module hosts all the validator microservices.

pub use solana_perf::report_target_features;
use {
    crate::{
        accounts_hash_verifier::{AccountsHashFaultInjector, AccountsHashVerifier},
        admin_rpc_post_init::AdminRpcRequestMetadataPostInit,
        banking_trace::{self, BankingTracer},
        cache_block_meta_service::{CacheBlockMetaSender, CacheBlockMetaService},
        cluster_info_vote_listener::VoteTracker,
        completed_data_sets_service::CompletedDataSetsService,
        consensus::{
            reconcile_blockstore_roots_with_external_source,
            tower_storage::{NullTowerStorage, TowerStorage},
            ExternalRootSource, Tower,
        },
        ledger_metric_report_service::LedgerMetricReportService,
        poh_timing_report_service::PohTimingReportService,
        repair::{self, serve_repair::ServeRepair, serve_repair_service::ServeRepairService},
        rewards_recorder_service::{RewardsRecorderSender, RewardsRecorderService},
        sample_performance_service::SamplePerformanceService,
        sigverify,
        snapshot_packager_service::SnapshotPackagerService,
        stats_reporter_service::StatsReporterService,
        system_monitor_service::{
            verify_net_stats_access, SystemMonitorService, SystemMonitorStatsReportConfig,
        },
        tpu::{Tpu, TpuSockets, DEFAULT_TPU_COALESCE},
        tvu::{Tvu, TvuConfig, TvuSockets},
    },
    crossbeam_channel::{bounded, unbounded, Receiver},
    lazy_static::lazy_static,
    quinn::Endpoint,
    solana_accounts_db::{
        accounts_db::{AccountShrinkThreshold, AccountsDbConfig},
        accounts_index::AccountSecondaryIndexes,
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        hardened_unpack::{open_genesis_config, MAX_GENESIS_ARCHIVE_UNPACKED_SIZE},
    },
    solana_client::connection_cache::{ConnectionCache, Protocol},
    solana_entry::poh::compute_hash_time_ns,
    solana_geyser_plugin_manager::{
        geyser_plugin_service::GeyserPluginService, GeyserPluginManagerRequest,
    },
    solana_gossip::{
        cluster_info::{
            ClusterInfo, Node, DEFAULT_CONTACT_DEBUG_INTERVAL_MILLIS,
            DEFAULT_CONTACT_SAVE_INTERVAL_MILLIS,
        },
        crds_gossip_pull::CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS,
        gossip_service::GossipService,
        legacy_contact_info::LegacyContactInfo as ContactInfo,
    },
    solana_ledger::{
        bank_forks_utils,
        blockstore::{
            Blockstore, BlockstoreError, BlockstoreSignals, CompletedSlotsReceiver, PurgeType,
        },
        blockstore_options::{BlockstoreOptions, BlockstoreRecoveryMode, LedgerColumnOptions},
        blockstore_processor::{self, TransactionStatusSender},
        entry_notifier_interface::EntryNotifierLock,
        entry_notifier_service::{EntryNotifierSender, EntryNotifierService},
        leader_schedule::FixedSchedule,
        leader_schedule_cache::LeaderScheduleCache,
        use_snapshot_archives_at_startup::UseSnapshotArchivesAtStartup,
    },
    solana_measure::measure::Measure,
    solana_metrics::{
        datapoint_info, metrics::metrics_config_sanity_check, poh_timing_point::PohTimingSender,
    },
    solana_poh::{
        poh_recorder::PohRecorder,
        poh_service::{self, PohService},
    },
    solana_rpc::{
        max_slots::MaxSlots,
        optimistically_confirmed_bank_tracker::{
            BankNotificationSenderConfig, OptimisticallyConfirmedBank,
            OptimisticallyConfirmedBankTracker,
        },
        rpc::JsonRpcConfig,
        rpc_completed_slots_service::RpcCompletedSlotsService,
        rpc_pubsub_service::{PubSubConfig, PubSubService},
        rpc_service::JsonRpcService,
        rpc_subscriptions::RpcSubscriptions,
        transaction_notifier_interface::TransactionNotifierLock,
        transaction_status_service::TransactionStatusService,
    },
    solana_runtime::{
        accounts_background_service::{
            AbsRequestHandlers, AbsRequestSender, AccountsBackgroundService, DroppedSlotsReceiver,
            PrunedBanksRequestHandler, SnapshotRequestHandler,
        },
        bank::Bank,
        bank_forks::BankForks,
        commitment::BlockCommitmentCache,
        prioritization_fee_cache::PrioritizationFeeCache,
        runtime_config::RuntimeConfig,
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_bank_utils::{self, DISABLED_SNAPSHOT_ARCHIVE_INTERVAL},
        snapshot_config::SnapshotConfig,
        snapshot_hash::StartingSnapshotHashes,
        snapshot_utils::{
            self, clean_orphaned_account_snapshot_dirs, move_and_async_delete_path_contents,
        },
    },
    solana_sdk::{
        clock::Slot,
        epoch_schedule::MAX_LEADER_SCHEDULE_EPOCH_OFFSET,
        exit::Exit,
        genesis_config::{ClusterType, GenesisConfig},
        hash::Hash,
        pubkey::Pubkey,
        shred_version::compute_shred_version,
        signature::{Keypair, Signer},
        timing::timestamp,
    },
    solana_send_transaction_service::send_transaction_service,
    solana_streamer::{socket::SocketAddrSpace, streamer::StakedNodes},
    solana_turbine::{self, broadcast_stage::BroadcastStageType},
    solana_vote_program::vote_state,
    std::{
        collections::{HashMap, HashSet},
        net::SocketAddr,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, RwLock,
        },
        thread::{sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
    strum::VariantNames,
    strum_macros::{Display, EnumString, EnumVariantNames, IntoStaticStr},
    tokio::runtime::Runtime as TokioRuntime,
};

const MAX_COMPLETED_DATA_SETS_IN_CHANNEL: usize = 100_000;
const WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT: u64 = 80;

#[derive(Clone, EnumString, EnumVariantNames, Default, IntoStaticStr, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum BlockVerificationMethod {
    #[default]
    BlockstoreProcessor,
}

impl BlockVerificationMethod {
    pub const fn cli_names() -> &'static [&'static str] {
        Self::VARIANTS
    }

    pub fn cli_message() -> &'static str {
        lazy_static! {
            static ref MESSAGE: String = format!(
                "Switch transaction scheduling method for verifying ledger entries [default: {}]",
                BlockVerificationMethod::default()
            );
        };

        &MESSAGE
    }
}

#[derive(Clone, EnumString, EnumVariantNames, Default, IntoStaticStr, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum BlockProductionMethod {
    #[default]
    ThreadLocalMultiIterator,
}

impl BlockProductionMethod {
    pub const fn cli_names() -> &'static [&'static str] {
        Self::VARIANTS
    }

    pub fn cli_message() -> &'static str {
        lazy_static! {
            static ref MESSAGE: String = format!(
                "Switch transaction scheduling method for producing ledger entries [default: {}]",
                BlockProductionMethod::default()
            );
        };

        &MESSAGE
    }
}

/// Configuration for the block generator invalidator for replay.
#[derive(Clone, Debug)]
pub struct GeneratorConfig {
    pub accounts_path: String,
    pub starting_keypairs: Arc<Vec<Keypair>>,
}

pub struct ValidatorConfig {
    pub halt_at_slot: Option<Slot>,
    pub expected_genesis_hash: Option<Hash>,
    pub expected_bank_hash: Option<Hash>,
    pub expected_shred_version: Option<u16>,
    pub voting_disabled: bool,
    pub account_paths: Vec<PathBuf>,
    pub account_snapshot_paths: Vec<PathBuf>,
    pub account_shrink_paths: Option<Vec<PathBuf>>,
    pub rpc_config: JsonRpcConfig,
    /// Specifies which plugins to start up with
    pub on_start_geyser_plugin_config_files: Option<Vec<PathBuf>>,
    pub rpc_addrs: Option<(SocketAddr, SocketAddr)>, // (JsonRpc, JsonRpcPubSub)
    pub pubsub_config: PubSubConfig,
    pub snapshot_config: SnapshotConfig,
    pub max_ledger_shreds: Option<u64>,
    pub broadcast_stage_type: BroadcastStageType,
    pub turbine_disabled: Arc<AtomicBool>,
    pub enforce_ulimit_nofile: bool,
    pub fixed_leader_schedule: Option<FixedSchedule>,
    pub wait_for_supermajority: Option<Slot>,
    pub new_hard_forks: Option<Vec<Slot>>,
    pub known_validators: Option<HashSet<Pubkey>>, // None = trust all
    pub repair_validators: Option<HashSet<Pubkey>>, // None = repair from all
    pub repair_whitelist: Arc<RwLock<HashSet<Pubkey>>>, // Empty = repair with all
    pub gossip_validators: Option<HashSet<Pubkey>>, // None = gossip with all
    pub accounts_hash_fault_injector: Option<AccountsHashFaultInjector>,
    pub accounts_hash_interval_slots: u64,
    pub max_genesis_archive_unpacked_size: u64,
    pub wal_recovery_mode: Option<BlockstoreRecoveryMode>,
    /// Run PoH, transaction signature and other transaction verifications during blockstore
    /// processing.
    pub run_verification: bool,
    pub require_tower: bool,
    pub tower_storage: Arc<dyn TowerStorage>,
    pub debug_keys: Option<Arc<HashSet<Pubkey>>>,
    pub contact_debug_interval: u64,
    pub contact_save_interval: u64,
    pub send_transaction_service_config: send_transaction_service::Config,
    pub no_poh_speed_test: bool,
    pub no_os_memory_stats_reporting: bool,
    pub no_os_network_stats_reporting: bool,
    pub no_os_cpu_stats_reporting: bool,
    pub no_os_disk_stats_reporting: bool,
    pub poh_pinned_cpu_core: usize,
    pub poh_hashes_per_batch: u64,
    pub process_ledger_before_services: bool,
    pub account_indexes: AccountSecondaryIndexes,
    pub accounts_db_config: Option<AccountsDbConfig>,
    pub warp_slot: Option<Slot>,
    pub accounts_db_test_hash_calculation: bool,
    pub accounts_db_skip_shrink: bool,
    pub accounts_db_force_initial_clean: bool,
    pub tpu_coalesce: Duration,
    pub staked_nodes_overrides: Arc<RwLock<HashMap<Pubkey, u64>>>,
    pub validator_exit: Arc<RwLock<Exit>>,
    pub no_wait_for_vote_to_start_leader: bool,
    pub accounts_shrink_ratio: AccountShrinkThreshold,
    pub wait_to_vote_slot: Option<Slot>,
    pub ledger_column_options: LedgerColumnOptions,
    pub runtime_config: RuntimeConfig,
    pub replay_slots_concurrently: bool,
    pub banking_trace_dir_byte_limit: banking_trace::DirByteLimit,
    pub block_verification_method: BlockVerificationMethod,
    pub block_production_method: BlockProductionMethod,
    pub generator_config: Option<GeneratorConfig>,
    pub use_snapshot_archives_at_startup: UseSnapshotArchivesAtStartup,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            halt_at_slot: None,
            expected_genesis_hash: None,
            expected_bank_hash: None,
            expected_shred_version: None,
            voting_disabled: false,
            max_ledger_shreds: None,
            account_paths: Vec::new(),
            account_snapshot_paths: Vec::new(),
            account_shrink_paths: None,
            rpc_config: JsonRpcConfig::default(),
            on_start_geyser_plugin_config_files: None,
            rpc_addrs: None,
            pubsub_config: PubSubConfig::default(),
            snapshot_config: SnapshotConfig::new_load_only(),
            broadcast_stage_type: BroadcastStageType::Standard,
            turbine_disabled: Arc::<AtomicBool>::default(),
            enforce_ulimit_nofile: true,
            fixed_leader_schedule: None,
            wait_for_supermajority: None,
            new_hard_forks: None,
            known_validators: None,
            repair_validators: None,
            repair_whitelist: Arc::new(RwLock::new(HashSet::default())),
            gossip_validators: None,
            accounts_hash_fault_injector: None,
            accounts_hash_interval_slots: std::u64::MAX,
            max_genesis_archive_unpacked_size: MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
            wal_recovery_mode: None,
            run_verification: true,
            require_tower: false,
            tower_storage: Arc::new(NullTowerStorage::default()),
            debug_keys: None,
            contact_debug_interval: DEFAULT_CONTACT_DEBUG_INTERVAL_MILLIS,
            contact_save_interval: DEFAULT_CONTACT_SAVE_INTERVAL_MILLIS,
            send_transaction_service_config: send_transaction_service::Config::default(),
            no_poh_speed_test: true,
            no_os_memory_stats_reporting: true,
            no_os_network_stats_reporting: true,
            no_os_cpu_stats_reporting: true,
            no_os_disk_stats_reporting: true,
            poh_pinned_cpu_core: poh_service::DEFAULT_PINNED_CPU_CORE,
            poh_hashes_per_batch: poh_service::DEFAULT_HASHES_PER_BATCH,
            process_ledger_before_services: false,
            account_indexes: AccountSecondaryIndexes::default(),
            warp_slot: None,
            accounts_db_test_hash_calculation: false,
            accounts_db_skip_shrink: false,
            accounts_db_force_initial_clean: false,
            tpu_coalesce: DEFAULT_TPU_COALESCE,
            staked_nodes_overrides: Arc::new(RwLock::new(HashMap::new())),
            validator_exit: Arc::new(RwLock::new(Exit::default())),
            no_wait_for_vote_to_start_leader: true,
            accounts_shrink_ratio: AccountShrinkThreshold::default(),
            accounts_db_config: None,
            wait_to_vote_slot: None,
            ledger_column_options: LedgerColumnOptions::default(),
            runtime_config: RuntimeConfig::default(),
            replay_slots_concurrently: false,
            banking_trace_dir_byte_limit: 0,
            block_verification_method: BlockVerificationMethod::default(),
            block_production_method: BlockProductionMethod::default(),
            generator_config: None,
            use_snapshot_archives_at_startup: UseSnapshotArchivesAtStartup::default(),
        }
    }
}

impl ValidatorConfig {
    pub fn default_for_test() -> Self {
        Self {
            enforce_ulimit_nofile: false,
            rpc_config: JsonRpcConfig::default_for_test(),
            ..Self::default()
        }
    }

    pub fn enable_default_rpc_block_subscribe(&mut self) {
        let pubsub_config = PubSubConfig {
            enable_block_subscription: true,
            ..PubSubConfig::default()
        };
        let rpc_config = JsonRpcConfig {
            enable_rpc_transaction_history: true,
            ..JsonRpcConfig::default_for_test()
        };

        self.pubsub_config = pubsub_config;
        self.rpc_config = rpc_config;
    }
}

// `ValidatorStartProgress` contains status information that is surfaced to the node operator over
// the admin RPC channel to help them to follow the general progress of node startup without
// having to watch log messages.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ValidatorStartProgress {
    Initializing, // Catch all, default state
    SearchingForRpcService,
    DownloadingSnapshot {
        slot: Slot,
        rpc_addr: SocketAddr,
    },
    CleaningBlockStore,
    CleaningAccounts,
    LoadingLedger,
    ProcessingLedger {
        slot: Slot,
        max_slot: Slot,
    },
    StartingServices,
    Halted, // Validator halted due to `--dev-halt-at-slot` argument
    WaitingForSupermajority {
        slot: Slot,
        gossip_stake_percent: u64,
    },

    // `Running` is the terminal state once the validator fully starts and all services are
    // operational
    Running,
}

impl Default for ValidatorStartProgress {
    fn default() -> Self {
        Self::Initializing
    }
}

struct BlockstoreRootScan {
    thread: Option<JoinHandle<Result<usize, BlockstoreError>>>,
}

impl BlockstoreRootScan {
    fn new(config: &ValidatorConfig, blockstore: Arc<Blockstore>, exit: Arc<AtomicBool>) -> Self {
        let thread = if config.rpc_addrs.is_some()
            && config.rpc_config.enable_rpc_transaction_history
            && config.rpc_config.rpc_scan_and_fix_roots
        {
            Some(
                Builder::new()
                    .name("solBStoreRtScan".to_string())
                    .spawn(move || blockstore.scan_and_fix_roots(None, None, &exit))
                    .unwrap(),
            )
        } else {
            None
        };
        Self { thread }
    }

    fn join(self) {
        if let Some(blockstore_root_scan) = self.thread {
            if let Err(err) = blockstore_root_scan.join() {
                warn!("blockstore_root_scan failed to join {:?}", err);
            }
        }
    }
}

#[derive(Default)]
struct TransactionHistoryServices {
    transaction_status_sender: Option<TransactionStatusSender>,
    transaction_status_service: Option<TransactionStatusService>,
    max_complete_transaction_status_slot: Arc<AtomicU64>,
    rewards_recorder_sender: Option<RewardsRecorderSender>,
    rewards_recorder_service: Option<RewardsRecorderService>,
    max_complete_rewards_slot: Arc<AtomicU64>,
    cache_block_meta_sender: Option<CacheBlockMetaSender>,
    cache_block_meta_service: Option<CacheBlockMetaService>,
}

pub struct Validator {
    validator_exit: Arc<RwLock<Exit>>,
    json_rpc_service: Option<JsonRpcService>,
    pubsub_service: Option<PubSubService>,
    rpc_completed_slots_service: JoinHandle<()>,
    optimistically_confirmed_bank_tracker: Option<OptimisticallyConfirmedBankTracker>,
    transaction_status_service: Option<TransactionStatusService>,
    rewards_recorder_service: Option<RewardsRecorderService>,
    cache_block_meta_service: Option<CacheBlockMetaService>,
    entry_notifier_service: Option<EntryNotifierService>,
    system_monitor_service: Option<SystemMonitorService>,
    sample_performance_service: Option<SamplePerformanceService>,
    poh_timing_report_service: PohTimingReportService,
    stats_reporter_service: StatsReporterService,
    gossip_service: GossipService,
    serve_repair_service: ServeRepairService,
    completed_data_sets_service: CompletedDataSetsService,
    snapshot_packager_service: Option<SnapshotPackagerService>,
    poh_recorder: Arc<RwLock<PohRecorder>>,
    poh_service: PohService,
    tpu: Tpu,
    tvu: Tvu,
    ip_echo_server: Option<solana_net_utils::IpEchoServer>,
    pub cluster_info: Arc<ClusterInfo>,
    pub bank_forks: Arc<RwLock<BankForks>>,
    pub blockstore: Arc<Blockstore>,
    geyser_plugin_service: Option<GeyserPluginService>,
    ledger_metric_report_service: LedgerMetricReportService,
    accounts_background_service: AccountsBackgroundService,
    accounts_hash_verifier: AccountsHashVerifier,
    turbine_quic_endpoint: Option<Endpoint>,
    turbine_quic_endpoint_runtime: Option<TokioRuntime>,
    turbine_quic_endpoint_join_handle: Option<solana_turbine::quic_endpoint::AsyncTryJoinHandle>,
    repair_quic_endpoint: Option<Endpoint>,
    repair_quic_endpoint_runtime: Option<TokioRuntime>,
    repair_quic_endpoint_join_handle: Option<repair::quic_endpoint::AsyncTryJoinHandle>,
}

impl Validator {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mut node: Node,
        identity_keypair: Arc<Keypair>,
        ledger_path: &Path,
        vote_account: &Pubkey,
        authorized_voter_keypairs: Arc<RwLock<Vec<Arc<Keypair>>>>,
        cluster_entrypoints: Vec<ContactInfo>,
        config: &ValidatorConfig,
        should_check_duplicate_instance: bool,
        rpc_to_plugin_manager_receiver: Option<Receiver<GeyserPluginManagerRequest>>,
        start_progress: Arc<RwLock<ValidatorStartProgress>>,
        socket_addr_space: SocketAddrSpace,
        use_quic: bool,
        tpu_connection_pool_size: usize,
        tpu_enable_udp: bool,
        admin_rpc_service_post_init: Arc<RwLock<Option<AdminRpcRequestMetadataPostInit>>>,
    ) -> Result<Self, String> {
        let id = identity_keypair.pubkey();
        assert_eq!(&id, node.info.pubkey());

        info!("identity pubkey: {id}");
        info!("vote account pubkey: {vote_account}");

        if !config.no_os_network_stats_reporting {
            verify_net_stats_access()
                .map_err(|err| format!("Failed to access network stats: {err:?}"))?;
        }

        let mut bank_notification_senders = Vec::new();

        let exit = Arc::new(AtomicBool::new(false));

        let geyser_plugin_service =
            if let Some(geyser_plugin_config_files) = &config.on_start_geyser_plugin_config_files {
                let (confirmed_bank_sender, confirmed_bank_receiver) = unbounded();
                bank_notification_senders.push(confirmed_bank_sender);
                let rpc_to_plugin_manager_receiver_and_exit =
                    rpc_to_plugin_manager_receiver.map(|receiver| (receiver, exit.clone()));
                Some(
                    GeyserPluginService::new_with_receiver(
                        confirmed_bank_receiver,
                        geyser_plugin_config_files,
                        rpc_to_plugin_manager_receiver_and_exit,
                    )
                    .map_err(|err| format!("Failed to load the Geyser plugin: {err:?}"))?,
                )
            } else {
                None
            };

        if config.voting_disabled {
            warn!("voting disabled");
            authorized_voter_keypairs.write().unwrap().clear();
        } else {
            for authorized_voter_keypair in authorized_voter_keypairs.read().unwrap().iter() {
                warn!("authorized voter: {}", authorized_voter_keypair.pubkey());
            }
        }

        for cluster_entrypoint in &cluster_entrypoints {
            info!("entrypoint: {:?}", cluster_entrypoint);
        }

        if rayon::ThreadPoolBuilder::new()
            .thread_name(|i| format!("solRayonGlob{i:02}"))
            .build_global()
            .is_err()
        {
            warn!("Rayon global thread pool already initialized");
        }

        if solana_perf::perf_libs::api().is_some() {
            info!("Initializing sigverify, this could take a while...");
        } else {
            info!("Initializing sigverify...");
        }
        sigverify::init();
        info!("Initializing sigverify done.");

        if !ledger_path.is_dir() {
            return Err(format!(
                "ledger directory does not exist or is not accessible: {ledger_path:?}"
            ));
        }

        if let Some(expected_shred_version) = config.expected_shred_version {
            if let Some(wait_for_supermajority_slot) = config.wait_for_supermajority {
                *start_progress.write().unwrap() = ValidatorStartProgress::CleaningBlockStore;
                backup_and_clear_blockstore(
                    ledger_path,
                    config,
                    wait_for_supermajority_slot + 1,
                    expected_shred_version,
                )
                .map_err(|err| {
                    format!(
                        "Failed to backup and clear shreds with incorrect \
                        shred version from blockstore: {err:?}"
                    )
                })?;
            }
        }

        info!("Cleaning accounts paths..");
        *start_progress.write().unwrap() = ValidatorStartProgress::CleaningAccounts;
        let mut timer = Measure::start("clean_accounts_paths");
        cleanup_accounts_paths(config);
        timer.stop();
        info!("Cleaning accounts paths done. {timer}");

        snapshot_utils::purge_incomplete_bank_snapshots(&config.snapshot_config.bank_snapshots_dir);
        snapshot_utils::purge_old_bank_snapshots_at_startup(
            &config.snapshot_config.bank_snapshots_dir,
        );

        info!("Cleaning orphaned account snapshot directories..");
        let mut timer = Measure::start("clean_orphaned_account_snapshot_dirs");
        clean_orphaned_account_snapshot_dirs(
            &config.snapshot_config.bank_snapshots_dir,
            &config.account_snapshot_paths,
        )
        .map_err(|err| format!("Failed to clean orphaned account snapshot directories: {err:?}"))?;
        timer.stop();
        info!("Cleaning orphaned account snapshot directories done. {timer}");

        // The accounts hash cache dir was renamed, so cleanup any old dirs that exist.
        let accounts_hash_cache_path = config
            .accounts_db_config
            .as_ref()
            .and_then(|config| config.accounts_hash_cache_path.as_ref())
            .map(PathBuf::as_path)
            .unwrap_or(ledger_path);
        let old_accounts_hash_cache_dirs = [
            ledger_path.join("calculate_accounts_hash_cache"),
            accounts_hash_cache_path.join("full"),
            accounts_hash_cache_path.join("incremental"),
            accounts_hash_cache_path.join("transient"),
        ];
        for old_accounts_hash_cache_dir in old_accounts_hash_cache_dirs {
            if old_accounts_hash_cache_dir.exists() {
                snapshot_utils::move_and_async_delete_path(old_accounts_hash_cache_dir);
            }
        }

        {
            let exit = exit.clone();
            config
                .validator_exit
                .write()
                .unwrap()
                .register_exit(Box::new(move || exit.store(true, Ordering::Relaxed)));
        }

        let accounts_update_notifier = geyser_plugin_service
            .as_ref()
            .and_then(|geyser_plugin_service| geyser_plugin_service.get_accounts_update_notifier());

        let transaction_notifier = geyser_plugin_service
            .as_ref()
            .and_then(|geyser_plugin_service| geyser_plugin_service.get_transaction_notifier());

        let entry_notifier = geyser_plugin_service
            .as_ref()
            .and_then(|geyser_plugin_service| geyser_plugin_service.get_entry_notifier());

        let block_metadata_notifier = geyser_plugin_service
            .as_ref()
            .and_then(|geyser_plugin_service| geyser_plugin_service.get_block_metadata_notifier());

        info!(
            "Geyser plugin: accounts_update_notifier: {}, \
            transaction_notifier: {}, \
            entry_notifier: {}",
            accounts_update_notifier.is_some(),
            transaction_notifier.is_some(),
            entry_notifier.is_some()
        );

        let system_monitor_service = Some(SystemMonitorService::new(
            exit.clone(),
            SystemMonitorStatsReportConfig {
                report_os_memory_stats: !config.no_os_memory_stats_reporting,
                report_os_network_stats: !config.no_os_network_stats_reporting,
                report_os_cpu_stats: !config.no_os_cpu_stats_reporting,
                report_os_disk_stats: !config.no_os_disk_stats_reporting,
            },
        ));

        let (poh_timing_point_sender, poh_timing_point_receiver) = unbounded();
        let poh_timing_report_service =
            PohTimingReportService::new(poh_timing_point_receiver, exit.clone());

        let (
            genesis_config,
            bank_forks,
            blockstore,
            original_blockstore_root,
            ledger_signal_receiver,
            completed_slots_receiver,
            leader_schedule_cache,
            starting_snapshot_hashes,
            TransactionHistoryServices {
                transaction_status_sender,
                transaction_status_service,
                max_complete_transaction_status_slot,
                rewards_recorder_sender,
                rewards_recorder_service,
                max_complete_rewards_slot,
                cache_block_meta_sender,
                cache_block_meta_service,
            },
            blockstore_process_options,
            blockstore_root_scan,
            pruned_banks_receiver,
            entry_notifier_service,
        ) = load_blockstore(
            config,
            ledger_path,
            exit.clone(),
            &start_progress,
            accounts_update_notifier,
            transaction_notifier,
            entry_notifier,
            Some(poh_timing_point_sender.clone()),
        )?;
        let hard_forks = bank_forks.read().unwrap().root_bank().hard_forks();
        if !hard_forks.is_empty() {
            info!("Hard forks: {:?}", hard_forks);
        }

        node.info.set_wallclock(timestamp());
        node.info.set_shred_version(compute_shred_version(
            &genesis_config.hash(),
            Some(&hard_forks),
        ));

        Self::print_node_info(&node);

        if let Some(expected_shred_version) = config.expected_shred_version {
            if expected_shred_version != node.info.shred_version() {
                return Err(format!(
                    "shred version mismatch: expected {} found: {}",
                    expected_shred_version,
                    node.info.shred_version(),
                ));
            }
        }

        let mut cluster_info = ClusterInfo::new(
            node.info.clone(),
            identity_keypair.clone(),
            socket_addr_space,
        );
        cluster_info.set_contact_debug_interval(config.contact_debug_interval);
        cluster_info.set_entrypoints(cluster_entrypoints);
        cluster_info.restore_contact_info(ledger_path, config.contact_save_interval);
        let cluster_info = Arc::new(cluster_info);

        assert!(is_snapshot_config_valid(
            &config.snapshot_config,
            config.accounts_hash_interval_slots,
        ));

        let (snapshot_package_sender, snapshot_packager_service) =
            if config.snapshot_config.should_generate_snapshots() {
                // filler accounts make snapshots invalid for use
                // so, do not publish that we have snapshots
                let enable_gossip_push = config
                    .accounts_db_config
                    .as_ref()
                    .map(|config| config.filler_accounts_config.count == 0)
                    .unwrap_or(true);
                let (snapshot_package_sender, snapshot_package_receiver) =
                    crossbeam_channel::unbounded();
                let snapshot_packager_service = SnapshotPackagerService::new(
                    snapshot_package_sender.clone(),
                    snapshot_package_receiver,
                    starting_snapshot_hashes,
                    exit.clone(),
                    cluster_info.clone(),
                    config.snapshot_config.clone(),
                    enable_gossip_push,
                );
                (
                    Some(snapshot_package_sender),
                    Some(snapshot_packager_service),
                )
            } else {
                (None, None)
            };

        let (accounts_package_sender, accounts_package_receiver) = crossbeam_channel::unbounded();
        let accounts_hash_verifier = AccountsHashVerifier::new(
            accounts_package_sender.clone(),
            accounts_package_receiver,
            snapshot_package_sender,
            exit.clone(),
            cluster_info.clone(),
            config.accounts_hash_fault_injector,
            config.snapshot_config.clone(),
        );

        let (snapshot_request_sender, snapshot_request_receiver) = unbounded();
        let accounts_background_request_sender =
            AbsRequestSender::new(snapshot_request_sender.clone());
        let snapshot_request_handler = SnapshotRequestHandler {
            snapshot_config: config.snapshot_config.clone(),
            snapshot_request_sender,
            snapshot_request_receiver,
            accounts_package_sender,
        };
        let pruned_banks_request_handler = PrunedBanksRequestHandler {
            pruned_banks_receiver,
        };
        let last_full_snapshot_slot = starting_snapshot_hashes.map(|x| x.full.0 .0);
        let accounts_background_service = AccountsBackgroundService::new(
            bank_forks.clone(),
            exit.clone(),
            AbsRequestHandlers {
                snapshot_request_handler,
                pruned_banks_request_handler,
            },
            config.accounts_db_test_hash_calculation,
            last_full_snapshot_slot,
        );
        info!(
            "Using: block-verification-method: {}, block-production-method: {}",
            config.block_verification_method, config.block_production_method
        );

        let leader_schedule_cache = Arc::new(leader_schedule_cache);
        let entry_notification_sender = entry_notifier_service
            .as_ref()
            .map(|service| service.sender());
        let mut process_blockstore = ProcessBlockStore::new(
            &id,
            vote_account,
            &start_progress,
            &blockstore,
            original_blockstore_root,
            &bank_forks,
            &leader_schedule_cache,
            &blockstore_process_options,
            transaction_status_sender.as_ref(),
            cache_block_meta_sender.clone(),
            entry_notification_sender,
            blockstore_root_scan,
            accounts_background_request_sender.clone(),
            config,
        );

        maybe_warp_slot(
            config,
            &mut process_blockstore,
            ledger_path,
            &bank_forks,
            &leader_schedule_cache,
            &accounts_background_request_sender,
        )?;

        if config.process_ledger_before_services {
            process_blockstore.process()?;
        }
        *start_progress.write().unwrap() = ValidatorStartProgress::StartingServices;

        let sample_performance_service =
            if config.rpc_addrs.is_some() && config.rpc_config.enable_rpc_transaction_history {
                Some(SamplePerformanceService::new(
                    &bank_forks,
                    blockstore.clone(),
                    exit.clone(),
                ))
            } else {
                None
            };

        let mut block_commitment_cache = BlockCommitmentCache::default();
        let bank_forks_guard = bank_forks.read().unwrap();
        block_commitment_cache.initialize_slots(
            bank_forks_guard.working_bank().slot(),
            bank_forks_guard.root(),
        );
        drop(bank_forks_guard);
        let block_commitment_cache = Arc::new(RwLock::new(block_commitment_cache));

        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);

        let rpc_subscriptions = Arc::new(RpcSubscriptions::new_with_config(
            exit.clone(),
            max_complete_transaction_status_slot.clone(),
            max_complete_rewards_slot.clone(),
            blockstore.clone(),
            bank_forks.clone(),
            block_commitment_cache.clone(),
            optimistically_confirmed_bank.clone(),
            &config.pubsub_config,
            None,
        ));

        let max_slots = Arc::new(MaxSlots::default());
        let (completed_data_sets_sender, completed_data_sets_receiver) =
            bounded(MAX_COMPLETED_DATA_SETS_IN_CHANNEL);
        let completed_data_sets_service = CompletedDataSetsService::new(
            completed_data_sets_receiver,
            blockstore.clone(),
            rpc_subscriptions.clone(),
            exit.clone(),
            max_slots.clone(),
        );

        let startup_verification_complete;
        let (poh_recorder, entry_receiver, record_receiver) = {
            let bank = &bank_forks.read().unwrap().working_bank();
            startup_verification_complete = Arc::clone(bank.get_startup_verification_complete());
            PohRecorder::new_with_clear_signal(
                bank.tick_height(),
                bank.last_blockhash(),
                bank.clone(),
                None,
                bank.ticks_per_slot(),
                &id,
                blockstore.clone(),
                blockstore.get_new_shred_signal(0),
                &leader_schedule_cache,
                &genesis_config.poh_config,
                Some(poh_timing_point_sender),
                exit.clone(),
            )
        };
        let poh_recorder = Arc::new(RwLock::new(poh_recorder));

        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));

        let connection_cache = match use_quic {
            true => {
                let connection_cache = ConnectionCache::new_with_client_options(
                    "connection_cache_tpu_quic",
                    tpu_connection_pool_size,
                    None,
                    Some((
                        &identity_keypair,
                        node.info
                            .tpu(Protocol::UDP)
                            .expect("Operator must spin up node with valid TPU address")
                            .ip(),
                    )),
                    Some((&staked_nodes, &identity_keypair.pubkey())),
                );
                Arc::new(connection_cache)
            }
            false => Arc::new(ConnectionCache::with_udp(
                "connection_cache_tpu_udp",
                tpu_connection_pool_size,
            )),
        };

        // block min prioritization fee cache should be readable by RPC, and writable by validator
        // (by both replay stage and banking stage)
        let prioritization_fee_cache = Arc::new(PrioritizationFeeCache::default());

        let rpc_override_health_check =
            Arc::new(AtomicBool::new(config.rpc_config.disable_health_check));
        let (
            json_rpc_service,
            pubsub_service,
            optimistically_confirmed_bank_tracker,
            bank_notification_sender,
        ) = if let Some((rpc_addr, rpc_pubsub_addr)) = config.rpc_addrs {
            assert_eq!(
                node.info
                    .rpc()
                    .map(|addr| socket_addr_space.check(&addr))
                    .ok(),
                node.info
                    .rpc_pubsub()
                    .map(|addr| socket_addr_space.check(&addr))
                    .ok()
            );
            let (bank_notification_sender, bank_notification_receiver) = unbounded();
            let confirmed_bank_subscribers = if !bank_notification_senders.is_empty() {
                Some(Arc::new(RwLock::new(bank_notification_senders)))
            } else {
                None
            };

            let json_rpc_service = JsonRpcService::new(
                rpc_addr,
                config.rpc_config.clone(),
                Some(config.snapshot_config.clone()),
                bank_forks.clone(),
                block_commitment_cache.clone(),
                blockstore.clone(),
                cluster_info.clone(),
                Some(poh_recorder.clone()),
                genesis_config.hash(),
                ledger_path,
                config.validator_exit.clone(),
                exit.clone(),
                rpc_override_health_check.clone(),
                startup_verification_complete,
                optimistically_confirmed_bank.clone(),
                config.send_transaction_service_config.clone(),
                max_slots.clone(),
                leader_schedule_cache.clone(),
                connection_cache.clone(),
                max_complete_transaction_status_slot,
                max_complete_rewards_slot,
                prioritization_fee_cache.clone(),
            )?;

            (
                Some(json_rpc_service),
                if !config.rpc_config.full_api {
                    None
                } else {
                    let (trigger, pubsub_service) = PubSubService::new(
                        config.pubsub_config.clone(),
                        &rpc_subscriptions,
                        rpc_pubsub_addr,
                    );
                    config
                        .validator_exit
                        .write()
                        .unwrap()
                        .register_exit(Box::new(move || trigger.cancel()));

                    Some(pubsub_service)
                },
                Some(OptimisticallyConfirmedBankTracker::new(
                    bank_notification_receiver,
                    exit.clone(),
                    bank_forks.clone(),
                    optimistically_confirmed_bank,
                    rpc_subscriptions.clone(),
                    confirmed_bank_subscribers,
                    prioritization_fee_cache.clone(),
                )),
                Some(BankNotificationSenderConfig {
                    sender: bank_notification_sender,
                    should_send_parents: geyser_plugin_service.is_some(),
                }),
            )
        } else {
            (None, None, None, None)
        };

        if config.halt_at_slot.is_some() {
            // Simulate a confirmed root to avoid RPC errors with CommitmentConfig::finalized() and
            // to ensure RPC endpoints like getConfirmedBlock, which require a confirmed root, work
            block_commitment_cache
                .write()
                .unwrap()
                .set_highest_super_majority_root(bank_forks.read().unwrap().root());

            // Park with the RPC service running, ready for inspection!
            warn!("Validator halted");
            *start_progress.write().unwrap() = ValidatorStartProgress::Halted;
            std::thread::park();
        }
        let ip_echo_server = match node.sockets.ip_echo {
            None => None,
            Some(tcp_listener) => Some(solana_net_utils::ip_echo_server(
                tcp_listener,
                Some(node.info.shred_version()),
            )),
        };

        let (stats_reporter_sender, stats_reporter_receiver) = unbounded();

        let stats_reporter_service =
            StatsReporterService::new(stats_reporter_receiver, exit.clone());

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(bank_forks.clone()),
            node.sockets.gossip,
            config.gossip_validators.clone(),
            should_check_duplicate_instance,
            Some(stats_reporter_sender.clone()),
            exit.clone(),
        );
        let serve_repair = ServeRepair::new(
            cluster_info.clone(),
            bank_forks.clone(),
            config.repair_whitelist.clone(),
        );
        let (repair_quic_endpoint_sender, repair_quic_endpoint_receiver) = unbounded();
        let serve_repair_service = ServeRepairService::new(
            serve_repair,
            // Incoming UDP repair requests are adapted into RemoteRequest
            // and also sent through the same channel.
            repair_quic_endpoint_sender.clone(),
            repair_quic_endpoint_receiver,
            blockstore.clone(),
            node.sockets.serve_repair,
            socket_addr_space,
            stats_reporter_sender,
            exit.clone(),
        );

        *admin_rpc_service_post_init.write().unwrap() = Some(AdminRpcRequestMetadataPostInit {
            bank_forks: bank_forks.clone(),
            cluster_info: cluster_info.clone(),
            vote_account: *vote_account,
            repair_whitelist: config.repair_whitelist.clone(),
        });

        let waited_for_supermajority = wait_for_supermajority(
            config,
            Some(&mut process_blockstore),
            &bank_forks,
            &cluster_info,
            rpc_override_health_check,
            &start_progress,
        )
        .map_err(|err| format!("wait_for_supermajority failed: {err:?}"))?;

        let ledger_metric_report_service =
            LedgerMetricReportService::new(blockstore.clone(), exit.clone());

        let wait_for_vote_to_start_leader =
            !waited_for_supermajority && !config.no_wait_for_vote_to_start_leader;

        let poh_service = PohService::new(
            poh_recorder.clone(),
            &genesis_config.poh_config,
            exit.clone(),
            bank_forks.read().unwrap().root_bank().ticks_per_slot(),
            config.poh_pinned_cpu_core,
            config.poh_hashes_per_batch,
            record_receiver,
        );
        assert_eq!(
            blockstore.get_new_shred_signals_len(),
            1,
            "New shred signal for the TVU should be the same as the clear bank signal."
        );

        let vote_tracker = Arc::<VoteTracker>::default();

        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let (verified_vote_sender, verified_vote_receiver) = unbounded();
        let (gossip_verified_vote_hash_sender, gossip_verified_vote_hash_receiver) = unbounded();
        let (cluster_confirmed_slot_sender, cluster_confirmed_slot_receiver) = unbounded();

        let rpc_completed_slots_service = RpcCompletedSlotsService::spawn(
            completed_slots_receiver,
            rpc_subscriptions.clone(),
            exit.clone(),
        );

        let (banking_tracer, tracer_thread) =
            BankingTracer::new((config.banking_trace_dir_byte_limit > 0).then_some((
                &blockstore.banking_trace_path(),
                exit.clone(),
                config.banking_trace_dir_byte_limit,
            )))
            .map_err(|err| format!("{} [{:?}]", &err, &err))?;
        if banking_tracer.is_enabled() {
            info!(
                "Enabled banking trace (dir_byte_limit: {})",
                config.banking_trace_dir_byte_limit
            );
        } else {
            info!("Disabled banking trace");
        }

        let entry_notification_sender = entry_notifier_service
            .as_ref()
            .map(|service| service.sender_cloned());

        // test-validator crate may start the validator in a tokio runtime
        // context which forces us to use the same runtime because a nested
        // runtime will cause panic at drop.
        // Outside test-validator crate, we always need a tokio runtime (and
        // the respective handle) to initialize the turbine QUIC endpoint.
        let current_runtime_handle = tokio::runtime::Handle::try_current();
        let turbine_quic_endpoint_runtime = (current_runtime_handle.is_err()
            && genesis_config.cluster_type != ClusterType::MainnetBeta)
            .then(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("solTurbineQuic")
                    .build()
                    .unwrap()
            });
        let (turbine_quic_endpoint_sender, turbine_quic_endpoint_receiver) = unbounded();
        let (
            turbine_quic_endpoint,
            turbine_quic_endpoint_sender,
            turbine_quic_endpoint_join_handle,
        ) = if genesis_config.cluster_type == ClusterType::MainnetBeta {
            let (sender, _receiver) = tokio::sync::mpsc::channel(1);
            (None, sender, None)
        } else {
            solana_turbine::quic_endpoint::new_quic_endpoint(
                turbine_quic_endpoint_runtime
                    .as_ref()
                    .map(TokioRuntime::handle)
                    .unwrap_or_else(|| current_runtime_handle.as_ref().unwrap()),
                &identity_keypair,
                node.sockets.tvu_quic,
                node.info
                    .tvu(Protocol::QUIC)
                    .expect("Operator must spin up node with valid QUIC TVU address")
                    .ip(),
                turbine_quic_endpoint_sender,
                bank_forks.clone(),
            )
            .map(|(endpoint, sender, join_handle)| (Some(endpoint), sender, Some(join_handle)))
            .unwrap()
        };

        // Repair quic endpoint.
        let repair_quic_endpoint_runtime = (current_runtime_handle.is_err()
            && genesis_config.cluster_type != ClusterType::MainnetBeta)
            .then(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("solRepairQuic")
                    .build()
                    .unwrap()
            });
        let (repair_quic_endpoint, repair_quic_endpoint_sender, repair_quic_endpoint_join_handle) =
            if genesis_config.cluster_type == ClusterType::MainnetBeta {
                let (sender, _receiver) = tokio::sync::mpsc::channel(1);
                (None, sender, None)
            } else {
                repair::quic_endpoint::new_quic_endpoint(
                    repair_quic_endpoint_runtime
                        .as_ref()
                        .map(TokioRuntime::handle)
                        .unwrap_or_else(|| current_runtime_handle.as_ref().unwrap()),
                    &identity_keypair,
                    node.sockets.serve_repair_quic,
                    node.info
                        .serve_repair(Protocol::QUIC)
                        .expect("Operator must spin up node with valid QUIC serve-repair address")
                        .ip(),
                    repair_quic_endpoint_sender,
                    bank_forks.clone(),
                )
                .map(|(endpoint, sender, join_handle)| (Some(endpoint), sender, Some(join_handle)))
                .unwrap()
            };

        let (replay_vote_sender, replay_vote_receiver) = unbounded();
        let tvu = Tvu::new(
            vote_account,
            authorized_voter_keypairs,
            &bank_forks,
            &cluster_info,
            TvuSockets {
                repair: node.sockets.repair,
                retransmit: node.sockets.retransmit_sockets,
                fetch: node.sockets.tvu,
                ancestor_hashes_requests: node.sockets.ancestor_hashes_requests,
            },
            blockstore.clone(),
            ledger_signal_receiver,
            &rpc_subscriptions,
            &poh_recorder,
            Some(process_blockstore),
            config.tower_storage.clone(),
            &leader_schedule_cache,
            exit.clone(),
            block_commitment_cache,
            config.turbine_disabled.clone(),
            transaction_status_sender.clone(),
            rewards_recorder_sender,
            cache_block_meta_sender,
            entry_notification_sender.clone(),
            vote_tracker.clone(),
            retransmit_slots_sender,
            gossip_verified_vote_hash_receiver,
            verified_vote_receiver,
            replay_vote_sender.clone(),
            completed_data_sets_sender,
            bank_notification_sender.clone(),
            cluster_confirmed_slot_receiver,
            TvuConfig {
                max_ledger_shreds: config.max_ledger_shreds,
                shred_version: node.info.shred_version(),
                repair_validators: config.repair_validators.clone(),
                repair_whitelist: config.repair_whitelist.clone(),
                wait_for_vote_to_start_leader,
                replay_slots_concurrently: config.replay_slots_concurrently,
            },
            &max_slots,
            block_metadata_notifier,
            config.wait_to_vote_slot,
            accounts_background_request_sender,
            config.runtime_config.log_messages_bytes_limit,
            &connection_cache,
            &prioritization_fee_cache,
            banking_tracer.clone(),
            turbine_quic_endpoint_sender.clone(),
            turbine_quic_endpoint_receiver,
            repair_quic_endpoint_sender,
        )?;

        let tpu = Tpu::new(
            &cluster_info,
            &poh_recorder,
            entry_receiver,
            retransmit_slots_receiver,
            TpuSockets {
                transactions: node.sockets.tpu,
                transaction_forwards: node.sockets.tpu_forwards,
                vote: node.sockets.tpu_vote,
                broadcast: node.sockets.broadcast,
                transactions_quic: node.sockets.tpu_quic,
                transactions_forwards_quic: node.sockets.tpu_forwards_quic,
            },
            &rpc_subscriptions,
            transaction_status_sender,
            entry_notification_sender,
            blockstore.clone(),
            &config.broadcast_stage_type,
            exit,
            node.info.shred_version(),
            vote_tracker,
            bank_forks.clone(),
            verified_vote_sender,
            gossip_verified_vote_hash_sender,
            replay_vote_receiver,
            replay_vote_sender,
            bank_notification_sender.map(|sender| sender.sender),
            config.tpu_coalesce,
            cluster_confirmed_slot_sender,
            &connection_cache,
            turbine_quic_endpoint_sender,
            &identity_keypair,
            config.runtime_config.log_messages_bytes_limit,
            &staked_nodes,
            config.staked_nodes_overrides.clone(),
            banking_tracer,
            tracer_thread,
            tpu_enable_udp,
            &prioritization_fee_cache,
            config.block_production_method.clone(),
            config.generator_config.clone(),
        );

        let cluster_type = bank_forks.read().unwrap().root_bank().cluster_type();
        metrics_config_sanity_check(cluster_type)?;

        datapoint_info!(
            "validator-new",
            ("id", id.to_string(), String),
            ("version", solana_version::version!(), String),
            ("cluster_type", cluster_type as u32, i64),
        );

        *start_progress.write().unwrap() = ValidatorStartProgress::Running;
        Ok(Self {
            stats_reporter_service,
            gossip_service,
            serve_repair_service,
            json_rpc_service,
            pubsub_service,
            rpc_completed_slots_service,
            optimistically_confirmed_bank_tracker,
            transaction_status_service,
            rewards_recorder_service,
            cache_block_meta_service,
            entry_notifier_service,
            system_monitor_service,
            sample_performance_service,
            poh_timing_report_service,
            snapshot_packager_service,
            completed_data_sets_service,
            tpu,
            tvu,
            poh_service,
            poh_recorder,
            ip_echo_server,
            validator_exit: config.validator_exit.clone(),
            cluster_info,
            bank_forks,
            blockstore,
            geyser_plugin_service,
            ledger_metric_report_service,
            accounts_background_service,
            accounts_hash_verifier,
            turbine_quic_endpoint,
            turbine_quic_endpoint_runtime,
            turbine_quic_endpoint_join_handle,
            repair_quic_endpoint,
            repair_quic_endpoint_runtime,
            repair_quic_endpoint_join_handle,
        })
    }

    // Used for notifying many nodes in parallel to exit
    pub fn exit(&mut self) {
        self.validator_exit.write().unwrap().exit();

        // drop all signals in blockstore
        self.blockstore.drop_signal();
    }

    pub fn close(mut self) {
        self.exit();
        self.join();
    }

    fn print_node_info(node: &Node) {
        info!("{:?}", node.info);
        info!(
            "local gossip address: {}",
            node.sockets.gossip.local_addr().unwrap()
        );
        info!(
            "local broadcast address: {}",
            node.sockets
                .broadcast
                .first()
                .unwrap()
                .local_addr()
                .unwrap()
        );
        info!(
            "local repair address: {}",
            node.sockets.repair.local_addr().unwrap()
        );
        info!(
            "local retransmit address: {}",
            node.sockets.retransmit_sockets[0].local_addr().unwrap()
        );
    }

    pub fn join(self) {
        drop(self.bank_forks);
        drop(self.cluster_info);

        self.poh_service.join().expect("poh_service");
        drop(self.poh_recorder);

        if let Some(json_rpc_service) = self.json_rpc_service {
            json_rpc_service.join().expect("rpc_service");
        }

        if let Some(pubsub_service) = self.pubsub_service {
            pubsub_service.join().expect("pubsub_service");
        }

        self.rpc_completed_slots_service
            .join()
            .expect("rpc_completed_slots_service");

        if let Some(optimistically_confirmed_bank_tracker) =
            self.optimistically_confirmed_bank_tracker
        {
            optimistically_confirmed_bank_tracker
                .join()
                .expect("optimistically_confirmed_bank_tracker");
        }

        if let Some(transaction_status_service) = self.transaction_status_service {
            transaction_status_service
                .join()
                .expect("transaction_status_service");
        }

        if let Some(rewards_recorder_service) = self.rewards_recorder_service {
            rewards_recorder_service
                .join()
                .expect("rewards_recorder_service");
        }

        if let Some(cache_block_meta_service) = self.cache_block_meta_service {
            cache_block_meta_service
                .join()
                .expect("cache_block_meta_service");
        }

        if let Some(system_monitor_service) = self.system_monitor_service {
            system_monitor_service
                .join()
                .expect("system_monitor_service");
        }

        if let Some(sample_performance_service) = self.sample_performance_service {
            sample_performance_service
                .join()
                .expect("sample_performance_service");
        }

        if let Some(entry_notifier_service) = self.entry_notifier_service {
            entry_notifier_service
                .join()
                .expect("entry_notifier_service");
        }

        if let Some(s) = self.snapshot_packager_service {
            s.join().expect("snapshot_packager_service");
        }

        self.gossip_service.join().expect("gossip_service");
        if let Some(repair_quic_endpoint) = &self.repair_quic_endpoint {
            repair::quic_endpoint::close_quic_endpoint(repair_quic_endpoint);
        }
        self.serve_repair_service
            .join()
            .expect("serve_repair_service");
        if let Some(repair_quic_endpoint_join_handle) = self.repair_quic_endpoint_join_handle {
            self.repair_quic_endpoint_runtime
                .map(|runtime| runtime.block_on(repair_quic_endpoint_join_handle))
                .transpose()
                .unwrap();
        };
        self.stats_reporter_service
            .join()
            .expect("stats_reporter_service");
        self.ledger_metric_report_service
            .join()
            .expect("ledger_metric_report_service");
        self.accounts_background_service
            .join()
            .expect("accounts_background_service");
        self.accounts_hash_verifier
            .join()
            .expect("accounts_hash_verifier");
        if let Some(turbine_quic_endpoint) = &self.turbine_quic_endpoint {
            solana_turbine::quic_endpoint::close_quic_endpoint(turbine_quic_endpoint);
        }
        self.tpu.join().expect("tpu");
        self.tvu.join().expect("tvu");
        if let Some(turbine_quic_endpoint_join_handle) = self.turbine_quic_endpoint_join_handle {
            self.turbine_quic_endpoint_runtime
                .map(|runtime| runtime.block_on(turbine_quic_endpoint_join_handle))
                .transpose()
                .unwrap();
        }
        self.completed_data_sets_service
            .join()
            .expect("completed_data_sets_service");
        if let Some(ip_echo_server) = self.ip_echo_server {
            ip_echo_server.shutdown_background();
        }

        if let Some(geyser_plugin_service) = self.geyser_plugin_service {
            geyser_plugin_service.join().expect("geyser_plugin_service");
        }

        self.poh_timing_report_service
            .join()
            .expect("poh_timing_report_service");
    }
}

fn active_vote_account_exists_in_bank(bank: &Bank, vote_account: &Pubkey) -> bool {
    if let Some(account) = &bank.get_account(vote_account) {
        if let Some(vote_state) = vote_state::from(account) {
            return !vote_state.votes.is_empty();
        }
    }
    false
}

fn check_poh_speed(
    genesis_config: &GenesisConfig,
    maybe_hash_samples: Option<u64>,
) -> Result<(), String> {
    if let Some(hashes_per_tick) = genesis_config.hashes_per_tick() {
        let ticks_per_slot = genesis_config.ticks_per_slot();
        let hashes_per_slot = hashes_per_tick * ticks_per_slot;

        let hash_samples = maybe_hash_samples.unwrap_or(hashes_per_slot);
        let hash_time_ns = compute_hash_time_ns(hash_samples);

        let my_ns_per_slot = (hash_time_ns * hashes_per_slot) / hash_samples;
        debug!("computed: ns_per_slot: {}", my_ns_per_slot);
        let target_ns_per_slot = genesis_config.ns_per_slot() as u64;
        debug!(
            "cluster ns_per_hash: {}ns ns_per_slot: {}",
            target_ns_per_slot / hashes_per_slot,
            target_ns_per_slot
        );
        if my_ns_per_slot < target_ns_per_slot {
            let extra_ns = target_ns_per_slot - my_ns_per_slot;
            info!("PoH speed check: Will sleep {}ns per slot.", extra_ns);
        } else {
            return Err(format!(
                "PoH is slower than cluster target tick rate! mine: {my_ns_per_slot} cluster: {target_ns_per_slot}.",
            ));
        }
    }
    Ok(())
}

fn maybe_cluster_restart_with_hard_fork(config: &ValidatorConfig, root_slot: Slot) -> Option<Slot> {
    // detect cluster restart (hard fork) indirectly via wait_for_supermajority...
    if let Some(wait_slot_for_supermajority) = config.wait_for_supermajority {
        if wait_slot_for_supermajority == root_slot {
            return Some(wait_slot_for_supermajority);
        }
    }

    None
}

fn post_process_restored_tower(
    restored_tower: crate::consensus::Result<Tower>,
    validator_identity: &Pubkey,
    vote_account: &Pubkey,
    config: &ValidatorConfig,
    bank_forks: &BankForks,
) -> Result<Tower, String> {
    let mut should_require_tower = config.require_tower;

    let restored_tower = restored_tower.and_then(|tower| {
        let root_bank = bank_forks.root_bank();
        let slot_history = root_bank.get_slot_history();
        // make sure tower isn't corrupted first before the following hard fork check
        let tower = tower.adjust_lockouts_after_replay(root_bank.slot(), &slot_history);

        if let Some(hard_fork_restart_slot) =
            maybe_cluster_restart_with_hard_fork(config, root_bank.slot())
        {
            // intentionally fail to restore tower; we're supposedly in a new hard fork; past
            // out-of-chain vote state doesn't make sense at all
            // what if --wait-for-supermajority again if the validator restarted?
            let message =
                format!("Hard fork is detected; discarding tower restoration result: {tower:?}");
            datapoint_error!("tower_error", ("error", message, String),);
            error!("{}", message);

            // unconditionally relax tower requirement so that we can always restore tower
            // from root bank.
            should_require_tower = false;
            return Err(crate::consensus::TowerError::HardFork(
                hard_fork_restart_slot,
            ));
        }

        if let Some(warp_slot) = config.warp_slot {
            // unconditionally relax tower requirement so that we can always restore tower
            // from root bank after the warp
            should_require_tower = false;
            return Err(crate::consensus::TowerError::HardFork(warp_slot));
        }

        tower
    });

    let restored_tower = match restored_tower {
        Ok(tower) => tower,
        Err(err) => {
            let voting_has_been_active =
                active_vote_account_exists_in_bank(&bank_forks.working_bank(), vote_account);
            if !err.is_file_missing() {
                datapoint_error!(
                    "tower_error",
                    ("error", format!("Unable to restore tower: {err}"), String),
                );
            }
            if should_require_tower && voting_has_been_active {
                return Err(format!(
                    "Requested mandatory tower restore failed: {err}. \
                     And there is an existing vote_account containing actual votes. \
                     Aborting due to possible conflicting duplicate votes"
                ));
            }
            if err.is_file_missing() && !voting_has_been_active {
                // Currently, don't protect against spoofed snapshots with no tower at all
                info!(
                    "Ignoring expected failed tower restore because this is the initial \
                      validator start with the vote account..."
                );
            } else {
                error!(
                    "Rebuilding a new tower from the latest vote account due to failed tower restore: {}",
                    err
                );
            }

            Tower::new_from_bankforks(bank_forks, validator_identity, vote_account)
        }
    };

    Ok(restored_tower)
}

fn blockstore_options_from_config(config: &ValidatorConfig) -> BlockstoreOptions {
    BlockstoreOptions {
        recovery_mode: config.wal_recovery_mode.clone(),
        column_options: config.ledger_column_options.clone(),
        enforce_ulimit_nofile: config.enforce_ulimit_nofile,
        ..BlockstoreOptions::default()
    }
}

#[allow(clippy::type_complexity)]
fn load_blockstore(
    config: &ValidatorConfig,
    ledger_path: &Path,
    exit: Arc<AtomicBool>,
    start_progress: &Arc<RwLock<ValidatorStartProgress>>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    transaction_notifier: Option<TransactionNotifierLock>,
    entry_notifier: Option<EntryNotifierLock>,
    poh_timing_point_sender: Option<PohTimingSender>,
) -> Result<
    (
        GenesisConfig,
        Arc<RwLock<BankForks>>,
        Arc<Blockstore>,
        Slot,
        Receiver<bool>,
        CompletedSlotsReceiver,
        LeaderScheduleCache,
        Option<StartingSnapshotHashes>,
        TransactionHistoryServices,
        blockstore_processor::ProcessOptions,
        BlockstoreRootScan,
        DroppedSlotsReceiver,
        Option<EntryNotifierService>,
    ),
    String,
> {
    info!("loading ledger from {:?}...", ledger_path);
    *start_progress.write().unwrap() = ValidatorStartProgress::LoadingLedger;
    let genesis_config = open_genesis_config(ledger_path, config.max_genesis_archive_unpacked_size);

    // This needs to be limited otherwise the state in the VoteAccount data
    // grows too large
    let leader_schedule_slot_offset = genesis_config.epoch_schedule.leader_schedule_slot_offset;
    let slots_per_epoch = genesis_config.epoch_schedule.slots_per_epoch;
    let leader_epoch_offset = (leader_schedule_slot_offset + slots_per_epoch - 1) / slots_per_epoch;
    assert!(leader_epoch_offset <= MAX_LEADER_SCHEDULE_EPOCH_OFFSET);

    let genesis_hash = genesis_config.hash();
    info!("genesis hash: {}", genesis_hash);

    if let Some(expected_genesis_hash) = config.expected_genesis_hash {
        if genesis_hash != expected_genesis_hash {
            return Err(format!(
                "genesis hash mismatch: hash={genesis_hash} expected={expected_genesis_hash}. Delete the ledger directory to continue: {ledger_path:?}",
            ));
        }
    }

    if !config.no_poh_speed_test {
        check_poh_speed(&genesis_config, None)?;
    }

    let BlockstoreSignals {
        mut blockstore,
        ledger_signal_receiver,
        completed_slots_receiver,
        ..
    } = Blockstore::open_with_signal(ledger_path, blockstore_options_from_config(config))
        .expect("Failed to open ledger database");
    blockstore.shred_timing_point_sender = poh_timing_point_sender;
    // following boot sequence (esp BankForks) could set root. so stash the original value
    // of blockstore root away here as soon as possible.
    let original_blockstore_root = blockstore.last_root();

    let blockstore = Arc::new(blockstore);
    let blockstore_root_scan = BlockstoreRootScan::new(config, blockstore.clone(), exit.clone());
    let halt_at_slot = config
        .halt_at_slot
        .or_else(|| blockstore.highest_slot().unwrap_or(None));

    let process_options = blockstore_processor::ProcessOptions {
        run_verification: config.run_verification,
        halt_at_slot,
        new_hard_forks: config.new_hard_forks.clone(),
        debug_keys: config.debug_keys.clone(),
        account_indexes: config.account_indexes.clone(),
        accounts_db_config: config.accounts_db_config.clone(),
        shrink_ratio: config.accounts_shrink_ratio,
        accounts_db_test_hash_calculation: config.accounts_db_test_hash_calculation,
        accounts_db_skip_shrink: config.accounts_db_skip_shrink,
        accounts_db_force_initial_clean: config.accounts_db_force_initial_clean,
        runtime_config: config.runtime_config.clone(),
        use_snapshot_archives_at_startup: config.use_snapshot_archives_at_startup,
        ..blockstore_processor::ProcessOptions::default()
    };

    let enable_rpc_transaction_history =
        config.rpc_addrs.is_some() && config.rpc_config.enable_rpc_transaction_history;
    let is_plugin_transaction_history_required = transaction_notifier.as_ref().is_some();
    let transaction_history_services =
        if enable_rpc_transaction_history || is_plugin_transaction_history_required {
            initialize_rpc_transaction_history_services(
                blockstore.clone(),
                exit.clone(),
                enable_rpc_transaction_history,
                config.rpc_config.enable_extended_tx_metadata_storage,
                transaction_notifier,
            )
        } else {
            TransactionHistoryServices::default()
        };

    let entry_notifier_service = entry_notifier
        .map(|entry_notifier| EntryNotifierService::new(entry_notifier, exit.clone()));

    let (bank_forks, mut leader_schedule_cache, starting_snapshot_hashes) =
        bank_forks_utils::load_bank_forks(
            &genesis_config,
            &blockstore,
            config.account_paths.clone(),
            config.account_shrink_paths.clone(),
            Some(&config.snapshot_config),
            &process_options,
            transaction_history_services
                .cache_block_meta_sender
                .as_ref(),
            entry_notifier_service
                .as_ref()
                .map(|service| service.sender()),
            accounts_update_notifier,
            exit,
        );

    // Before replay starts, set the callbacks in each of the banks in BankForks so that
    // all dropped banks come through the `pruned_banks_receiver` channel. This way all bank
    // drop behavior can be safely synchronized with any other ongoing accounts activity like
    // cache flush, clean, shrink, as long as the same thread performing those activities also
    // is processing the dropped banks from the `pruned_banks_receiver` channel.
    let pruned_banks_receiver =
        AccountsBackgroundService::setup_bank_drop_callback(bank_forks.clone());

    leader_schedule_cache.set_fixed_leader_schedule(config.fixed_leader_schedule.clone());
    {
        let mut bank_forks = bank_forks.write().unwrap();
        bank_forks.set_snapshot_config(Some(config.snapshot_config.clone()));
        bank_forks.set_accounts_hash_interval_slots(config.accounts_hash_interval_slots);
        if let Some(ref shrink_paths) = config.account_shrink_paths {
            bank_forks
                .working_bank()
                .set_shrink_paths(shrink_paths.clone());
        }
    }

    Ok((
        genesis_config,
        bank_forks,
        blockstore,
        original_blockstore_root,
        ledger_signal_receiver,
        completed_slots_receiver,
        leader_schedule_cache,
        starting_snapshot_hashes,
        transaction_history_services,
        process_options,
        blockstore_root_scan,
        pruned_banks_receiver,
        entry_notifier_service,
    ))
}

pub struct ProcessBlockStore<'a> {
    id: &'a Pubkey,
    vote_account: &'a Pubkey,
    start_progress: &'a Arc<RwLock<ValidatorStartProgress>>,
    blockstore: &'a Blockstore,
    original_blockstore_root: Slot,
    bank_forks: &'a Arc<RwLock<BankForks>>,
    leader_schedule_cache: &'a LeaderScheduleCache,
    process_options: &'a blockstore_processor::ProcessOptions,
    transaction_status_sender: Option<&'a TransactionStatusSender>,
    cache_block_meta_sender: Option<CacheBlockMetaSender>,
    entry_notification_sender: Option<&'a EntryNotifierSender>,
    blockstore_root_scan: Option<BlockstoreRootScan>,
    accounts_background_request_sender: AbsRequestSender,
    config: &'a ValidatorConfig,
    tower: Option<Tower>,
}

impl<'a> ProcessBlockStore<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: &'a Pubkey,
        vote_account: &'a Pubkey,
        start_progress: &'a Arc<RwLock<ValidatorStartProgress>>,
        blockstore: &'a Blockstore,
        original_blockstore_root: Slot,
        bank_forks: &'a Arc<RwLock<BankForks>>,
        leader_schedule_cache: &'a LeaderScheduleCache,
        process_options: &'a blockstore_processor::ProcessOptions,
        transaction_status_sender: Option<&'a TransactionStatusSender>,
        cache_block_meta_sender: Option<CacheBlockMetaSender>,
        entry_notification_sender: Option<&'a EntryNotifierSender>,
        blockstore_root_scan: BlockstoreRootScan,
        accounts_background_request_sender: AbsRequestSender,
        config: &'a ValidatorConfig,
    ) -> Self {
        Self {
            id,
            vote_account,
            start_progress,
            blockstore,
            original_blockstore_root,
            bank_forks,
            leader_schedule_cache,
            process_options,
            transaction_status_sender,
            cache_block_meta_sender,
            entry_notification_sender,
            blockstore_root_scan: Some(blockstore_root_scan),
            accounts_background_request_sender,
            config,
            tower: None,
        }
    }

    pub(crate) fn process(&mut self) -> Result<(), String> {
        if self.tower.is_none() {
            let previous_start_process = *self.start_progress.read().unwrap();
            *self.start_progress.write().unwrap() = ValidatorStartProgress::LoadingLedger;

            let exit = Arc::new(AtomicBool::new(false));
            if let Ok(Some(max_slot)) = self.blockstore.highest_slot() {
                let bank_forks = self.bank_forks.clone();
                let exit = exit.clone();
                let start_progress = self.start_progress.clone();

                let _ = Builder::new()
                    .name("solRptLdgrStat".to_string())
                    .spawn(move || {
                        while !exit.load(Ordering::Relaxed) {
                            let slot = bank_forks.read().unwrap().working_bank().slot();
                            *start_progress.write().unwrap() =
                                ValidatorStartProgress::ProcessingLedger { slot, max_slot };
                            sleep(Duration::from_secs(2));
                        }
                    })
                    .unwrap();
            }
            blockstore_processor::process_blockstore_from_root(
                self.blockstore,
                self.bank_forks,
                self.leader_schedule_cache,
                self.process_options,
                self.transaction_status_sender,
                self.cache_block_meta_sender.as_ref(),
                self.entry_notification_sender,
                &self.accounts_background_request_sender,
            )
            .map_err(|err| {
                exit.store(true, Ordering::Relaxed);
                format!("Failed to load ledger: {err:?}")
            })?;
            exit.store(true, Ordering::Relaxed);

            if let Some(blockstore_root_scan) = self.blockstore_root_scan.take() {
                blockstore_root_scan.join();
            }

            self.tower = Some({
                let restored_tower = Tower::restore(self.config.tower_storage.as_ref(), self.id);
                if let Ok(tower) = &restored_tower {
                    // reconciliation attempt 1 of 2 with tower
                    reconcile_blockstore_roots_with_external_source(
                        ExternalRootSource::Tower(tower.root()),
                        self.blockstore,
                        &mut self.original_blockstore_root,
                    )
                    .map_err(|err| format!("Failed to reconcile blockstore with tower: {err:?}"))?;
                }

                post_process_restored_tower(
                    restored_tower,
                    self.id,
                    self.vote_account,
                    self.config,
                    &self.bank_forks.read().unwrap(),
                )?
            });

            if let Some(hard_fork_restart_slot) = maybe_cluster_restart_with_hard_fork(
                self.config,
                self.bank_forks.read().unwrap().root(),
            ) {
                // reconciliation attempt 2 of 2 with hard fork
                // this should be #2 because hard fork root > tower root in almost all cases
                reconcile_blockstore_roots_with_external_source(
                    ExternalRootSource::HardFork(hard_fork_restart_slot),
                    self.blockstore,
                    &mut self.original_blockstore_root,
                )
                .map_err(|err| format!("Failed to reconcile blockstore with hard fork: {err:?}"))?;
            }

            *self.start_progress.write().unwrap() = previous_start_process;
        }
        Ok(())
    }

    pub(crate) fn process_to_create_tower(mut self) -> Result<Tower, String> {
        self.process()?;
        Ok(self.tower.unwrap())
    }
}

fn maybe_warp_slot(
    config: &ValidatorConfig,
    process_blockstore: &mut ProcessBlockStore,
    ledger_path: &Path,
    bank_forks: &RwLock<BankForks>,
    leader_schedule_cache: &LeaderScheduleCache,
    accounts_background_request_sender: &AbsRequestSender,
) -> Result<(), String> {
    if let Some(warp_slot) = config.warp_slot {
        let mut bank_forks = bank_forks.write().unwrap();

        let working_bank = bank_forks.working_bank();

        if warp_slot <= working_bank.slot() {
            return Err(format!(
                "warp slot ({}) cannot be less than the working bank slot ({})",
                warp_slot,
                working_bank.slot()
            ));
        }
        info!("warping to slot {}", warp_slot);

        let root_bank = bank_forks.root_bank();

        // An accounts hash calculation from storages will occur in warp_from_parent() below.  This
        // requires that the accounts cache has been flushed, which requires the parent slot to be
        // rooted.
        root_bank.squash();
        root_bank.force_flush_accounts_cache();

        bank_forks.insert(Bank::warp_from_parent(
            root_bank,
            &Pubkey::default(),
            warp_slot,
            solana_accounts_db::accounts_db::CalcAccountsHashDataSource::Storages,
        ));
        bank_forks.set_root(
            warp_slot,
            accounts_background_request_sender,
            Some(warp_slot),
        );
        leader_schedule_cache.set_root(&bank_forks.root_bank());

        let full_snapshot_archive_info = match snapshot_bank_utils::bank_to_full_snapshot_archive(
            ledger_path,
            &bank_forks.root_bank(),
            None,
            &config.snapshot_config.full_snapshot_archives_dir,
            &config.snapshot_config.incremental_snapshot_archives_dir,
            config.snapshot_config.archive_format,
            config
                .snapshot_config
                .maximum_full_snapshot_archives_to_retain,
            config
                .snapshot_config
                .maximum_incremental_snapshot_archives_to_retain,
        ) {
            Ok(archive_info) => archive_info,
            Err(e) => return Err(format!("Unable to create snapshot: {e}")),
        };
        info!(
            "created snapshot: {}",
            full_snapshot_archive_info.path().display()
        );

        drop(bank_forks);
        // Process blockstore after warping bank forks to make sure tower and
        // bank forks are in sync.
        process_blockstore.process()?;
    }
    Ok(())
}

/// Searches the blockstore for data shreds with the incorrect shred version.
fn blockstore_contains_bad_shred_version(
    blockstore: &Blockstore,
    start_slot: Slot,
    expected_shred_version: u16,
) -> Result<bool, BlockstoreError> {
    const TIMEOUT: Duration = Duration::from_secs(60);
    let timer = Instant::now();
    // Search for shreds with incompatible version in blockstore
    let slot_meta_iterator = blockstore.slot_meta_iterator(start_slot)?;

    info!("Searching blockstore for shred with incorrect version..");
    for (slot, _meta) in slot_meta_iterator {
        let shreds = blockstore.get_data_shreds_for_slot(slot, 0)?;
        for shred in &shreds {
            if shred.version() != expected_shred_version {
                return Ok(true);
            }
        }
        if timer.elapsed() > TIMEOUT {
            info!("Didn't find incorrect shreds after 60 seconds, aborting");
            break;
        }
    }
    Ok(false)
}

/// If the blockstore contains any shreds with the incorrect shred version,
/// copy them to a backup blockstore and purge them from the actual blockstore.
fn backup_and_clear_blockstore(
    ledger_path: &Path,
    config: &ValidatorConfig,
    start_slot: Slot,
    expected_shred_version: u16,
) -> Result<(), BlockstoreError> {
    let blockstore =
        Blockstore::open_with_options(ledger_path, blockstore_options_from_config(config))?;
    let do_copy_and_clear =
        blockstore_contains_bad_shred_version(&blockstore, start_slot, expected_shred_version)?;

    if do_copy_and_clear {
        // .unwrap() safe because getting to this point implies blockstore has slots/shreds
        let end_slot = blockstore.highest_slot()?.unwrap();

        // Backing up the shreds that will be deleted from primary blockstore is
        // not critical, so swallow errors from backup blockstore operations.
        let backup_folder = format!(
            "{}_backup_{}_{}_{}",
            config
                .ledger_column_options
                .shred_storage_type
                .blockstore_directory(),
            expected_shred_version,
            start_slot,
            end_slot
        );
        match Blockstore::open_with_options(
            &ledger_path.join(backup_folder),
            blockstore_options_from_config(config),
        ) {
            Ok(backup_blockstore) => {
                info!("Backing up slots from {start_slot} to {end_slot}");
                let mut timer = Measure::start("blockstore backup");

                const PRINT_INTERVAL: Duration = Duration::from_secs(5);
                let mut print_timer = Instant::now();
                let mut num_slots_copied = 0;
                let slot_meta_iterator = blockstore.slot_meta_iterator(start_slot)?;
                for (slot, _meta) in slot_meta_iterator {
                    let shreds = blockstore.get_data_shreds_for_slot(slot, 0)?;
                    let _ = backup_blockstore.insert_shreds(shreds, None, true);
                    num_slots_copied += 1;

                    if print_timer.elapsed() > PRINT_INTERVAL {
                        info!("Backed up {num_slots_copied} slots thus far");
                        print_timer = Instant::now();
                    }
                }

                timer.stop();
                info!("Backing up slots done. {timer}");
            }
            Err(err) => {
                warn!("Unable to backup shreds with incorrect shred version: {err}");
            }
        }

        info!("Purging slots {start_slot} to {end_slot} from blockstore");
        let mut timer = Measure::start("blockstore purge");
        blockstore.purge_from_next_slots(start_slot, end_slot);
        blockstore.purge_slots(start_slot, end_slot, PurgeType::Exact);
        timer.stop();
        info!("Purging slots done. {timer}");
    } else {
        info!("Only shreds with the correct version were found in the blockstore");
    }

    Ok(())
}

fn initialize_rpc_transaction_history_services(
    blockstore: Arc<Blockstore>,
    exit: Arc<AtomicBool>,
    enable_rpc_transaction_history: bool,
    enable_extended_tx_metadata_storage: bool,
    transaction_notifier: Option<TransactionNotifierLock>,
) -> TransactionHistoryServices {
    let max_complete_transaction_status_slot = Arc::new(AtomicU64::new(blockstore.max_root()));
    let (transaction_status_sender, transaction_status_receiver) = unbounded();
    let transaction_status_sender = Some(TransactionStatusSender {
        sender: transaction_status_sender,
    });
    let transaction_status_service = Some(TransactionStatusService::new(
        transaction_status_receiver,
        max_complete_transaction_status_slot.clone(),
        enable_rpc_transaction_history,
        transaction_notifier,
        blockstore.clone(),
        enable_extended_tx_metadata_storage,
        exit.clone(),
    ));

    let max_complete_rewards_slot = Arc::new(AtomicU64::new(blockstore.max_root()));
    let (rewards_recorder_sender, rewards_receiver) = unbounded();
    let rewards_recorder_sender = Some(rewards_recorder_sender);
    let rewards_recorder_service = Some(RewardsRecorderService::new(
        rewards_receiver,
        max_complete_rewards_slot.clone(),
        blockstore.clone(),
        exit.clone(),
    ));

    let (cache_block_meta_sender, cache_block_meta_receiver) = unbounded();
    let cache_block_meta_sender = Some(cache_block_meta_sender);
    let cache_block_meta_service = Some(CacheBlockMetaService::new(
        cache_block_meta_receiver,
        blockstore,
        exit,
    ));
    TransactionHistoryServices {
        transaction_status_sender,
        transaction_status_service,
        max_complete_transaction_status_slot,
        rewards_recorder_sender,
        rewards_recorder_service,
        max_complete_rewards_slot,
        cache_block_meta_sender,
        cache_block_meta_service,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ValidatorError {
    BadExpectedBankHash,
    NotEnoughLedgerData,
    Error(String),
}

// Return if the validator waited on other nodes to start. In this case
// it should not wait for one of it's votes to land to produce blocks
// because if the whole network is waiting, then it will stall.
//
// Error indicates that a bad hash was encountered or another condition
// that is unrecoverable and the validator should exit.
fn wait_for_supermajority(
    config: &ValidatorConfig,
    process_blockstore: Option<&mut ProcessBlockStore>,
    bank_forks: &RwLock<BankForks>,
    cluster_info: &ClusterInfo,
    rpc_override_health_check: Arc<AtomicBool>,
    start_progress: &Arc<RwLock<ValidatorStartProgress>>,
) -> Result<bool, ValidatorError> {
    match config.wait_for_supermajority {
        None => Ok(false),
        Some(wait_for_supermajority_slot) => {
            if let Some(process_blockstore) = process_blockstore {
                process_blockstore
                    .process()
                    .map_err(ValidatorError::Error)?;
            }

            let bank = bank_forks.read().unwrap().working_bank();
            match wait_for_supermajority_slot.cmp(&bank.slot()) {
                std::cmp::Ordering::Less => return Ok(false),
                std::cmp::Ordering::Greater => {
                    error!(
                        "Ledger does not have enough data to wait for supermajority, \
                             please enable snapshot fetch. Has {} needs {}",
                        bank.slot(),
                        wait_for_supermajority_slot
                    );
                    return Err(ValidatorError::NotEnoughLedgerData);
                }
                _ => {}
            }

            if let Some(expected_bank_hash) = config.expected_bank_hash {
                if bank.hash() != expected_bank_hash {
                    error!(
                        "Bank hash({}) does not match expected value: {}",
                        bank.hash(),
                        expected_bank_hash
                    );
                    return Err(ValidatorError::BadExpectedBankHash);
                }
            }

            for i in 1.. {
                let logging = i % 10 == 1;
                if logging {
                    info!(
                        "Waiting for {}% of activated stake at slot {} to be in gossip...",
                        WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT,
                        bank.slot()
                    );
                }

                let gossip_stake_percent =
                    get_stake_percent_in_gossip(&bank, cluster_info, logging);

                *start_progress.write().unwrap() =
                    ValidatorStartProgress::WaitingForSupermajority {
                        slot: wait_for_supermajority_slot,
                        gossip_stake_percent,
                    };

                if gossip_stake_percent >= WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT {
                    info!(
                        "Supermajority reached, {}% active stake detected, starting up now.",
                        gossip_stake_percent,
                    );
                    break;
                }
                // The normal RPC health checks don't apply as the node is waiting, so feign health to
                // prevent load balancers from removing the node from their list of candidates during a
                // manual restart.
                rpc_override_health_check.store(true, Ordering::Relaxed);
                sleep(Duration::new(1, 0));
            }
            rpc_override_health_check.store(false, Ordering::Relaxed);
            Ok(true)
        }
    }
}

// Get the activated stake percentage (based on the provided bank) that is visible in gossip
fn get_stake_percent_in_gossip(bank: &Bank, cluster_info: &ClusterInfo, log: bool) -> u64 {
    let mut online_stake = 0;
    let mut wrong_shred_stake = 0;
    let mut wrong_shred_nodes = vec![];
    let mut offline_stake = 0;
    let mut offline_nodes = vec![];

    let mut total_activated_stake = 0;
    let now = timestamp();
    // Nodes contact infos are saved to disk and restored on validator startup.
    // Staked nodes entries will not expire until an epoch after. So it
    // is necessary here to filter for recent entries to establish liveness.
    let peers: HashMap<_, _> = cluster_info
        .all_tvu_peers()
        .into_iter()
        .filter(|node| {
            let age = now.saturating_sub(node.wallclock());
            // Contact infos are refreshed twice during this period.
            age < CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS
        })
        .map(|node| (*node.pubkey(), node))
        .collect();
    let my_shred_version = cluster_info.my_shred_version();
    let my_id = cluster_info.id();

    for (activated_stake, vote_account) in bank.vote_accounts().values() {
        let activated_stake = *activated_stake;
        total_activated_stake += activated_stake;

        if activated_stake == 0 {
            continue;
        }
        let vote_state_node_pubkey = vote_account.node_pubkey().unwrap_or_default();

        if let Some(peer) = peers.get(&vote_state_node_pubkey) {
            if peer.shred_version() == my_shred_version {
                trace!(
                    "observed {} in gossip, (activated_stake={})",
                    vote_state_node_pubkey,
                    activated_stake
                );
                online_stake += activated_stake;
            } else {
                wrong_shred_stake += activated_stake;
                wrong_shred_nodes.push((activated_stake, vote_state_node_pubkey));
            }
        } else if vote_state_node_pubkey == my_id {
            online_stake += activated_stake; // This node is online
        } else {
            offline_stake += activated_stake;
            offline_nodes.push((activated_stake, vote_state_node_pubkey));
        }
    }

    let online_stake_percentage = (online_stake as f64 / total_activated_stake as f64) * 100.;
    if log {
        info!(
            "{:.3}% of active stake visible in gossip",
            online_stake_percentage
        );

        if !wrong_shred_nodes.is_empty() {
            info!(
                "{:.3}% of active stake has the wrong shred version in gossip",
                (wrong_shred_stake as f64 / total_activated_stake as f64) * 100.,
            );
            wrong_shred_nodes.sort_by(|b, a| a.0.cmp(&b.0)); // sort by reverse stake weight
            for (stake, identity) in wrong_shred_nodes {
                info!(
                    "    {:.3}% - {}",
                    (stake as f64 / total_activated_stake as f64) * 100.,
                    identity
                );
            }
        }

        if !offline_nodes.is_empty() {
            info!(
                "{:.3}% of active stake is not visible in gossip",
                (offline_stake as f64 / total_activated_stake as f64) * 100.
            );
            offline_nodes.sort_by(|b, a| a.0.cmp(&b.0)); // sort by reverse stake weight
            for (stake, identity) in offline_nodes {
                info!(
                    "    {:.3}% - {}",
                    (stake as f64 / total_activated_stake as f64) * 100.,
                    identity
                );
            }
        }
    }

    online_stake_percentage as u64
}

fn cleanup_accounts_paths(config: &ValidatorConfig) {
    for accounts_path in &config.account_paths {
        move_and_async_delete_path_contents(accounts_path);
    }
    if let Some(ref shrink_paths) = config.account_shrink_paths {
        for accounts_path in shrink_paths {
            move_and_async_delete_path_contents(accounts_path);
        }
    }
}

pub fn is_snapshot_config_valid(
    snapshot_config: &SnapshotConfig,
    accounts_hash_interval_slots: Slot,
) -> bool {
    // if the snapshot config is configured to *not* take snapshots, then it is valid
    if !snapshot_config.should_generate_snapshots() {
        return true;
    }

    let full_snapshot_interval_slots = snapshot_config.full_snapshot_archive_interval_slots;
    let incremental_snapshot_interval_slots =
        snapshot_config.incremental_snapshot_archive_interval_slots;

    let is_incremental_config_valid =
        if incremental_snapshot_interval_slots == DISABLED_SNAPSHOT_ARCHIVE_INTERVAL {
            true
        } else {
            incremental_snapshot_interval_slots >= accounts_hash_interval_slots
                && incremental_snapshot_interval_slots % accounts_hash_interval_slots == 0
                && full_snapshot_interval_slots > incremental_snapshot_interval_slots
        };

    full_snapshot_interval_slots >= accounts_hash_interval_slots
        && full_snapshot_interval_slots % accounts_hash_interval_slots == 0
        && is_incremental_config_valid
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crossbeam_channel::{bounded, RecvTimeoutError},
        solana_entry::entry,
        solana_gossip::contact_info::{ContactInfo, LegacyContactInfo},
        solana_ledger::{
            blockstore, create_new_tmp_ledger, genesis_utils::create_genesis_config_with_leader,
            get_tmp_ledger_path_auto_delete,
        },
        solana_sdk::{genesis_config::create_genesis_config, poh_config::PohConfig},
        solana_tpu_client::tpu_client::{
            DEFAULT_TPU_CONNECTION_POOL_SIZE, DEFAULT_TPU_ENABLE_UDP, DEFAULT_TPU_USE_QUIC,
        },
        std::{fs::remove_dir_all, thread, time::Duration},
    };

    #[test]
    fn validator_exit() {
        solana_logger::setup();
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());

        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());
        let genesis_config =
            create_genesis_config_with_leader(10_000, &leader_keypair.pubkey(), 1000)
                .genesis_config;
        let (validator_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

        let voting_keypair = Arc::new(Keypair::new());
        let config = ValidatorConfig {
            rpc_addrs: Some((
                validator_node.info.rpc().unwrap(),
                validator_node.info.rpc_pubsub().unwrap(),
            )),
            ..ValidatorConfig::default_for_test()
        };
        let start_progress = Arc::new(RwLock::new(ValidatorStartProgress::default()));
        let validator = Validator::new(
            validator_node,
            Arc::new(validator_keypair),
            &validator_ledger_path,
            &voting_keypair.pubkey(),
            Arc::new(RwLock::new(vec![voting_keypair])),
            vec![LegacyContactInfo::try_from(&leader_node.info).unwrap()],
            &config,
            true, // should_check_duplicate_instance
            None, // rpc_to_plugin_manager_receiver
            start_progress.clone(),
            SocketAddrSpace::Unspecified,
            DEFAULT_TPU_USE_QUIC,
            DEFAULT_TPU_CONNECTION_POOL_SIZE,
            DEFAULT_TPU_ENABLE_UDP,
            Arc::new(RwLock::new(None)),
        )
        .expect("assume successful validator start");
        assert_eq!(
            *start_progress.read().unwrap(),
            ValidatorStartProgress::Running
        );
        validator.close();
        remove_dir_all(validator_ledger_path).unwrap();
    }

    #[test]
    fn test_backup_and_clear_blockstore() {
        solana_logger::setup();

        let validator_config = ValidatorConfig::default_for_test();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let entries = entry::create_ticks(1, 0, Hash::default());
        for i in 1..10 {
            let shreds = blockstore::entries_to_test_shreds(
                &entries,
                i,     // slot
                i - 1, // parent_slot
                true,  // is_full_slot
                1,     // version
                true,  // merkle_variant
            );
            blockstore.insert_shreds(shreds, None, true).unwrap();
        }
        drop(blockstore);

        // this purges and compacts all slots greater than or equal to 5
        backup_and_clear_blockstore(ledger_path.path(), &validator_config, 5, 2).unwrap();

        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        // assert that slots less than 5 aren't affected
        assert!(blockstore.meta(4).unwrap().unwrap().next_slots.is_empty());
        for i in 5..10 {
            assert!(blockstore
                .get_data_shreds_for_slot(i, 0)
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn validator_parallel_exit() {
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());
        let genesis_config =
            create_genesis_config_with_leader(10_000, &leader_keypair.pubkey(), 1000)
                .genesis_config;

        let mut ledger_paths = vec![];
        let mut validators: Vec<Validator> = (0..2)
            .map(|_| {
                let validator_keypair = Keypair::new();
                let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());
                let (validator_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);
                ledger_paths.push(validator_ledger_path.clone());
                let vote_account_keypair = Keypair::new();
                let config = ValidatorConfig {
                    rpc_addrs: Some((
                        validator_node.info.rpc().unwrap(),
                        validator_node.info.rpc_pubsub().unwrap(),
                    )),
                    ..ValidatorConfig::default_for_test()
                };
                Validator::new(
                    validator_node,
                    Arc::new(validator_keypair),
                    &validator_ledger_path,
                    &vote_account_keypair.pubkey(),
                    Arc::new(RwLock::new(vec![Arc::new(vote_account_keypair)])),
                    vec![LegacyContactInfo::try_from(&leader_node.info).unwrap()],
                    &config,
                    true, // should_check_duplicate_instance.
                    None, // rpc_to_plugin_manager_receiver
                    Arc::new(RwLock::new(ValidatorStartProgress::default())),
                    SocketAddrSpace::Unspecified,
                    DEFAULT_TPU_USE_QUIC,
                    DEFAULT_TPU_CONNECTION_POOL_SIZE,
                    DEFAULT_TPU_ENABLE_UDP,
                    Arc::new(RwLock::new(None)),
                )
                .expect("assume successful validator start")
            })
            .collect();

        // Each validator can exit in parallel to speed many sequential calls to join`
        validators.iter_mut().for_each(|v| v.exit());

        // spawn a new thread to wait for the join of the validator
        let (sender, receiver) = bounded(0);
        let _ = thread::spawn(move || {
            validators.into_iter().for_each(|validator| {
                validator.join();
            });
            sender.send(()).unwrap();
        });

        let timeout = Duration::from_secs(60);
        if let Err(RecvTimeoutError::Timeout) = receiver.recv_timeout(timeout) {
            panic!("timeout for shutting down validators",);
        }

        for path in ledger_paths {
            remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn test_wait_for_supermajority() {
        solana_logger::setup();
        use solana_sdk::hash::hash;
        let node_keypair = Arc::new(Keypair::new());
        let cluster_info = ClusterInfo::new(
            ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
            node_keypair,
            SocketAddrSpace::Unspecified,
        );

        let (genesis_config, _mint_keypair) = create_genesis_config(1);
        let bank_forks = BankForks::new_rw_arc(Bank::new_for_tests(&genesis_config));
        let mut config = ValidatorConfig::default_for_test();
        let rpc_override_health_check = Arc::new(AtomicBool::new(false));
        let start_progress = Arc::new(RwLock::new(ValidatorStartProgress::default()));

        assert!(!wait_for_supermajority(
            &config,
            None,
            &bank_forks,
            &cluster_info,
            rpc_override_health_check.clone(),
            &start_progress,
        )
        .unwrap());

        // bank=0, wait=1, should fail
        config.wait_for_supermajority = Some(1);
        assert_eq!(
            wait_for_supermajority(
                &config,
                None,
                &bank_forks,
                &cluster_info,
                rpc_override_health_check.clone(),
                &start_progress,
            ),
            Err(ValidatorError::NotEnoughLedgerData)
        );

        // bank=1, wait=0, should pass, bank is past the wait slot
        let bank_forks = BankForks::new_rw_arc(Bank::new_from_parent(
            bank_forks.read().unwrap().root_bank(),
            &Pubkey::default(),
            1,
        ));
        config.wait_for_supermajority = Some(0);
        assert!(!wait_for_supermajority(
            &config,
            None,
            &bank_forks,
            &cluster_info,
            rpc_override_health_check.clone(),
            &start_progress,
        )
        .unwrap());

        // bank=1, wait=1, equal, but bad hash provided
        config.wait_for_supermajority = Some(1);
        config.expected_bank_hash = Some(hash(&[1]));
        assert_eq!(
            wait_for_supermajority(
                &config,
                None,
                &bank_forks,
                &cluster_info,
                rpc_override_health_check,
                &start_progress,
            ),
            Err(ValidatorError::BadExpectedBankHash)
        );
    }

    #[test]
    fn test_interval_check() {
        fn new_snapshot_config(
            full_snapshot_archive_interval_slots: Slot,
            incremental_snapshot_archive_interval_slots: Slot,
        ) -> SnapshotConfig {
            SnapshotConfig {
                full_snapshot_archive_interval_slots,
                incremental_snapshot_archive_interval_slots,
                ..SnapshotConfig::default()
            }
        }

        assert!(is_snapshot_config_valid(
            &new_snapshot_config(300, 200),
            100
        ));

        let default_accounts_hash_interval =
            snapshot_bank_utils::DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS;
        assert!(is_snapshot_config_valid(
            &new_snapshot_config(
                snapshot_bank_utils::DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
                snapshot_bank_utils::DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS
            ),
            default_accounts_hash_interval,
        ));
        assert!(is_snapshot_config_valid(
            &new_snapshot_config(
                snapshot_bank_utils::DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
                DISABLED_SNAPSHOT_ARCHIVE_INTERVAL
            ),
            default_accounts_hash_interval
        ));
        assert!(is_snapshot_config_valid(
            &new_snapshot_config(
                snapshot_bank_utils::DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
                DISABLED_SNAPSHOT_ARCHIVE_INTERVAL
            ),
            default_accounts_hash_interval
        ));
        assert!(is_snapshot_config_valid(
            &new_snapshot_config(
                DISABLED_SNAPSHOT_ARCHIVE_INTERVAL,
                DISABLED_SNAPSHOT_ARCHIVE_INTERVAL
            ),
            Slot::MAX
        ));

        assert!(!is_snapshot_config_valid(&new_snapshot_config(0, 100), 100));
        assert!(!is_snapshot_config_valid(&new_snapshot_config(100, 0), 100));
        assert!(!is_snapshot_config_valid(
            &new_snapshot_config(42, 100),
            100
        ));
        assert!(!is_snapshot_config_valid(
            &new_snapshot_config(100, 42),
            100
        ));
        assert!(!is_snapshot_config_valid(
            &new_snapshot_config(100, 100),
            100
        ));
        assert!(!is_snapshot_config_valid(
            &new_snapshot_config(100, 200),
            100
        ));
        assert!(!is_snapshot_config_valid(
            &new_snapshot_config(444, 200),
            100
        ));
        assert!(!is_snapshot_config_valid(
            &new_snapshot_config(400, 222),
            100
        ));

        assert!(is_snapshot_config_valid(
            &SnapshotConfig::new_load_only(),
            100
        ));
        assert!(is_snapshot_config_valid(
            &SnapshotConfig {
                full_snapshot_archive_interval_slots: 41,
                incremental_snapshot_archive_interval_slots: 37,
                ..SnapshotConfig::new_load_only()
            },
            100
        ));
        assert!(is_snapshot_config_valid(
            &SnapshotConfig {
                full_snapshot_archive_interval_slots: DISABLED_SNAPSHOT_ARCHIVE_INTERVAL,
                incremental_snapshot_archive_interval_slots: DISABLED_SNAPSHOT_ARCHIVE_INTERVAL,
                ..SnapshotConfig::new_load_only()
            },
            100
        ));
    }

    #[test]
    fn test_poh_speed() {
        solana_logger::setup();
        let poh_config = PohConfig {
            target_tick_duration: Duration::from_millis(solana_sdk::clock::MS_PER_TICK),
            // make PoH rate really fast to cause the panic condition
            hashes_per_tick: Some(100 * solana_sdk::clock::DEFAULT_HASHES_PER_TICK),
            ..PohConfig::default()
        };
        let genesis_config = GenesisConfig {
            poh_config,
            ..GenesisConfig::default()
        };
        assert!(check_poh_speed(&genesis_config, Some(10_000)).is_err());
    }

    #[test]
    fn test_poh_speed_no_hashes_per_tick() {
        let poh_config = PohConfig {
            target_tick_duration: Duration::from_millis(solana_sdk::clock::MS_PER_TICK),
            hashes_per_tick: None,
            ..PohConfig::default()
        };
        let genesis_config = GenesisConfig {
            poh_config,
            ..GenesisConfig::default()
        };
        check_poh_speed(&genesis_config, Some(10_000)).unwrap();
    }
}
