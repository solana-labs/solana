//! The `fullnode` module hosts all the fullnode microservices.

use crate::bank_forks::{BankForks, SnapshotConfig};
use crate::blocktree::{Blocktree, CompletedSlotsReceiver};
use crate::blocktree_processor::{self, BankForksInfo};
use crate::broadcast_stage::BroadcastStageType;
use crate::cluster_info::{ClusterInfo, Node};
use crate::contact_info::ContactInfo;
use crate::erasure::ErasureConfig;
use crate::gossip_service::{discover_cluster, GossipService};
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::poh_recorder::PohRecorder;
use crate::poh_service::PohService;
use crate::rpc::JsonRpcConfig;
use crate::rpc_pubsub_service::PubSubService;
use crate::rpc_service::JsonRpcService;
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::snapshot_utils;
use crate::storage_stage::StorageState;
use crate::tpu::Tpu;
use crate::tvu::{Sockets, Tvu};
use solana_metrics::datapoint_info;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::poh_config::PohConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::{timestamp, Slot, DEFAULT_SLOTS_PER_TURN};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::Result;

#[derive(Clone, Debug)]
pub struct ValidatorConfig {
    pub dev_sigverify_disabled: bool,
    pub dev_halt_at_slot: Option<Slot>,
    pub expected_genesis_blockhash: Option<Hash>,
    pub voting_disabled: bool,
    pub blockstream_unix_socket: Option<PathBuf>,
    pub storage_slots_per_turn: u64,
    pub account_paths: Option<String>,
    pub rpc_config: JsonRpcConfig,
    pub snapshot_config: Option<SnapshotConfig>,
    pub max_ledger_slots: Option<u64>,
    pub broadcast_stage_type: BroadcastStageType,
    pub erasure_config: ErasureConfig,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            dev_sigverify_disabled: false,
            dev_halt_at_slot: None,
            expected_genesis_blockhash: None,
            voting_disabled: false,
            blockstream_unix_socket: None,
            storage_slots_per_turn: DEFAULT_SLOTS_PER_TURN,
            max_ledger_slots: None,
            account_paths: None,
            rpc_config: JsonRpcConfig::default(),
            snapshot_config: None,
            broadcast_stage_type: BroadcastStageType::Standard,
            erasure_config: ErasureConfig::default(),
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
    gossip_service: GossipService,
    poh_recorder: Arc<Mutex<PohRecorder>>,
    poh_service: PohService,
    tpu: Tpu,
    tvu: Tvu,
    ip_echo_server: solana_netutil::IpEchoServer,
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
        verify_ledger: bool,
        config: &ValidatorConfig,
    ) -> Self {
        let id = keypair.pubkey();
        assert_eq!(id, node.info.id);

        warn!("identity pubkey: {:?}", id);
        warn!("vote pubkey: {:?}", vote_account);
        warn!("CUDA is {}abled", if cfg!(cuda) { "en" } else { "dis" });
        info!("entrypoint: {:?}", entrypoint_info_option);
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
            node.sockets.retransmit.local_addr().unwrap()
        );

        info!("creating bank...");
        let (
            genesis_blockhash,
            bank_forks,
            bank_forks_info,
            blocktree,
            ledger_signal_receiver,
            completed_slots_receiver,
            leader_schedule_cache,
            poh_config,
        ) = new_banks_from_blocktree(
            config.expected_genesis_blockhash,
            ledger_path,
            config.account_paths.clone(),
            config.snapshot_config.clone(),
            verify_ledger,
            config.dev_halt_at_slot,
        );

        let leader_schedule_cache = Arc::new(leader_schedule_cache);
        let exit = Arc::new(AtomicBool::new(false));
        let bank_info = &bank_forks_info[0];
        let bank = bank_forks[bank_info.bank_slot].clone();
        let bank_forks = Arc::new(RwLock::new(bank_forks));

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

        let rpc_service = if node.info.rpc.port() == 0 {
            None
        } else {
            Some(JsonRpcService::new(
                &cluster_info,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), node.info.rpc.port()),
                storage_state.clone(),
                config.rpc_config.clone(),
                bank_forks.clone(),
                ledger_path,
                genesis_blockhash,
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

        let blocktree = Arc::new(blocktree);

        let poh_config = Arc::new(poh_config);
        let (mut poh_recorder, entry_receiver) = PohRecorder::new_with_clear_signal(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.slot(),
            leader_schedule_cache.next_leader_slot(&id, bank.slot(), &bank, Some(&blocktree)),
            bank.ticks_per_slot(),
            &id,
            &blocktree,
            blocktree.new_blobs_signals.first().cloned(),
            &leader_schedule_cache,
            &poh_config,
        );
        if config.snapshot_config.is_some() {
            poh_recorder.set_bank(&bank);
        }

        let poh_recorder = Arc::new(Mutex::new(poh_recorder));
        let poh_service = PohService::new(poh_recorder.clone(), &poh_config, &exit);
        assert_eq!(
            blocktree.new_blobs_signals.len(),
            1,
            "New blob signal for the TVU should be the same as the clear bank signal."
        );

        let ip_echo_server =
            solana_netutil::ip_echo_server(node.sockets.gossip.local_addr().unwrap().port());

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
                .retransmit
                .try_clone()
                .expect("Failed to clone retransmit socket"),
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
            Some(voting_keypair)
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
            &blocktree,
            &config.broadcast_stage_type,
            &config.erasure_config,
            &exit,
        );

        datapoint_info!("validator-new", ("id", id.to_string(), String));
        Self {
            id,
            gossip_service,
            rpc_service,
            rpc_pubsub_service,
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
}

