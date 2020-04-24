//! The `validator` module hosts all the validator microservices.

use crate::{
    broadcast_stage::BroadcastStageType,
    cluster_info::{ClusterInfo, Node},
    cluster_info_vote_listener::VoteTracker,
    commitment::BlockCommitmentCache,
    contact_info::ContactInfo,
    gossip_service::{discover_cluster, GossipService},
    poh_recorder::{PohRecorder, GRACE_TICKS_FACTOR, MAX_GRACE_SLOTS},
    poh_service::PohService,
    rewards_recorder_service::RewardsRecorderService,
    rpc::JsonRpcConfig,
    rpc_pubsub_service::PubSubService,
    rpc_service::JsonRpcService,
    rpc_subscriptions::RpcSubscriptions,
    serve_repair::ServeRepair,
    serve_repair_service::ServeRepairService,
    sigverify,
    snapshot_packager_service::SnapshotPackagerService,
    storage_stage::StorageState,
    tpu::Tpu,
    transaction_status_service::TransactionStatusService,
    tvu::{Sockets, Tvu, TvuConfig},
};
use crossbeam_channel::unbounded;
use solana_ledger::{
    bank_forks::{BankForks, SnapshotConfig},
    bank_forks_utils,
    blockstore::{Blockstore, CompletedSlotsReceiver},
    blockstore_processor::{self, BankForksInfo},
    create_new_tmp_ledger,
    hardened_unpack::open_genesis_config,
    leader_schedule::FixedSchedule,
    leader_schedule_cache::LeaderScheduleCache,
};
use solana_metrics::datapoint_info;
use solana_runtime::bank::Bank;
use solana_sdk::{
    clock::{Slot, DEFAULT_SLOTS_PER_TURN},
    epoch_schedule::MAX_LEADER_SCHEDULE_EPOCH_OFFSET,
    genesis_config::GenesisConfig,
    hash::Hash,
    pubkey::Pubkey,
    shred_version::compute_shred_version,
    signature::{Keypair, Signer},
    timing::timestamp,
};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::Receiver,
    sync::{mpsc::channel, Arc, Mutex, RwLock},
    thread::{sleep, Result},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct ValidatorConfig {
    pub dev_sigverify_disabled: bool,
    pub dev_halt_at_slot: Option<Slot>,
    pub expected_genesis_hash: Option<Hash>,
    pub expected_shred_version: Option<u16>,
    pub voting_disabled: bool,
    pub storage_slots_per_turn: u64,
    pub account_paths: Vec<PathBuf>,
    pub rpc_config: JsonRpcConfig,
    pub rpc_ports: Option<(u16, u16)>, // (API, PubSub)
    pub snapshot_config: Option<SnapshotConfig>,
    pub max_ledger_shreds: Option<u64>,
    pub broadcast_stage_type: BroadcastStageType,
    pub enable_partition: Option<Arc<AtomicBool>>,
    pub fixed_leader_schedule: Option<FixedSchedule>,
    pub wait_for_supermajority: Option<Slot>,
    pub new_hard_forks: Option<Vec<Slot>>,
    pub trusted_validators: Option<HashSet<Pubkey>>, // None = trust all
    pub halt_on_trusted_validators_accounts_hash_mismatch: bool,
    pub accounts_hash_fault_injection_slots: u64, // 0 = no fault injection
    pub frozen_accounts: Vec<Pubkey>,
    pub no_rocksdb_compaction: bool,
    pub accounts_hash_interval_slots: u64,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            dev_sigverify_disabled: false,
            dev_halt_at_slot: None,
            expected_genesis_hash: None,
            expected_shred_version: None,
            voting_disabled: false,
            storage_slots_per_turn: DEFAULT_SLOTS_PER_TURN,
            max_ledger_shreds: None,
            account_paths: Vec::new(),
            rpc_config: JsonRpcConfig::default(),
            rpc_ports: None,
            snapshot_config: None,
            broadcast_stage_type: BroadcastStageType::Standard,
            enable_partition: None,
            fixed_leader_schedule: None,
            wait_for_supermajority: None,
            new_hard_forks: None,
            trusted_validators: None,
            halt_on_trusted_validators_accounts_hash_mismatch: false,
            accounts_hash_fault_injection_slots: 0,
            frozen_accounts: vec![],
            no_rocksdb_compaction: false,
            accounts_hash_interval_slots: std::u64::MAX,
        }
    }
}

