use {
    crate::accountsdb_repl_service::AccountsDbReplService,
    crossbeam_channel::unbounded,
    log::*,
    solana_download_utils::download_snapshot_archive,
    solana_genesis_utils::download_then_check_genesis_hash,
    solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo},
    solana_ledger::{
        blockstore::Blockstore, blockstore_db::AccessType, blockstore_processor,
        leader_schedule_cache::LeaderScheduleCache,
    },
    solana_replica_lib::accountsdb_repl_client::AccountsDbReplClientServiceConfig,
    solana_rpc::{
        max_slots::MaxSlots,
        optimistically_confirmed_bank_tracker::{
            OptimisticallyConfirmedBank, OptimisticallyConfirmedBankTracker,
        },
        rpc::JsonRpcConfig,
        rpc_pubsub_service::{PubSubConfig, PubSubService},
        rpc_service::JsonRpcService,
        rpc_subscriptions::RpcSubscriptions,
    },
    solana_runtime::{
        accounts_index::AccountSecondaryIndexes, bank_forks::BankForks,
        commitment::BlockCommitmentCache, hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
        snapshot_config::SnapshotConfig, snapshot_package::SnapshotType, snapshot_utils,
    },
    solana_sdk::{clock::Slot, exit::Exit, genesis_config::GenesisConfig, hash::Hash},
    solana_send_transaction_service::send_transaction_service,
    solana_streamer::socket::SocketAddrSpace,
    std::{
        fs,
        net::SocketAddr,
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicU64},
            Arc, RwLock,
        },
    },
};

pub struct ReplicaNodeConfig {
    pub rpc_peer_addr: SocketAddr,
    pub accountsdb_repl_peer_addr: Option<SocketAddr>,
    pub rpc_addr: SocketAddr,
    pub rpc_pubsub_addr: SocketAddr,
    pub ledger_path: PathBuf,
    pub snapshot_archives_dir: PathBuf,
    pub bank_snapshots_dir: PathBuf,
    pub account_paths: Vec<PathBuf>,
    pub snapshot_info: (Slot, Hash),
    pub cluster_info: Arc<ClusterInfo>,
    pub rpc_config: JsonRpcConfig,
    pub snapshot_config: Option<SnapshotConfig>,
    pub pubsub_config: PubSubConfig,
    pub account_indexes: AccountSecondaryIndexes,
    pub accounts_db_caching_enabled: bool,
    pub replica_exit: Arc<RwLock<Exit>>,
    pub socket_addr_space: SocketAddrSpace,
}

pub struct ReplicaNode {
    json_rpc_service: Option<JsonRpcService>,
    pubsub_service: Option<PubSubService>,
    optimistically_confirmed_bank_tracker: Option<OptimisticallyConfirmedBankTracker>,
    accountsdb_repl_service: Option<AccountsDbReplService>,
}

// Struct maintaining information about banks
struct ReplicaBankInfo {
    bank_forks: Arc<RwLock<BankForks>>,
    optimistically_confirmed_bank: Arc<RwLock<OptimisticallyConfirmedBank>>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
}

// Initialize the replica by downloading snapshot from the peer, initialize
// the BankForks, OptimisticallyConfirmedBank, LeaderScheduleCache and
// BlockCommitmentCache and return the info wrapped as ReplicaBankInfo.
fn initialize_from_snapshot(
    replica_config: &ReplicaNodeConfig,
    snapshot_config: &SnapshotConfig,
    genesis_config: &GenesisConfig,
) -> ReplicaBankInfo {
    info!(
        "Downloading snapshot from the peer into {:?}",
        replica_config.snapshot_archives_dir
    );

    download_snapshot_archive(
        &replica_config.rpc_peer_addr,
        &replica_config.snapshot_archives_dir,
        replica_config.snapshot_info,
        SnapshotType::FullSnapshot,
        snapshot_config.maximum_full_snapshot_archives_to_retain,
        snapshot_config.maximum_incremental_snapshot_archives_to_retain,
        false,
        &mut None,
    )
    .unwrap();

    fs::create_dir_all(&snapshot_config.bank_snapshots_dir)
        .expect("Couldn't create bank snapshot directory");

    let archive_info = snapshot_utils::get_highest_full_snapshot_archive_info(
        &replica_config.snapshot_archives_dir,
    )
    .unwrap();

    let process_options = blockstore_processor::ProcessOptions {
        account_indexes: replica_config.account_indexes.clone(),
        accounts_db_caching_enabled: replica_config.accounts_db_caching_enabled,
        ..blockstore_processor::ProcessOptions::default()
    };

    info!(
        "Build bank from snapshot archive: {:?}",
        &snapshot_config.bank_snapshots_dir
    );
    let (bank0, _) = snapshot_utils::bank_from_snapshot_archives(
        &replica_config.account_paths,
        &snapshot_config.bank_snapshots_dir,
        &archive_info,
        None,
        genesis_config,
        process_options.debug_keys.clone(),
        None,
        process_options.account_indexes.clone(),
        process_options.accounts_db_caching_enabled,
        process_options.limit_load_slot_count_from_snapshot,
        process_options.shrink_ratio,
        process_options.accounts_db_test_hash_calculation,
        false,
        process_options.verify_index,
        process_options.accounts_db_config,
        None,
    )
    .unwrap();

    let bank0_slot = bank0.slot();
    let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank0));

    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank0)));

    let optimistically_confirmed_bank =
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);

    let mut block_commitment_cache = BlockCommitmentCache::default();
    block_commitment_cache.initialize_slots(bank0_slot);
    let block_commitment_cache = Arc::new(RwLock::new(block_commitment_cache));

    ReplicaBankInfo {
        bank_forks,
        optimistically_confirmed_bank,
        leader_schedule_cache,
        block_commitment_cache,
    }
}