fn get_bank_forks(
    genesis_block: &GenesisBlock,
    blocktree: &Blocktree,
    account_paths: Option<String>,
    snapshot_config: Option<&SnapshotConfig>,
    verify_ledger: bool,
    dev_halt_at_slot: Option<Slot>,
) -> (BankForks, Vec<BankForksInfo>, LeaderScheduleCache) {
    if let Some(snapshot_config) = snapshot_config.as_ref() {
        info!(
            "Initializing snapshot path: {:?}",
            snapshot_config.snapshot_path
        );
        let _ = fs::remove_dir_all(&snapshot_config.snapshot_path);
        fs::create_dir_all(&snapshot_config.snapshot_path)
            .expect("Couldn't create snapshot directory");

        let tar =
            snapshot_utils::get_snapshot_tar_path(&snapshot_config.snapshot_package_output_path);
        if tar.exists() {
            info!("Loading snapshot package: {:?}", tar);
            // Fail hard here if snapshot fails to load, don't silently continue
            let deserialized_bank = snapshot_utils::bank_from_archive(
                account_paths
                    .clone()
                    .expect("Account paths not present when booting from snapshot"),
                snapshot_config,
                &tar,
            )
            .expect("Load from snapshot failed");

            return blocktree_processor::process_blocktree_from_root(
                blocktree,
                Arc::new(deserialized_bank),
                verify_ledger,
                dev_halt_at_slot,
            )
            .expect("processing blocktree after loading snapshot failed");
        } else {
            info!("Snapshot package does not exist: {:?}", tar);
        }
    } else {
        info!("Snapshots disabled");
    }

    info!("Processing ledger from genesis");
    blocktree_processor::process_blocktree(
        &genesis_block,
        &blocktree,
        account_paths,
        verify_ledger,
        dev_halt_at_slot,
    )
    .expect("process_blocktree failed")
}

#[cfg(not(unix))]
fn adjust_ulimit_nofile() {}

#[cfg(unix)]
fn adjust_ulimit_nofile() {
    // Rocks DB likes to have many open files.  The default open file descriptor limit is
    // usually not enough
    let desired_nofile = 65000;

    fn get_nofile() -> libc::rlimit {
        let mut nofile = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut nofile) } != 0 {
            warn!("getrlimit(RLIMIT_NOFILE) failed");
        }
        nofile
    }

    let mut nofile = get_nofile();
    if nofile.rlim_cur < desired_nofile {
        nofile.rlim_cur = desired_nofile;
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &nofile) } != 0 {
            error!(
                "Unable to increase the maximum open file descriptor limit to {}",
                desired_nofile
            );

            if cfg!(target_os = "macos") {
                error!("On mac OS you may need to run |sudo launchctl limit maxfiles 65536 200000| first");
            }
        }

        nofile = get_nofile();
    }
    info!("Maximum open file descriptors: {}", nofile.rlim_cur);
}