#[derive(Default)]
pub struct ValidatorExit {
    exits: Vec<Box<dyn FnOnce() + Send + Sync>>,
}

impl ValidatorExit {
    pub fn register_exit(&mut self, exit: Box<dyn FnOnce() -> () + Send + Sync>) {
        self.exits.push(exit);
    }

    pub fn exit(self) {
        for exit in self.exits {
            exit();
        }
    }
}

pub struct Validator {
    pub id: Pubkey,
    validator_exit: Arc<RwLock<Option<ValidatorExit>>>,
    rpc_service: Option<(JsonRpcService, PubSubService)>,
    transaction_status_service: Option<TransactionStatusService>,
    rewards_recorder_service: Option<RewardsRecorderService>,
    gossip_service: GossipService,
    serve_repair_service: ServeRepairService,
    snapshot_packager_service: Option<SnapshotPackagerService>,
    poh_recorder: Arc<Mutex<PohRecorder>>,
    poh_service: PohService,
    tpu: Tpu,
    tvu: Tvu,
    ip_echo_server: solana_net_utils::IpEchoServer,
}

impl Validator {
    #[allow(clippy::cognitive_complexity)]
    pub fn new(
        mut node: Node,
        keypair: &Arc<Keypair>,
        ledger_path: &Path,
        vote_account: &Pubkey,
        mut authorized_voter_keypairs: Vec<Arc<Keypair>>,
        storage_keypair: &Arc<Keypair>,
        entrypoint_info_option: Option<&ContactInfo>,
        poh_verify: bool,
        config: &ValidatorConfig,
    ) -> Self {
        let id = keypair.pubkey();
        assert_eq!(id, node.info.id);

        warn!("identity: {}", id);
        warn!("vote account: {}", vote_account);

        if config.voting_disabled {
            warn!("voting disabled");
            authorized_voter_keypairs.clear();
        } else {
            for authorized_voter_keypair in &authorized_voter_keypairs {
                warn!("authorized voter: {}", authorized_voter_keypair.pubkey());
            }
        }
        report_target_features();

        info!("entrypoint: {:?}", entrypoint_info_option);

        info!("Initializing sigverify, this could take a while...");
        sigverify::init();
        info!("Done.");

        info!("creating bank...");
        let (
            genesis_config,
            bank_forks,
            bank_forks_info,
            blockstore,
            ledger_signal_receiver,
            completed_slots_receiver,
            leader_schedule_cache,
            snapshot_hash,
        ) = new_banks_from_blockstore(config, ledger_path, poh_verify);

        let leader_schedule_cache = Arc::new(leader_schedule_cache);
        let exit = Arc::new(AtomicBool::new(false));
        let bank_info = &bank_forks_info[0];
        let bank = bank_forks[bank_info.bank_slot].clone();

        info!("Starting validator from slot {}", bank.slot());
        {
            let hard_forks: Vec<_> = bank.hard_forks().read().unwrap().iter().copied().collect();
            if !hard_forks.is_empty() {
                info!("Hard forks: {:?}", hard_forks);
            }
        }

        let bank_forks = Arc::new(RwLock::new(bank_forks));

        let mut validator_exit = ValidatorExit::default();
        let exit_ = exit.clone();
        validator_exit.register_exit(Box::new(move || exit_.store(true, Ordering::Relaxed)));
        let validator_exit = Arc::new(RwLock::new(Some(validator_exit)));

        node.info.wallclock = timestamp();
        node.info.shred_version = compute_shred_version(
            &genesis_config.hash(),
            Some(&bank.hard_forks().read().unwrap()),
        );
        Self::print_node_info(&node);

        if let Some(expected_shred_version) = config.expected_shred_version {
            if expected_shred_version != node.info.shred_version {
                error!(
                    "shred version mismatch: expected {}",
                    expected_shred_version
                );
                process::exit(1);
            }
        }

        let cluster_info = Arc::new(ClusterInfo::new(node.info.clone(), keypair.clone()));

        let storage_state = StorageState::new(
            &bank.last_blockhash(),
            config.storage_slots_per_turn,
            bank.slots_per_segment(),
        );

        let blockstore = Arc::new(blockstore);
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::default_with_blockstore(blockstore.clone()),
        ));

        let subscriptions = Arc::new(RpcSubscriptions::new(&exit, block_commitment_cache.clone()));

        let rpc_service = config.rpc_ports.map(|(rpc_port, rpc_pubsub_port)| {
            if ContactInfo::is_valid_address(&node.info.rpc) {
                assert!(ContactInfo::is_valid_address(&node.info.rpc_pubsub));
                assert_eq!(rpc_port, node.info.rpc.port());
                assert_eq!(rpc_pubsub_port, node.info.rpc_pubsub.port());
            } else {
                assert!(!ContactInfo::is_valid_address(&node.info.rpc_pubsub));
            }
            (
                JsonRpcService::new(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), rpc_port),
                    config.rpc_config.clone(),
                    config.snapshot_config.clone(),
                    bank_forks.clone(),
                    block_commitment_cache.clone(),
                    blockstore.clone(),
                    cluster_info.clone(),
                    genesis_config.hash(),
                    ledger_path,
                    storage_state.clone(),
                    validator_exit.clone(),
                    config.trusted_validators.clone(),
                ),
                PubSubService::new(
                    &subscriptions,
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), rpc_pubsub_port),
                    &exit,
                ),
            )
        });

        let (transaction_status_sender, transaction_status_service) =
            if rpc_service.is_some() && config.rpc_config.enable_rpc_transaction_history {
                let (transaction_status_sender, transaction_status_receiver) = unbounded();
                (
                    Some(transaction_status_sender),
                    Some(TransactionStatusService::new(
                        transaction_status_receiver,
                        blockstore.clone(),
                        &exit,
                    )),
                )
            } else {
                (None, None)
            };

        let (rewards_recorder_sender, rewards_recorder_service) =
            if rpc_service.is_some() && config.rpc_config.enable_rpc_transaction_history {
                let (rewards_recorder_sender, rewards_receiver) = unbounded();
                (
                    Some(rewards_recorder_sender),
                    Some(RewardsRecorderService::new(
                        rewards_receiver,
                        blockstore.clone(),
                        &exit,
                    )),
                )
            } else {
                (None, None)
            };

        info!(
            "Starting PoH: epoch={} slot={} tick_height={} blockhash={} leader={:?}",
            bank.epoch(),
            bank.slot(),
            bank.tick_height(),
            bank.last_blockhash(),
            leader_schedule_cache.slot_leader_at(bank.slot(), Some(&bank))
        );

        if config.dev_halt_at_slot.is_some() {
            // Park with the RPC service running, ready for inspection!
            warn!("Validator halted");
            std::thread::park();
        }

        let poh_config = Arc::new(genesis_config.poh_config);
        let (mut poh_recorder, entry_receiver) = PohRecorder::new_with_clear_signal(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.slot(),
            leader_schedule_cache.next_leader_slot(
                &id,
                bank.slot(),
                &bank,
                Some(&blockstore),
                GRACE_TICKS_FACTOR * MAX_GRACE_SLOTS,
            ),
            bank.ticks_per_slot(),
            &id,
            &blockstore,
            blockstore.new_shreds_signals.first().cloned(),
            &leader_schedule_cache,
            &poh_config,
        );
        if config.snapshot_config.is_some() {
            poh_recorder.set_bank(&bank);
        }
        let poh_recorder = Arc::new(Mutex::new(poh_recorder));

        let ip_echo_server = solana_net_utils::ip_echo_server(node.sockets.ip_echo.unwrap());

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(bank_forks.clone()),
            node.sockets.gossip,
            &exit,
        );

        let serve_repair = Arc::new(RwLock::new(ServeRepair::new(cluster_info.clone())));
        let serve_repair_service = ServeRepairService::new(
            &serve_repair,
            Some(blockstore.clone()),
            node.sockets.serve_repair,
            &exit,
        );

        // Insert the entrypoint info, should only be None if this node
        // is the bootstrap validator
        if let Some(entrypoint_info) = entrypoint_info_option {
            cluster_info.set_entrypoint(entrypoint_info.clone());
        }

        let (snapshot_packager_service, snapshot_package_sender) =
            if config.snapshot_config.is_some() {
                // Start a snapshot packaging service
                let (sender, receiver) = channel();
                let snapshot_packager_service =
                    SnapshotPackagerService::new(receiver, snapshot_hash, &exit, &cluster_info);
                (Some(snapshot_packager_service), Some(sender))
            } else {
                (None, None)
            };

        wait_for_supermajority(config, &bank, &cluster_info);

        let poh_service = PohService::new(poh_recorder.clone(), &poh_config, &exit);
        assert_eq!(
            blockstore.new_shreds_signals.len(),
            1,
            "New shred signal for the TVU should be the same as the clear bank signal."
        );

        let vote_tracker = Arc::new({ VoteTracker::new(bank_forks.read().unwrap().root_bank()) });

        let (retransmit_slots_sender, retransmit_slots_receiver) = unbounded();
        let tvu = Tvu::new(
            vote_account,
            authorized_voter_keypairs,
            storage_keypair,
            &bank_forks,
            &cluster_info,
            Sockets {
                repair: node
                    .sockets
                    .repair
                    .try_clone()
                    .expect("Failed to clone repair socket"),
                retransmit: node
                    .sockets
                    .retransmit_sockets
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone retransmit socket"))
                    .collect(),
                fetch: node
                    .sockets
                    .tvu
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TVU Sockets"))
                    .collect(),
                forwards: node
                    .sockets
                    .tvu_forwards
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TVU forwards Sockets"))
                    .collect(),
            },
            blockstore.clone(),
            &storage_state,
            ledger_signal_receiver,
            &subscriptions,
            &poh_recorder,
            &leader_schedule_cache,
            &exit,
            completed_slots_receiver,
            block_commitment_cache,
            config.enable_partition.clone(),
            transaction_status_sender.clone(),
            rewards_recorder_sender,
            snapshot_package_sender,
            vote_tracker.clone(),
            retransmit_slots_sender,
            TvuConfig {
                max_ledger_shreds: config.max_ledger_shreds,
                sigverify_disabled: config.dev_sigverify_disabled,
                halt_on_trusted_validators_accounts_hash_mismatch: config
                    .halt_on_trusted_validators_accounts_hash_mismatch,
                shred_version: node.info.shred_version,
                trusted_validators: config.trusted_validators.clone(),
                accounts_hash_fault_injection_slots: config.accounts_hash_fault_injection_slots,
            },
        );

        if config.dev_sigverify_disabled {
            warn!("signature verification disabled");
        }

        let tpu = Tpu::new(
            &cluster_info,
            &poh_recorder,
            entry_receiver,
            retransmit_slots_receiver,
            node.sockets.tpu,
            node.sockets.tpu_forwards,
            node.sockets.broadcast,
            config.dev_sigverify_disabled,
            transaction_status_sender,
            &blockstore,
            &config.broadcast_stage_type,
            &exit,
            node.info.shred_version,
            vote_tracker,
            bank_forks,
        );

        datapoint_info!("validator-new", ("id", id.to_string(), String));
        Self {
            id,
            gossip_service,
            serve_repair_service,
            rpc_service,
            transaction_status_service,
            rewards_recorder_service,
            snapshot_packager_service,
            tpu,
            tvu,
            poh_service,
            poh_recorder,
            ip_echo_server,
            validator_exit,
        }
    }

    // Used for notifying many nodes in parallel to exit
    pub fn exit(&mut self) {
        if let Some(x) = self.validator_exit.write().unwrap().take() {
            x.exit()
        }
    }

    pub fn close(mut self) -> Result<()> {
        self.exit();
        self.join()
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

    pub fn join(self) -> Result<()> {
        self.poh_service.join()?;
        drop(self.poh_recorder);
        if let Some((rpc_service, rpc_pubsub_service)) = self.rpc_service {
            rpc_service.join()?;
            rpc_pubsub_service.join()?;
        }
        if let Some(transaction_status_service) = self.transaction_status_service {
            transaction_status_service.join()?;
        }

        if let Some(rewards_recorder_service) = self.rewards_recorder_service {
            rewards_recorder_service.join()?;
        }

        if let Some(s) = self.snapshot_packager_service {
            s.join()?;
        }

        self.gossip_service.join()?;
        self.serve_repair_service.join()?;
        self.tpu.join()?;
        self.tvu.join()?;
        self.ip_echo_server.shutdown_now();

        Ok(())
    }
}

