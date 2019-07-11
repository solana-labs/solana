//! The `fullnode` module hosts all the fullnode microservices.

use crate::bank_forks::BankForks;
use crate::blocktree::{Blocktree, CompletedSlotsReceiver};
use crate::blocktree_processor::{self, BankForksInfo};
use crate::broadcast_stage::BroadcastStageType;
use crate::cluster_info::{ClusterInfo, Node};
use crate::contact_info::ContactInfo;
use crate::gossip_service::{discover_cluster, GossipService};
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::poh_recorder::PohRecorder;
use crate::poh_service::PohService;
use crate::rpc::JsonRpcConfig;
use crate::rpc_pubsub_service::PubSubService;
use crate::rpc_service::JsonRpcService;
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::storage_stage::StorageState;
use crate::tpu::Tpu;
use crate::tvu::{Sockets, Tvu};
use solana_metrics::datapoint_info;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::poh_config::PohConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::{timestamp, DEFAULT_SLOTS_PER_TURN};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::Result;

#[derive(Clone, Debug)]
pub struct ValidatorConfig {
    pub sigverify_disabled: bool,
    pub voting_disabled: bool,
    pub blockstream: Option<String>,
    pub storage_slots_per_turn: u64,
    pub account_paths: Option<String>,
    pub rpc_config: JsonRpcConfig,
    pub snapshot_path: Option<String>,
    pub broadcast_stage_type: BroadcastStageType,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            sigverify_disabled: false,
            voting_disabled: false,
            blockstream: None,
            storage_slots_per_turn: DEFAULT_SLOTS_PER_TURN,
            account_paths: None,
            rpc_config: JsonRpcConfig::default(),
            snapshot_path: None,
            broadcast_stage_type: BroadcastStageType::Standard,
        }
    }
}

pub struct Validator {
    pub id: Pubkey,
    exit: Arc<AtomicBool>,
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
        ledger_path: &str,
        vote_account: &Pubkey,
        voting_keypair: &Arc<Keypair>,
        storage_keypair: &Arc<Keypair>,
        entrypoint_info_option: Option<&ContactInfo>,
        config: &ValidatorConfig,
    ) -> Self {
        warn!("CUDA is {}abled", if cfg!(cuda) { "en" } else { "dis" });

        let id = keypair.pubkey();
        assert_eq!(id, node.info.id);

        info!("creating bank...");
        let (
            bank_forks,
            bank_forks_info,
            blocktree,
            ledger_signal_receiver,
            completed_slots_receiver,
            leader_schedule_cache,
            poh_config,
        ) = new_banks_from_blocktree(
            ledger_path,
            config.account_paths.clone(),
            config.snapshot_path.clone(),
        );

        let leader_schedule_cache = Arc::new(leader_schedule_cache);
        let exit = Arc::new(AtomicBool::new(false));
        let bank_info = &bank_forks_info[0];
        let bank = bank_forks[bank_info.bank_slot].clone();

        info!(
            "starting PoH... {} {}",
            bank.tick_height(),
            bank.last_blockhash(),
        );
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
        if config.snapshot_path.is_some() {
            poh_recorder.set_bank(&bank);
        }

        let poh_recorder = Arc::new(Mutex::new(poh_recorder));
        let poh_service = PohService::new(poh_recorder.clone(), &poh_config, &exit);
        assert_eq!(
            blocktree.new_blobs_signals.len(),
            1,
            "New blob signal for the TVU should be the same as the clear bank signal."
        );

        info!("node info: {:?}", node.info);
        info!("node entrypoint_info: {:?}", entrypoint_info_option);
        info!(
            "node local gossip address: {}",
            node.sockets.gossip.local_addr().unwrap()
        );

        let bank_forks = Arc::new(RwLock::new(bank_forks));

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
                &exit,
            ))
        };

        let ip_echo_server =
            solana_netutil::ip_echo_server(node.sockets.gossip.local_addr().unwrap().port());

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
            config.blockstream.as_ref(),
            ledger_signal_receiver,
            &subscriptions,
            &poh_recorder,
            &leader_schedule_cache,
            &exit,
            completed_slots_receiver,
        );

        if config.sigverify_disabled {
            warn!("signature verification disabled");
        }

        let tpu = Tpu::new(
            &cluster_info,
            &poh_recorder,
            entry_receiver,
            node.sockets.tpu,
            node.sockets.tpu_via_blobs,
            node.sockets.broadcast,
            config.sigverify_disabled,
            &blocktree,
            &config.broadcast_stage_type,
            &exit,
        );

        datapoint_info!("validator-new");
        Self {
            id,
            gossip_service,
            rpc_service,
            rpc_pubsub_service,
            tpu,
            tvu,
            exit,
            poh_service,
            poh_recorder,
            ip_echo_server,
        }
    }

    // Used for notifying many nodes in parallel to exit
    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> Result<()> {
        self.exit();
        self.join()
    }
}

fn get_bank_forks(
    genesis_block: &GenesisBlock,
    blocktree: &Blocktree,
    account_paths: Option<String>,
    snapshot_path: Option<String>,
) -> (BankForks, Vec<BankForksInfo>, LeaderScheduleCache) {
    if snapshot_path.is_some() {
        let bank_forks =
            BankForks::load_from_snapshot(&genesis_block, account_paths.clone(), &snapshot_path);
        match bank_forks {
            Ok(v) => {
                let bank = &v.working_bank();
                let fork_info = BankForksInfo {
                    bank_slot: bank.slot(),
                    entry_height: bank.tick_height(),
                };
                return (v, vec![fork_info], LeaderScheduleCache::new_from_bank(bank));
            }
            Err(_) => warn!("Failed to load from snapshot, fallback to load from ledger"),
        }
    }
    let (mut bank_forks, bank_forks_info, leader_schedule_cache) =
        blocktree_processor::process_blocktree(&genesis_block, &blocktree, account_paths)
            .expect("process_blocktree failed");
    if snapshot_path.is_some() {
        bank_forks.set_snapshot_config(snapshot_path);
        let _ = bank_forks.add_snapshot(0, 0);
    }
    (bank_forks, bank_forks_info, leader_schedule_cache)
}

pub fn new_banks_from_blocktree(
    blocktree_path: &str,
    account_paths: Option<String>,
    snapshot_path: Option<String>,
) -> (
    BankForks,
    Vec<BankForksInfo>,
    Blocktree,
    Receiver<bool>,
    CompletedSlotsReceiver,
    LeaderScheduleCache,
    PohConfig,
) {
    let genesis_block =
        GenesisBlock::load(blocktree_path).expect("Expected to successfully open genesis block");

    let (blocktree, ledger_signal_receiver, completed_slots_receiver) =
        Blocktree::open_with_signal(blocktree_path)
            .expect("Expected to successfully open database ledger");

    let (bank_forks, bank_forks_info, leader_schedule_cache) =
        get_bank_forks(&genesis_block, &blocktree, account_paths, snapshot_path);

    (
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

pub fn new_validator_for_tests() -> (Validator, ContactInfo, Keypair, String) {
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
        let validators: Vec<Validator> = (0..2)
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
                    &ValidatorConfig::default(),
                )
            })
            .collect();

        // Each validator can exit in parallel to speed many sequential calls to `join`
        validators.iter().for_each(|v| v.exit());
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