pub fn new_banks_from_blocktree(
    expected_genesis_blockhash: Option<Hash>,
    blocktree_path: &Path,
    account_paths: Option<String>,
    snapshot_config: Option<SnapshotConfig>,
    verify_ledger: bool,
    dev_halt_at_slot: Option<Slot>,
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
    let genesis_block = GenesisBlock::load(blocktree_path).expect("Failed to load genesis block");
    let genesis_blockhash = genesis_block.hash();

    if let Some(expected_genesis_blockhash) = expected_genesis_blockhash {
        if genesis_blockhash != expected_genesis_blockhash {
            panic!(
                "Genesis blockhash mismatch: expected {} but local genesis blockhash is {}",
                expected_genesis_blockhash, genesis_blockhash,
            );
        }
    }

    adjust_ulimit_nofile();

    let (blocktree, ledger_signal_receiver, completed_slots_receiver) =
        Blocktree::open_with_signal(blocktree_path).expect("Failed to open ledger database");

    let (mut bank_forks, bank_forks_info, leader_schedule_cache) = get_bank_forks(
        &genesis_block,
        &blocktree,
        account_paths,
        snapshot_config.as_ref(),
        verify_ledger,
        dev_halt_at_slot,
    );

    if snapshot_config.is_some() {
        bank_forks.set_snapshot_config(snapshot_config.unwrap());
    }

    (
        genesis_blockhash,
        bank_forks,
        bank_forks_info,
        blocktree,
        ledger_signal_receiver,
        completed_slots_receiver,
        leader_schedule_cache,
        genesis_block.poh_config,
    )
}

impl Service for Validator {
    type JoinReturnType = ();

    fn join(self) -> Result<()> {
        self.poh_service.join()?;
        drop(self.poh_recorder);
        if let Some(rpc_service) = self.rpc_service {
            rpc_service.join()?;
        }
        if let Some(rpc_pubsub_service) = self.rpc_pubsub_service {
            rpc_pubsub_service.join()?;
        }

        self.gossip_service.join()?;
        self.tpu.join()?;
        self.tvu.join()?;
        self.ip_echo_server.shutdown_now();

        Ok(())
    }
}

pub fn new_validator_for_tests() -> (Validator, ContactInfo, Keypair, PathBuf) {
    use crate::blocktree::create_new_tmp_ledger;
    use crate::genesis_utils::{create_genesis_block_with_leader, GenesisBlockInfo};

    let node_keypair = Arc::new(Keypair::new());
    let node = Node::new_localhost_with_pubkey(&node_keypair.pubkey());
    let contact_info = node.info.clone();

    let GenesisBlockInfo {
        mut genesis_block,
        mint_keypair,
        ..
    } = create_genesis_block_with_leader(10_000, &contact_info.id, 42);
    genesis_block
        .native_instruction_processors
        .push(solana_budget_program!());

    let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

    let voting_keypair = Arc::new(Keypair::new());
    let storage_keypair = Arc::new(Keypair::new());
    let node = Validator::new(
        node,
        &node_keypair,
        &ledger_path,
        &voting_keypair.pubkey(),
        &voting_keypair,
        &storage_keypair,
        None,
        true,
        &ValidatorConfig::default(),
    );
    discover_cluster(&contact_info.gossip, 1).expect("Node startup failed");
    (node, contact_info, mint_keypair, ledger_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::create_new_tmp_ledger;
    use crate::genesis_utils::create_genesis_block_with_leader;
    use std::fs::remove_dir_all;

    #[test]
    fn validator_exit() {
        solana_logger::setup();
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());

        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());
        let genesis_block =
            create_genesis_block_with_leader(10_000, &leader_keypair.pubkey(), 1000).genesis_block;
        let (validator_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

        let voting_keypair = Arc::new(Keypair::new());
        let storage_keypair = Arc::new(Keypair::new());
        let validator = Validator::new(
            validator_node,
            &Arc::new(validator_keypair),
            &validator_ledger_path,
            &voting_keypair.pubkey(),
            &voting_keypair,
            &storage_keypair,
            Some(&leader_node.info),
            true,
            &ValidatorConfig::default(),
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
                let genesis_block =
                    create_genesis_block_with_leader(10_000, &leader_keypair.pubkey(), 1000)
                        .genesis_block;
                let (validator_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);
                ledger_paths.push(validator_ledger_path.clone());
                let voting_keypair = Arc::new(Keypair::new());
                let storage_keypair = Arc::new(Keypair::new());
                Validator::new(
                    validator_node,
                    &Arc::new(validator_keypair),
                    &validator_ledger_path,
                    &voting_keypair.pubkey(),
                    &voting_keypair,
                    &storage_keypair,
                    Some(&leader_node.info),
                    true,
                    &ValidatorConfig::default(),
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