#[allow(clippy::type_complexity)]
fn new_banks_from_blockstore(
    config: &ValidatorConfig,
    blockstore_path: &Path,
    poh_verify: bool,
) -> (
    GenesisConfig,
    BankForks,
    Vec<BankForksInfo>,
    Blockstore,
    Receiver<bool>,
    CompletedSlotsReceiver,
    LeaderScheduleCache,
    Option<(Slot, Hash)>,
) {
    let genesis_config = open_genesis_config(blockstore_path);

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
            error!("genesis hash mismatch: expected {}", expected_genesis_hash);
            error!(
                "Delete the ledger directory to continue: {:?}",
                blockstore_path
            );
            process::exit(1);
        }
    }

    let (mut blockstore, ledger_signal_receiver, completed_slots_receiver) =
        Blockstore::open_with_signal(blockstore_path).expect("Failed to open ledger database");
    blockstore.set_no_compaction(config.no_rocksdb_compaction);

    let process_options = blockstore_processor::ProcessOptions {
        poh_verify,
        dev_halt_at_slot: config.dev_halt_at_slot,
        new_hard_forks: config.new_hard_forks.clone(),
        frozen_accounts: config.frozen_accounts.clone(),
        ..blockstore_processor::ProcessOptions::default()
    };

    let (mut bank_forks, bank_forks_info, mut leader_schedule_cache, snapshot_hash) =
        bank_forks_utils::load(
            &genesis_config,
            &blockstore,
            config.account_paths.clone(),
            config.snapshot_config.as_ref(),
            process_options,
        )
        .unwrap_or_else(|err| {
            error!("Failed to load ledger: {:?}", err);
            std::process::exit(1);
        });

    leader_schedule_cache.set_fixed_leader_schedule(config.fixed_leader_schedule.clone());

    bank_forks.set_snapshot_config(config.snapshot_config.clone());
    bank_forks.set_accounts_hash_interval_slots(config.accounts_hash_interval_slots);

    (
        genesis_config,
        bank_forks,
        bank_forks_info,
        blockstore,
        ledger_signal_receiver,
        completed_slots_receiver,
        leader_schedule_cache,
        snapshot_hash,
    )
}