fn start_client_rpc_services(
    replica_config: &ReplicaNodeConfig,
    genesis_config: &GenesisConfig,
    cluster_info: Arc<ClusterInfo>,
    bank_info: &ReplicaBankInfo,
    socket_addr_space: &SocketAddrSpace,
) -> (
    Option<JsonRpcService>,
    Option<PubSubService>,
    Option<OptimisticallyConfirmedBankTracker>,
) {
    let ReplicaBankInfo {
        bank_forks,
        optimistically_confirmed_bank,
        leader_schedule_cache,
        block_commitment_cache,
    } = bank_info;
    let blockstore = Arc::new(
        Blockstore::open_with_access_type(
            &replica_config.ledger_path,
            AccessType::PrimaryOnly,
            None,
            false,
        )
        .unwrap(),
    );

    let max_complete_transaction_status_slot = Arc::new(AtomicU64::new(0));

    let max_slots = Arc::new(MaxSlots::default());
    let exit = Arc::new(AtomicBool::new(false));

    let subscriptions = Arc::new(RpcSubscriptions::new(
        &exit,
        bank_forks.clone(),
        block_commitment_cache.clone(),
        optimistically_confirmed_bank.clone(),
    ));

    let rpc_override_health_check = Arc::new(AtomicBool::new(false));
    if ContactInfo::is_valid_address(&replica_config.rpc_addr, socket_addr_space) {
        assert!(ContactInfo::is_valid_address(
            &replica_config.rpc_pubsub_addr,
            socket_addr_space
        ));
    } else {
        assert!(!ContactInfo::is_valid_address(
            &replica_config.rpc_pubsub_addr,
            socket_addr_space
        ));
    }

    let (trigger, pubsub_service) = PubSubService::new(
        replica_config.pubsub_config.clone(),
        &subscriptions,
        replica_config.rpc_pubsub_addr,
    );
    replica_config
        .replica_exit
        .write()
        .unwrap()
        .register_exit(Box::new(move || trigger.cancel()));

    let (_bank_notification_sender, bank_notification_receiver) = unbounded();
    (
        Some(JsonRpcService::new(
            replica_config.rpc_addr,
            replica_config.rpc_config.clone(),
            replica_config.snapshot_config.clone(),
            bank_forks.clone(),
            block_commitment_cache.clone(),
            blockstore,
            cluster_info,
            None,
            genesis_config.hash(),
            &replica_config.ledger_path,
            replica_config.replica_exit.clone(),
            None,
            rpc_override_health_check,
            optimistically_confirmed_bank.clone(),
            send_transaction_service::Config {
                retry_rate_ms: 0,
                leader_forward_count: 0,
                ..send_transaction_service::Config::default()
            },
            max_slots,
            leader_schedule_cache.clone(),
            max_complete_transaction_status_slot,
        )),
        Some(pubsub_service),
        Some(OptimisticallyConfirmedBankTracker::new(
            bank_notification_receiver,
            &exit,
            bank_forks.clone(),
            optimistically_confirmed_bank.clone(),
            subscriptions,
            None,
        )),
    )
}

impl ReplicaNode {
    pub fn new(replica_config: ReplicaNodeConfig) -> Self {
        let genesis_config = download_then_check_genesis_hash(
            &replica_config.rpc_peer_addr,
            &replica_config.ledger_path,
            None,
            MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
            false,
            true,
        )
        .unwrap();

        let snapshot_config = SnapshotConfig {
            full_snapshot_archive_interval_slots: Slot::MAX,
            incremental_snapshot_archive_interval_slots: Slot::MAX,
            snapshot_archives_dir: replica_config.snapshot_archives_dir.clone(),
            bank_snapshots_dir: replica_config.bank_snapshots_dir.clone(),
            ..SnapshotConfig::default()
        };

        let bank_info =
            initialize_from_snapshot(&replica_config, &snapshot_config, &genesis_config);

        let (json_rpc_service, pubsub_service, optimistically_confirmed_bank_tracker) =
            start_client_rpc_services(
                &replica_config,
                &genesis_config,
                replica_config.cluster_info.clone(),
                &bank_info,
                &replica_config.socket_addr_space,
            );

        let accountsdb_repl_client_config = AccountsDbReplClientServiceConfig {
            worker_threads: 1,
            replica_server_addr: replica_config.accountsdb_repl_peer_addr.unwrap(),
        };

        let last_replicated_slot = bank_info.bank_forks.read().unwrap().root_bank().slot();
        info!(
            "Starting AccountsDbReplService from slot {:?}",
            last_replicated_slot
        );
        let accountsdb_repl_service = Some(
            AccountsDbReplService::new(last_replicated_slot, accountsdb_repl_client_config)
                .expect("Failed to start AccountsDb replication service"),
        );

        info!(
            "Started AccountsDbReplService from slot {:?}",
            last_replicated_slot
        );

        ReplicaNode {
            json_rpc_service,
            pubsub_service,
            optimistically_confirmed_bank_tracker,
            accountsdb_repl_service,
        }
    }

    pub fn join(self) {
        if let Some(json_rpc_service) = self.json_rpc_service {
            json_rpc_service.join().expect("rpc_service");
        }

        if let Some(pubsub_service) = self.pubsub_service {
            pubsub_service.join().expect("pubsub_service");
        }

        if let Some(optimistically_confirmed_bank_tracker) =
            self.optimistically_confirmed_bank_tracker
        {
            optimistically_confirmed_bank_tracker
                .join()
                .expect("optimistically_confirmed_bank_tracker");
        }
        if let Some(accountsdb_repl_service) = self.accountsdb_repl_service {
            accountsdb_repl_service
                .join()
                .expect("accountsdb_repl_service");
        }
    }
}
