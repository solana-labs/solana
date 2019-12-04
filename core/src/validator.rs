//! The `validator` module hosts all the validator microservices.

use crate::{
    broadcast_stage::BroadcastStageType,
    cluster_info::{ClusterInfo, Node},
    commitment::BlockCommitmentCache,
    contact_info::ContactInfo,
    gossip_service::{discover_cluster, GossipService},
    partition_cfg::PartitionCfg,
    poh_recorder::PohRecorder,
    poh_service::PohService,
    rpc::JsonRpcConfig,
    rpc_pubsub_service::PubSubService,
    rpc_service::JsonRpcService,
    rpc_subscriptions::RpcSubscriptions,
    sigverify,
    storage_stage::StorageState,
    tpu::Tpu,
    transaction_status_service::TransactionStatusService,
    tvu::{Sockets, Tvu},
};
use crossbeam_channel::unbounded;
use solana_ledger::{
    bank_forks::{BankForks, SnapshotConfig},
    bank_forks_utils,
    blocktree::{Blocktree, CompletedSlotsReceiver},
    blocktree_processor::{self, BankForksInfo},
    create_new_tmp_ledger,
    leader_schedule::FixedSchedule,
    leader_schedule_cache::LeaderScheduleCache,
};
use solana_metrics::datapoint_info;
use solana_sdk::{
    clock::{Slot, DEFAULT_SLOTS_PER_TURN},
    genesis_config::GenesisConfig,
    hash::Hash,
    poh_config::PohConfig,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    timing::timestamp,
};

use solana_ledger::shred::Shred;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::Receiver,
    sync::{Arc, Mutex, RwLock},
    thread::Result,
};

#[derive(Clone, Debug)]
pub struct ValidatorConfig {
    pub dev_sigverify_disabled: bool,
    pub dev_halt_at_slot: Option<Slot>,
    pub expected_genesis_hash: Option<Hash>,
    pub voting_disabled: bool,
    pub transaction_status_service_disabled: bool,
    pub blockstream_unix_socket: Option<PathBuf>,
    pub storage_slots_per_turn: u64,
    pub account_paths: Option<String>,
    pub rpc_config: JsonRpcConfig,
    pub snapshot_config: Option<SnapshotConfig>,
    pub max_ledger_slots: Option<u64>,
    pub broadcast_stage_type: BroadcastStageType,
    pub partition_cfg: Option<PartitionCfg>,
    pub fixed_leader_schedule: Option<FixedSchedule>,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            dev_sigverify_disabled: false,
            dev_halt_at_slot: None,
            expected_genesis_hash: None,
            voting_disabled: false,
            transaction_status_service_disabled: false,
            blockstream_unix_socket: None,
            storage_slots_per_turn: DEFAULT_SLOTS_PER_TURN,
            max_ledger_slots: None,
            account_paths: None,
            rpc_config: JsonRpcConfig::default(),
            snapshot_config: None,
            broadcast_stage_type: BroadcastStageType::Standard,
            partition_cfg: None,
            fixed_leader_schedule: None,
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
    rpc_service: Option<JsonRpcService>,
    rpc_pubsub_service: Option<PubSubService>,
    transaction_status_service: Option<TransactionStatusService>,
    gossip_service: GossipService,
    poh_recorder: Arc<Mutex<PohRecorder>>,
    poh_service: PohService,
    tpu: Tpu,
    tvu: Tvu,
    ip_echo_server: solana_net_utils::IpEchoServer,
}