fn wait_for_supermajority(config: &ValidatorConfig, bank: &Bank, cluster_info: &ClusterInfo) {
    if config.wait_for_supermajority != Some(bank.slot()) {
        return;
    }

    info!(
        "Waiting for 80% of activated stake at slot {} to be in gossip...",
        bank.slot()
    );
    for i in 1.. {
        let gossip_stake_percent = get_stake_percent_in_gossip(&bank, &cluster_info, i % 10 == 0);

        if gossip_stake_percent >= 80 {
            break;
        }
        sleep(Duration::new(1, 0));
    }
}

pub struct TestValidator {
    pub server: Validator,
    pub leader_data: ContactInfo,
    pub alice: Keypair,
    pub ledger_path: PathBuf,
    pub genesis_hash: Hash,
    pub vote_pubkey: Pubkey,
}

pub struct TestValidatorOptions {
    pub fees: u64,
    pub bootstrap_validator_lamports: u64,
    pub mint_lamports: u64,
}

impl Default for TestValidatorOptions {
    fn default() -> Self {
        use solana_ledger::genesis_utils::BOOTSTRAP_VALIDATOR_LAMPORTS;
        TestValidatorOptions {
            fees: 0,
            bootstrap_validator_lamports: BOOTSTRAP_VALIDATOR_LAMPORTS,
            mint_lamports: 1_000_000,
        }
    }
}

impl TestValidator {
    pub fn run() -> Self {
        Self::run_with_options(TestValidatorOptions::default())
    }

    pub fn run_with_options(options: TestValidatorOptions) -> Self {
        use solana_ledger::genesis_utils::{
            create_genesis_config_with_leader_ex, GenesisConfigInfo,
        };
        use solana_sdk::fee_calculator::FeeRateGovernor;

        let TestValidatorOptions {
            fees,
            bootstrap_validator_lamports,
            mint_lamports,
        } = options;
        let node_keypair = Arc::new(Keypair::new());
        let node = Node::new_localhost_with_pubkey(&node_keypair.pubkey());
        let contact_info = node.info.clone();

        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            voting_keypair,
        } = create_genesis_config_with_leader_ex(
            mint_lamports,
            &contact_info.id,
            42,
            bootstrap_validator_lamports,
        );
        genesis_config
            .native_instruction_processors
            .push(solana_budget_program!());
        genesis_config
            .native_instruction_processors
            .push(solana_bpf_loader_program!());

        genesis_config.rent.lamports_per_byte_year = 1;
        genesis_config.rent.exemption_threshold = 1.0;
        genesis_config.fee_rate_governor = FeeRateGovernor::new(fees, 0);

        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);

        let leader_voting_keypair = Arc::new(voting_keypair);
        let storage_keypair = Arc::new(Keypair::new());
        let config = ValidatorConfig {
            rpc_ports: Some((node.info.rpc.port(), node.info.rpc_pubsub.port())),
            ..ValidatorConfig::default()
        };
        let node = Validator::new(
            node,
            &node_keypair,
            &ledger_path,
            &leader_voting_keypair.pubkey(),
            vec![leader_voting_keypair.clone()],
            &storage_keypair,
            None,
            true,
            &config,
        );
        discover_cluster(&contact_info.gossip, 1).expect("Node startup failed");
        TestValidator {
            server: node,
            leader_data: contact_info,
            alice: mint_keypair,
            ledger_path,
            genesis_hash: blockhash,
            vote_pubkey: leader_voting_keypair.pubkey(),
        }
    }
}