impl Validator {
    pub fn new(
        mut node: Node,
        keypair: &Arc<Keypair>,
        ledger_path: &Path,
        vote_account: &Pubkey,
        voting_keypair: &Arc<Keypair>,
        storage_keypair: &Arc<Keypair>,
        entrypoint_info_option: Option<&ContactInfo>,
        poh_verify: bool,
        config: &ValidatorConfig,
    ) -> Self {
        let id = keypair.pubkey();
        assert_eq!(id, node.info.id);

        warn!("identity pubkey: {:?}", id);
        warn!("vote pubkey: {:?}", vote_account);
        warn!(
            "CUDA is {}abled",
            if solana_perf::perf_libs::api().is_some() {
                "en"
            } else {
                "dis"
            }
        );

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

        info!("entrypoint: {:?}", entrypoint_info_option);

        Self::print_node_info(&node);

        info!("Initializing sigverify, this could take a while...");
        sigverify::init();
        info!("Done.");

        info!("creating bank...");
        let (
            genesis_hash,
            bank_forks,
            bank_forks_info,
            blocktree,
            ledger_signal_receiver,
            completed_slots_receiver,
            leader_schedule_cache,
            poh_config,
        ) = new_banks_from_blocktree(
            config.expected_genesis_hash,
            ledger_path,
            config.account_paths.clone(),
            config.snapshot_config.clone(),
            poh_verify,
            config.dev_halt_at_slot,
            config.fixed_leader_schedule.clone(),
        );

        let leader_schedule_cache = Arc::new(leader_schedule_cache);
        let exit = Arc::new(AtomicBool::new(false));
        let bank_info = &bank_forks_info[0];
        let bank = bank_forks[bank_info.bank_slot].clone();
        let bank_forks = Arc::new(RwLock::new(bank_forks));
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        // The version used by shreds, derived from genesis
        let shred_version = Shred::version_from_hash(&genesis_hash);

        let mut validator_exit = ValidatorExit::default();
        let exit_ = exit.clone();
        validator_exit.register_exit(Box::new(move || exit_.store(true, Ordering::Relaxed)));
        let validator_exit = Arc::new(RwLock::new(Some(validator_exit)));

        node.info.wallclock = timestamp();
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(
            node.info.clone(),
            keypair.clone(),
        )));

        let storage_state = StorageState::new(
            &bank.last_blockhash(),
            config.storage_slots_per_turn,
            bank.slots_per_segment(),
        );

        let blocktree = Arc::new(blocktree);

        let rpc_service = if node.info.rpc.port() == 0 {
            None
        } else {
            Some(JsonRpcService::new(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), node.info.rpc.port()),
                config.rpc_config.clone(),
                bank_forks.clone(),
                block_commitment_cache.clone(),
                blocktree.clone(),
                &cluster_info,
                genesis_hash,
                ledger_path,
                storage_state.clone(),
                &validator_exit,
            ))
        };

        let subscriptions = Arc::new(RpcSubscriptions::default());
        let rpc_pubsub_service = if node.info.rpc_pubsub.port() == 0 {
            None
        } else {
            Some(PubSubService::new(
                &subscriptions,
                SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                    node.info.rpc_pubsub.port(),
                ),
                &exit,
            ))
        };

        let (transaction_status_sender, transaction_status_service) =
            if rpc_service.is_some() && !config.transaction_status_service_disabled {
                let (transaction_status_sender, transaction_status_receiver) = unbounded();
                (
                    Some(transaction_status_sender),
                    Some(TransactionStatusService::new(
                        transaction_status_receiver,
                        blocktree.clone(),
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

        let poh_config = Arc::new(poh_config);
        let (mut poh_recorder, entry_receiver) = PohRecorder::new_with_clear_signal(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.slot(),
            leader_schedule_cache.next_leader_slot(&id, bank.slot(), &bank, Some(&blocktree)),
            bank.ticks_per_slot(),
            &id,
            &blocktree,
            blocktree.new_shreds_signals.first().cloned(),
            &leader_schedule_cache,
            &poh_config,
        );
        if config.snapshot_config.is_some() {
            poh_recorder.set_bank(&bank);
        }

        let poh_recorder = Arc::new(Mutex::new(poh_recorder));
        let poh_service = PohService::new(poh_recorder.clone(), &poh_config, &exit);
        assert_eq!(
            blocktree.new_shreds_signals.len(),
            1,
            "New shred signal for the TVU should be the same as the clear bank signal."
        );

        let ip_echo_server = solana_net_utils::ip_echo_server(node.sockets.ip_echo.unwrap());

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(blocktree.clone()),
            Some(bank_forks.clone()),
            node.sockets.gossip,
            &exit,
        );

        // Insert the entrypoint info, should only be None if this node
        // is the bootstrap leader
        if let Some(entrypoint_info) = entrypoint_info_option {
            cluster_info
                .write()
                .unwrap()
                .set_entrypoint(entrypoint_info.clone());
        }

        let sockets = Sockets {
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
        };

        let voting_keypair = if config.voting_disabled {
            None
        } else {
            Some(voting_keypair.clone())
        };

        let tvu = Tvu::new(
            vote_account,
            voting_keypair,
            storage_keypair,
            &bank_forks,
            &cluster_info,
            sockets,
            blocktree.clone(),
            &storage_state,
            config.blockstream_unix_socket.as_ref(),
            config.max_ledger_slots,
            ledger_signal_receiver,
            &subscriptions,
            &poh_recorder,
            &leader_schedule_cache,
            &exit,
            completed_slots_receiver,
            block_commitment_cache,
            config.dev_sigverify_disabled,
            config.partition_cfg.clone(),
            shred_version,
            transaction_status_sender.clone(),
        );

        if config.dev_sigverify_disabled {
            warn!("signature verification disabled");
        }

        let tpu = Tpu::new(
            &cluster_info,
            &poh_recorder,
            entry_receiver,
            node.sockets.tpu,
            node.sockets.tpu_forwards,
            node.sockets.broadcast,
            config.dev_sigverify_disabled,
            transaction_status_sender,
            &blocktree,
            &config.broadcast_stage_type,
            &exit,
            shred_version,
        );

        datapoint_info!("validator-new", ("id", id.to_string(), String));
        Self {
            id,
            gossip_service,
            rpc_service,
            rpc_pubsub_service,
            transaction_status_service,
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
            node.sockets.broadcast.local_addr().unwrap()
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
        if let Some(rpc_service) = self.rpc_service {
            rpc_service.join()?;
        }
        if let Some(rpc_pubsub_service) = self.rpc_pubsub_service {
            rpc_pubsub_service.join()?;
        }
        if let Some(transaction_status_service) = self.transaction_status_service {
            transaction_status_service.join()?;
        }

        self.gossip_service.join()?;
        self.tpu.join()?;
        self.tvu.join()?;
        self.ip_echo_server.shutdown_now();

        Ok(())
    }
}

pub fn new_banks_from_blocktree(
    expected_genesis_hash: Option<Hash>,
    blocktree_path: &Path,
    account_paths: Option<String>,
    snapshot_config: Option<SnapshotConfig>,
    poh_verify: bool,
    dev_halt_at_slot: Option<Slot>,
    fixed_leader_schedule: Option<FixedSchedule>,
) -> (
    Hash,
    BankForks,
    Vec<BankForksInfo>,
    Blocktree,
    Receiver<bool>,
    CompletedSlotsReceiver,
    LeaderScheduleCache,
    PohConfig,
) {
    let genesis_config = GenesisConfig::load(blocktree_path).unwrap_or_else(|err| {
        error!("Failed to load genesis from {:?}: {}", blocktree_path, err);
        process::exit(1);
    });
    let genesis_hash = genesis_config.hash();
    info!("genesis hash: {}", genesis_hash);

    if let Some(expected_genesis_hash) = expected_genesis_hash {
        if genesis_hash != expected_genesis_hash {
            error!("genesis hash mismatch: expected {}", expected_genesis_hash);
            error!(
                "Delete the ledger directory to continue: {:?}",
                blocktree_path
            );
            process::exit(1);
        }
    }

    let (blocktree, ledger_signal_receiver, completed_slots_receiver) =
        Blocktree::open_with_signal(blocktree_path).expect("Failed to open ledger database");

    let process_options = blocktree_processor::ProcessOptions {
        poh_verify,
        dev_halt_at_slot,
        ..blocktree_processor::ProcessOptions::default()
    };

    let (mut bank_forks, bank_forks_info, mut leader_schedule_cache) = bank_forks_utils::load(
        &genesis_config,
        &blocktree,
        account_paths,
        snapshot_config.as_ref(),
        process_options,
    )
    .unwrap_or_else(|err| {
        error!("Failed to load ledger: {:?}", err);
        std::process::exit(1);
    });

    leader_schedule_cache.set_fixed_leader_schedule(fixed_leader_schedule);

    bank_forks.set_snapshot_config(snapshot_config);

    (
        genesis_hash,
        bank_forks,
        bank_forks_info,
        blocktree,
        ledger_signal_receiver,
        completed_slots_receiver,
        leader_schedule_cache,
        genesis_config.poh_config,
    )
}

pub fn new_validator_for_tests() -> (Validator, ContactInfo, Keypair, PathBuf) {
    use crate::genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo};

    let node_keypair = Arc::new(Keypair::new());
    let node = Node::new_localhost_with_pubkey(&node_keypair.pubkey());
    let contact_info = node.info.clone();

    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        voting_keypair,
    } = create_genesis_config_with_leader(1_000_000, &contact_info.id, 42);
    genesis_config
        .native_instruction_processors
        .push(solana_budget_program!());

    genesis_config.rent.lamports_per_byte_year = 1;
    genesis_config.rent.exemption_threshold = 1.0;

    let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

    let leader_voting_keypair = Arc::new(voting_keypair);
    let storage_keypair = Arc::new(Keypair::new());
    let mut config = ValidatorConfig::default();
    config.transaction_status_service_disabled = true;
    let node = Validator::new(
        node,
        &node_keypair,
        &ledger_path,
        &leader_voting_keypair.pubkey(),
        &leader_voting_keypair,
        &storage_keypair,
        None,
        true,
        &config,
    );
    discover_cluster(&contact_info.gossip, 1).expect("Node startup failed");
    (node, contact_info, mint_keypair, ledger_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::create_genesis_config_with_leader;
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
        let mut config = ValidatorConfig::default();
        config.transaction_status_service_disabled = true;
        let validator = Validator::new(
            validator_node,
            &Arc::new(validator_keypair),
            &validator_ledger_path,
            &voting_keypair.pubkey(),
            &voting_keypair,
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
                let voting_keypair = Arc::new(Keypair::new());
                let storage_keypair = Arc::new(Keypair::new());
                let mut config = ValidatorConfig::default();
                config.transaction_status_service_disabled = true;
                Validator::new(
                    validator_node,
                    &Arc::new(validator_keypair),
                    &validator_ledger_path,
                    &voting_keypair.pubkey(),
                    &voting_keypair,
                    &storage_keypair,
                    Some(&leader_node.info),
                    true,
                    &config,
                )
            })
            .collect();

        // Each validator can exit in parallel to speed many sequential calls to `join`
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