fn report_target_features() {
    warn!(
        "CUDA is {}abled",
        if solana_perf::perf_libs::api().is_some() {
            "en"
        } else {
            "dis"
        }
    );

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        // Validator binaries built on a machine with AVX support will generate invalid opcodes
        // when run on machines without AVX causing a non-obvious process abort.  Instead detect
        // the mismatch and error cleanly.
        #[target_feature(enable = "avx")]
        {
            if is_x86_feature_detected!("avx") {
                info!("AVX detected");
            } else {
                error!("Your machine does not have AVX support, please rebuild from source on your machine");
                process::exit(1);
            }
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
    let all_tvu_peers = cluster_info.all_tvu_peers();
    let my_shred_version = cluster_info.my_shred_version();
    let my_id = cluster_info.id();

    for (activated_stake, vote_account) in bank.vote_accounts().values() {
        let vote_state =
            solana_vote_program::vote_state::VoteState::from(&vote_account).unwrap_or_default();
        total_activated_stake += activated_stake;

        if *activated_stake == 0 {
            continue;
        }

        if let Some(peer) = all_tvu_peers
            .iter()
            .find(|peer| peer.id == vote_state.node_pubkey)
        {
            if peer.shred_version == my_shred_version {
                trace!(
                    "observed {} in gossip, (activated_stake={})",
                    vote_state.node_pubkey,
                    activated_stake
                );
                online_stake += activated_stake;
            } else {
                wrong_shred_stake += activated_stake;
                wrong_shred_nodes.push((*activated_stake, vote_state.node_pubkey));
            }
        } else if vote_state.node_pubkey == my_id {
            online_stake += activated_stake; // This node is online
        } else {
            offline_stake += activated_stake;
            offline_nodes.push((*activated_stake, vote_state.node_pubkey));
        }
    }

    if log {
        info!(
            "{}% of active stake visible in gossip",
            online_stake * 100 / total_activated_stake
        );

        if !wrong_shred_nodes.is_empty() {
            info!(
                "{}% of active stake has the wrong shred version in gossip",
                wrong_shred_stake * 100 / total_activated_stake,
            );
            for (stake, identity) in wrong_shred_nodes {
                info!(
                    "    {}% - {}",
                    stake * 100 / total_activated_stake,
                    identity
                );
            }
        }

        if !offline_nodes.is_empty() {
            info!(
                "{}% of active stake is not visible in gossip",
                offline_stake * 100 / total_activated_stake
            );
            for (stake, identity) in offline_nodes {
                info!(
                    "    {}% - {}",
                    stake * 100 / total_activated_stake,
                    identity
                );
            }
        }
    }

    online_stake * 100 / total_activated_stake
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_ledger::genesis_utils::create_genesis_config_with_leader;
    use std::fs::remove_dir_all;

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
        let storage_keypair = Arc::new(Keypair::new());
        let config = ValidatorConfig {
            rpc_ports: Some((
                validator_node.info.rpc.port(),
                validator_node.info.rpc_pubsub.port(),
            )),
            ..ValidatorConfig::default()
        };
        let validator = Validator::new(
            validator_node,
            &Arc::new(validator_keypair),
            &validator_ledger_path,
            &voting_keypair.pubkey(),
            vec![voting_keypair.clone()],
            &storage_keypair,
            Some(&leader_node.info),
            true,
            &config,
        );
        validator.close().unwrap();
        remove_dir_all(validator_ledger_path).unwrap();
    }

    #[test]
    fn validator_parallel_exit() {
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());

        let mut ledger_paths = vec![];
        let mut validators: Vec<Validator> = (0..2)
            .map(|_| {
                let validator_keypair = Keypair::new();
                let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());
                let genesis_config =
                    create_genesis_config_with_leader(10_000, &leader_keypair.pubkey(), 1000)
                        .genesis_config;
                let (validator_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);
                ledger_paths.push(validator_ledger_path.clone());
                let vote_account_keypair = Arc::new(Keypair::new());
                let storage_keypair = Arc::new(Keypair::new());
                let config = ValidatorConfig {
                    rpc_ports: Some((
                        validator_node.info.rpc.port(),
                        validator_node.info.rpc_pubsub.port(),
                    )),
                    ..ValidatorConfig::default()
                };
                Validator::new(
                    validator_node,
                    &Arc::new(validator_keypair),
                    &validator_ledger_path,
                    &vote_account_keypair.pubkey(),
                    vec![vote_account_keypair.clone()],
                    &storage_keypair,
                    Some(&leader_node.info),
                    true,
                    &config,
                )
            })
            .collect();

        // Each validator can exit in parallel to speed many sequential calls to join`
        validators.iter_mut().for_each(|v| v.exit());
        // While join is called sequentially, the above exit call notified all the
        // validators to exit from all their threads
        validators.into_iter().for_each(|validator| {
            validator.join().unwrap();
        });

        for path in ledger_paths {
            remove_dir_all(path).unwrap();
        }
    }
}
