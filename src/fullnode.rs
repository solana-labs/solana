//! The `fullnode` module hosts all the fullnode microservices.

use crate::blocktree::{Blocktree, BlocktreeConfig};
use crate::blocktree_processor;
use crate::cluster_info::{ClusterInfo, Node, NodeInfo};
use crate::gossip_service::GossipService;
use crate::leader_scheduler::{LeaderScheduler, LeaderSchedulerConfig};
use crate::poh_service::PohServiceConfig;
use crate::rpc_pubsub_service::PubSubService;
use crate::rpc_service::JsonRpcService;
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::storage_stage::StorageState;
use crate::tpu::Tpu;
use crate::tvu::{Sockets, Tvu, TvuRotationReceiver};
use crate::voting_keypair::VotingKeypair;
use log::Level;
use solana_metrics::counter::Counter;
use solana_runtime::bank::Bank;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::timestamp;
use std::net::UdpSocket;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, spawn, Result};
use std::time::Duration;

struct NodeServices {
    tpu: Tpu,
    tvu: Tvu,
}

impl NodeServices {
    fn new(tpu: Tpu, tvu: Tvu) -> Self {
        NodeServices { tpu, tvu }
    }

    fn join(self) -> Result<()> {
        self.tpu.join()?;
        //tvu will never stop unless exit is signaled
        self.tvu.join()?;
        Ok(())
    }

    fn exit(&self) {
        self.tpu.exit();
        self.tvu.exit();
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FullnodeReturnType {
    LeaderToValidatorRotation,
    ValidatorToLeaderRotation,
    LeaderToLeaderRotation,
}

pub struct FullnodeConfig {
    pub sigverify_disabled: bool,
    pub voting_disabled: bool,
    pub entry_stream: Option<String>,
    pub storage_rotate_count: u64,
    pub leader_scheduler_config: LeaderSchedulerConfig,
    pub tick_config: PohServiceConfig,
    pub account_paths: String,
}
impl Default for FullnodeConfig {
    fn default() -> Self {
        // TODO: remove this, temporary parameter to configure
        // storage amount differently for test configurations
        // so tests don't take forever to run.
        const NUM_HASHES_FOR_STORAGE_ROTATE: u64 = 1024;
        Self {
            sigverify_disabled: false,
            voting_disabled: false,
            entry_stream: None,
            storage_rotate_count: NUM_HASHES_FOR_STORAGE_ROTATE,
            leader_scheduler_config: LeaderSchedulerConfig::default(),
            tick_config: PohServiceConfig::default(),
            account_paths: "0,1,2,3".to_string(),
        }
    }
}

impl FullnodeConfig {
    pub fn ledger_config(&self) -> BlocktreeConfig {
        // TODO: Refactor LeaderSchedulerConfig and BlocktreeConfig to avoid the duplicated
        //       `ticks_per_slot` field that must be identical between the two
        BlocktreeConfig::new(self.leader_scheduler_config.ticks_per_slot)
    }
}

pub struct Fullnode {
    id: Pubkey,
    exit: Arc<AtomicBool>,
    rpc_service: Option<JsonRpcService>,
    rpc_pubsub_service: Option<PubSubService>,
    gossip_service: GossipService,
    bank: Arc<Bank>,
    sigverify_disabled: bool,
    tpu_sockets: Vec<UdpSocket>,
    broadcast_socket: UdpSocket,
    node_services: NodeServices,
    rotation_receiver: TvuRotationReceiver,
    blocktree: Arc<Blocktree>,
    leader_scheduler: Arc<RwLock<LeaderScheduler>>,
}

impl Fullnode {
    pub fn new(
        mut node: Node,
        keypair: &Arc<Keypair>,
        ledger_path: &str,
        voting_keypair: VotingKeypair,
        entrypoint_info_option: Option<&NodeInfo>,
        config: &FullnodeConfig,
    ) -> Self {
        info!("creating bank...");

        let id = keypair.pubkey();
        assert_eq!(id, node.info.id);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::new(
            &config.leader_scheduler_config,
        )));
        let (bank, entry_height, last_entry_id, blocktree, ledger_signal_receiver) =
            new_bank_from_ledger(
                ledger_path,
                &config.account_paths,
                &config.ledger_config(),
                &leader_scheduler,
            );

        info!("node info: {:?}", node.info);
        info!("node entrypoint_info: {:?}", entrypoint_info_option);
        info!(
            "node local gossip address: {}",
            node.sockets.gossip.local_addr().unwrap()
        );

        let exit = Arc::new(AtomicBool::new(false));
        let blocktree = Arc::new(blocktree);

        node.info.wallclock = timestamp();
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_keypair(
            node.info.clone(),
            keypair.clone(),
        )));

        // TODO: The RPC service assumes that there is a drone running on the cluster
        //       entrypoint, which is a bad assumption.
        //       See https://github.com/solana-labs/solana/issues/1830 for the removal of drone
        //       from the RPC API
        let drone_addr = {
            let mut entrypoint_drone_addr = match entrypoint_info_option {
                Some(entrypoint_info_info) => entrypoint_info_info.rpc,
                None => node.info.rpc,
            };
            entrypoint_drone_addr.set_port(solana_drone::drone::DRONE_PORT);
            entrypoint_drone_addr
        };

        let storage_state = StorageState::new();

        let rpc_service = JsonRpcService::new(
            &bank,
            &cluster_info,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), node.info.rpc.port()),
            drone_addr,
            storage_state.clone(),
        );

        let subscriptions = Arc::new(RpcSubscriptions::default());
        let rpc_pubsub_service = PubSubService::new(
            &subscriptions,
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                node.info.rpc_pubsub.port(),
            ),
        );

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(blocktree.clone()),
            Some(bank.clone()),
            node.sockets.gossip,
            exit.clone(),
        );

        // Insert the entrypoint info, should only be None if this node
        // is the bootstrap leader
        if let Some(entrypoint_info) = entrypoint_info_option {
            cluster_info
                .write()
                .unwrap()
                .insert_info(entrypoint_info.clone());
        }

        // Figure which node should generate the next tick
        let (scheduled_leader, max_tick_height, blob_index) = {
            let next_tick = bank.tick_height() + 1;

            let leader_scheduler = leader_scheduler.read().unwrap();
            let slot_at_next_tick = leader_scheduler.tick_height_to_slot(next_tick);

            let scheduled_leader = leader_scheduler
                .get_leader_for_slot(slot_at_next_tick)
                .expect("Leader not known after processing bank");
            let max_tick_height = next_tick + leader_scheduler.num_ticks_left_in_slot(next_tick);
            let blob_index =
                if let Some(meta) = blocktree.meta(slot_at_next_tick).expect("Database error") {
                    meta.consumed
                } else {
                    0
                };

            trace!(
                "node {:?} scheduled as leader for ticks ({},{}), starting blob_index={}",
                scheduled_leader,
                next_tick,
                max_tick_height,
                blob_index,
            );

            (scheduled_leader, max_tick_height, blob_index)
        };

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

        let voting_keypair_option = if config.voting_disabled {
            None
        } else {
            Some(Arc::new(voting_keypair))
        };

        // Setup channel for rotation indications
        let (rotation_sender, rotation_receiver) = channel();

        let tvu = Tvu::new(
            voting_keypair_option,
            &bank,
            blob_index,
            entry_height,
            last_entry_id,
            &cluster_info,
            sockets,
            blocktree.clone(),
            config.storage_rotate_count,
            &rotation_sender,
            &storage_state,
            config.entry_stream.as_ref(),
            ledger_signal_receiver,
            leader_scheduler.clone(),
            &subscriptions,
        );
        let tpu = Tpu::new(id, &cluster_info);

        let mut fullnode = Self {
            id,
            bank,
            sigverify_disabled: config.sigverify_disabled,
            gossip_service,
            rpc_service: Some(rpc_service),
            rpc_pubsub_service: Some(rpc_pubsub_service),
            node_services: NodeServices::new(tpu, tvu),
            exit,
            tpu_sockets: node.sockets.tpu,
            broadcast_socket: node.sockets.broadcast,
            rotation_receiver,
            blocktree,
            leader_scheduler,
        };

        fullnode.rotate(
            scheduled_leader,
            max_tick_height,
            blob_index,
            &last_entry_id,
        );
        inc_new_counter_info!("fullnode-new", 1);
        fullnode
    }

    fn get_next_leader(&self, tick_height: u64) -> (Pubkey, u64) {
        loop {
            let bank_tick_height = self.bank.tick_height();
            if bank_tick_height >= tick_height {
                break;
            }
            trace!(
                "{:?}: Waiting for bank tick_height to catch up from {} to {}",
                self.id,
                bank_tick_height,
                tick_height
            );
            sleep(Duration::from_millis(10));
        }

        let (scheduled_leader, max_tick_height) = {
            let mut leader_scheduler = self.leader_scheduler.write().unwrap();

            // A transition is only permitted on the final tick of a slot
            assert_eq!(leader_scheduler.num_ticks_left_in_slot(tick_height), 0);
            let first_tick_of_next_slot = tick_height + 1;

            leader_scheduler.update_tick_height(first_tick_of_next_slot, &self.bank);
            let slot = leader_scheduler.tick_height_to_slot(first_tick_of_next_slot);
            (
                leader_scheduler.get_leader_for_slot(slot).unwrap(),
                first_tick_of_next_slot
                    + leader_scheduler.num_ticks_left_in_slot(first_tick_of_next_slot),
            )
        };

        debug!(
            "node {:?} scheduled as leader for ticks [{}, {})",
            scheduled_leader,
            tick_height + 1,
            max_tick_height
        );

        (scheduled_leader, max_tick_height)
    }

    fn rotate(
        &mut self,
        next_leader: Pubkey,
        max_tick_height: u64,
        blob_index: u64,
        last_entry_id: &Hash,
    ) -> FullnodeReturnType {
        if next_leader == self.id {
            let transition = match self.node_services.tpu.is_leader() {
                Some(was_leader) => {
                    if was_leader {
                        debug!("{:?} remaining in leader role", self.id);
                        FullnodeReturnType::LeaderToLeaderRotation
                    } else {
                        debug!("{:?} rotating to leader role", self.id);
                        FullnodeReturnType::ValidatorToLeaderRotation
                    }
                }
                None => FullnodeReturnType::LeaderToLeaderRotation, // value doesn't matter here...
            };
            let tpu_bank = Arc::new(Bank::new_from_parent(&self.bank));
            self.node_services.tpu.switch_to_leader(
                &tpu_bank,
                PohServiceConfig::default(),
                self.tpu_sockets
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                    .collect(),
                self.broadcast_socket
                    .try_clone()
                    .expect("Failed to clone broadcast socket"),
                self.sigverify_disabled,
                max_tick_height,
                blob_index,
                last_entry_id,
                &self.blocktree,
                &self.leader_scheduler,
            );
            transition
        } else {
            debug!("{:?} rotating to validator role", self.id);
            self.node_services.tpu.switch_to_forwarder(
                next_leader,
                self.tpu_sockets
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                    .collect(),
            );
            FullnodeReturnType::LeaderToValidatorRotation
        }
    }

    // Runs a thread to manage node role transitions.  The returned closure can be used to signal the
    // node to exit.
    pub fn run(
        mut self,
        rotation_notifier: Option<Sender<(FullnodeReturnType, u64)>>,
    ) -> impl FnOnce() {
        let (sender, receiver) = channel();
        let exit = self.exit.clone();
        let timeout = Duration::from_secs(1);
        spawn(move || loop {
            if self.exit.load(Ordering::Relaxed) {
                debug!("node shutdown requested");
                self.close().expect("Unable to close node");
                sender.send(true).expect("Unable to signal exit");
                break;
            }

            match self.rotation_receiver.recv_timeout(timeout) {
                Ok(tick_height) => {
                    trace!("{:?}: rotate at tick_height={}", self.id, tick_height);
                    let (next_leader, max_tick_height) = self.get_next_leader(tick_height);
                    let transition =
                        self.rotate(next_leader, max_tick_height, 0, &self.bank.last_id());
                    debug!("role transition complete: {:?}", transition);
                    if let Some(ref rotation_notifier) = rotation_notifier {
                        rotation_notifier
                            .send((transition, tick_height + 1))
                            .unwrap();
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                _ => (),
            }
        });
        move || {
            exit.store(true, Ordering::Relaxed);
            receiver.recv().unwrap();
            debug!("node shutdown complete");
        }
    }

    // Used for notifying many nodes in parallel to exit
    fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
        if let Some(ref rpc_service) = self.rpc_service {
            rpc_service.exit();
        }
        if let Some(ref rpc_pubsub_service) = self.rpc_pubsub_service {
            rpc_pubsub_service.exit();
        }
        self.node_services.exit()
    }

    pub fn close(self) -> Result<()> {
        self.exit();
        self.join()
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn new_banks_from_blocktree(
    blocktree_path: &str,
    account_paths: &str,
    blocktree_config: &BlocktreeConfig,
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> (Arc<Bank>, u64, Hash, Blocktree, Receiver<bool>) {
    let (blocktree, ledger_signal_receiver) =
        Blocktree::open_with_config_signal(blocktree_path, blocktree_config)
            .expect("Expected to successfully open database ledger");
    let genesis_block =
        GenesisBlock::load(blocktree_path).expect("Expected to successfully open genesis block");

    let (mut bank_forks, bank_forks_info) =
        blocktree_processor::process_blocktree(&genesis_block, &blocktree, leader_scheduler)
            .expect("process_blocktree failed");

    if bank_forks_info.len() != 1 {
        warn!("TODO: figure out what to do with multiple bank forks");
    }
    bank_forks.set_working_bank_id(bank_forks_info[0].bank_id);
    (
        bank_forks.working_bank(),
        bank_forks_info[0].entry_height,
        bank_forks_info[0].last_entry_id,
        blocktree,
        ledger_signal_receiver,
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub fn new_bank_from_ledger(
    ledger_path: &str,
    account_paths: &str,
    ledger_config: &BlocktreeConfig,
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> (Arc<Bank>, u64, Hash, Blocktree, Receiver<bool>) {
    let (working_bank, entry_height, last_entry_id, blocktree, ledger_signal_receiver) =
        new_banks_from_blocktree(ledger_path, account_paths, ledger_config, leader_scheduler);
    (
        working_bank,
        entry_height,
        last_entry_id,
        blocktree,
        ledger_signal_receiver,
    )
}

impl Service for Fullnode {
    type JoinReturnType = ();

    fn join(self) -> Result<()> {
        if let Some(rpc_service) = self.rpc_service {
            rpc_service.join()?;
        }
        if let Some(rpc_pubsub_service) = self.rpc_pubsub_service {
            rpc_pubsub_service.join()?;
        }

        self.gossip_service.join()?;
        self.node_services.join()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::{
        create_tmp_sample_ledger, tmp_copy_ledger, BlocktreeConfig, DEFAULT_SLOT_HEIGHT,
    };
    use crate::entry::make_consecutive_blobs;
    use crate::leader_scheduler::make_active_set_entries;
    use crate::streamer::responder;
    use std::fs::remove_dir_all;

    #[test]
    fn validator_exit() {
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
        let (
            _mint_keypair,
            validator_ledger_path,
            _tick_height,
            _last_entry_height,
            _last_id,
            _last_entry_id,
        ) = create_tmp_sample_ledger(
            "validator_exit",
            10_000,
            0,
            leader_keypair.pubkey(),
            1000,
            &BlocktreeConfig::default(),
        );

        let validator = Fullnode::new(
            validator_node,
            &Arc::new(validator_keypair),
            &validator_ledger_path,
            VotingKeypair::new(),
            Some(&leader_node.info),
            &FullnodeConfig::default(),
        );
        validator.close().unwrap();
        remove_dir_all(validator_ledger_path).unwrap();
    }

    #[test]
    fn validator_parallel_exit() {
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let mut ledger_paths = vec![];
        let validators: Vec<Fullnode> = (0..2)
            .map(|i| {
                let validator_keypair = Keypair::new();
                let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
                let (
                    _mint_keypair,
                    validator_ledger_path,
                    _tick_height,
                    _last_entry_height,
                    _last_id,
                    _last_entry_id,
                ) = create_tmp_sample_ledger(
                    &format!("validator_parallel_exit_{}", i),
                    10_000,
                    0,
                    leader_keypair.pubkey(),
                    1000,
                    &BlocktreeConfig::default(),
                );
                ledger_paths.push(validator_ledger_path.clone());
                Fullnode::new(
                    validator_node,
                    &Arc::new(validator_keypair),
                    &validator_ledger_path,
                    VotingKeypair::new(),
                    Some(&leader_node.info),
                    &FullnodeConfig::default(),
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

    #[test]
    fn test_leader_to_leader_transition() {
        solana_logger::setup();

        let bootstrap_leader_keypair = Keypair::new();
        let bootstrap_leader_node =
            Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());

        // Once the bootstrap leader hits the second epoch, because there are no other choices in
        // the active set, this leader will remain the leader in the second epoch. In the second
        // epoch, check that the same leader knows to shut down and restart as a leader again.
        let ticks_per_slot = 5;
        let slots_per_epoch = 2;
        let ticks_per_epoch = slots_per_epoch * ticks_per_slot;
        let active_window_length = 10 * ticks_per_epoch;
        let leader_scheduler_config =
            LeaderSchedulerConfig::new(ticks_per_slot, slots_per_epoch, active_window_length);

        let bootstrap_leader_keypair = Arc::new(bootstrap_leader_keypair);
        let voting_keypair = VotingKeypair::new_local(&bootstrap_leader_keypair);
        let mut fullnode_config = FullnodeConfig::default();
        fullnode_config.leader_scheduler_config = leader_scheduler_config;

        let (
            _mint_keypair,
            bootstrap_leader_ledger_path,
            _tick_height,
            _genesis_entry_height,
            _last_id,
            _last_entry_id,
        ) = create_tmp_sample_ledger(
            "test_leader_to_leader_transition",
            10_000,
            1,
            bootstrap_leader_keypair.pubkey(),
            500,
            &fullnode_config.ledger_config(),
        );

        // Start the bootstrap leader
        let bootstrap_leader = Fullnode::new(
            bootstrap_leader_node,
            &bootstrap_leader_keypair,
            &bootstrap_leader_ledger_path,
            voting_keypair,
            None,
            &fullnode_config,
        );

        let (rotation_sender, rotation_receiver) = channel();
        let bootstrap_leader_exit = bootstrap_leader.run(Some(rotation_sender));

        // Wait for the bootstrap leader to transition.  Since there are no other nodes in the
        // cluster it will continue to be the leader
        assert_eq!(
            rotation_receiver.recv().unwrap(),
            (FullnodeReturnType::LeaderToLeaderRotation, ticks_per_slot)
        );
        bootstrap_leader_exit();
    }

    #[test]
    fn test_wrong_role_transition() {
        solana_logger::setup();

        let mut fullnode_config = FullnodeConfig::default();
        let ticks_per_slot = 16;
        let slots_per_epoch = 2;
        fullnode_config.leader_scheduler_config = LeaderSchedulerConfig::new(
            ticks_per_slot,
            slots_per_epoch,
            ticks_per_slot * slots_per_epoch,
        );
        let blocktree_config = &fullnode_config.ledger_config();

        // Create the leader and validator nodes
        let bootstrap_leader_keypair = Arc::new(Keypair::new());
        let validator_keypair = Arc::new(Keypair::new());
        let (bootstrap_leader_node, validator_node, bootstrap_leader_ledger_path, _, _) =
            setup_leader_validator(
                &bootstrap_leader_keypair,
                &validator_keypair,
                0,
                // Generate enough ticks for two epochs to flush the bootstrap_leader's vote at
                // tick_height = 0 from the leader scheduler's active window
                ticks_per_slot * slots_per_epoch * 2,
                "test_wrong_role_transition",
                &blocktree_config,
            );
        let bootstrap_leader_info = bootstrap_leader_node.info.clone();

        let validator_ledger_path = tmp_copy_ledger(
            &bootstrap_leader_ledger_path,
            "test_wrong_role_transition",
            &blocktree_config,
        );

        let ledger_paths = vec![
            bootstrap_leader_ledger_path.clone(),
            validator_ledger_path.clone(),
        ];

        {
            // Test that a node knows to transition to a validator based on parsing the ledger
            let bootstrap_leader = Fullnode::new(
                bootstrap_leader_node,
                &bootstrap_leader_keypair,
                &bootstrap_leader_ledger_path,
                VotingKeypair::new(),
                Some(&bootstrap_leader_info),
                &fullnode_config,
            );

            assert!(!bootstrap_leader.node_services.tpu.is_leader().unwrap());

            // Test that a node knows to transition to a leader based on parsing the ledger
            let validator = Fullnode::new(
                validator_node,
                &validator_keypair,
                &validator_ledger_path,
                VotingKeypair::new(),
                Some(&bootstrap_leader_info),
                &fullnode_config,
            );

            assert!(validator.node_services.tpu.is_leader().unwrap());
            validator.close().expect("Expected leader node to close");
            bootstrap_leader
                .close()
                .expect("Expected validator node to close");
        }
        for path in ledger_paths {
            Blocktree::destroy(&path).expect("Expected successful database destruction");
            let _ignored = remove_dir_all(&path);
        }
    }

    // TODO: Rework this test or TVU (make_consecutive_blobs sends blobs that can't be handled by
    //       the replay_stage)
    #[test]
    #[ignore]
    fn test_validator_to_leader_transition() {
        solana_logger::setup();
        // Make leader and validator node
        let ticks_per_slot = 10;
        let slots_per_epoch = 4;
        let leader_keypair = Arc::new(Keypair::new());
        let validator_keypair = Arc::new(Keypair::new());
        let mut fullnode_config = FullnodeConfig::default();
        fullnode_config.leader_scheduler_config = LeaderSchedulerConfig::new(
            ticks_per_slot,
            slots_per_epoch,
            ticks_per_slot * slots_per_epoch,
        );
        let (leader_node, validator_node, validator_ledger_path, ledger_initial_len, last_id) =
            setup_leader_validator(
                &leader_keypair,
                &validator_keypair,
                0,
                0,
                "test_validator_to_leader_transition",
                &fullnode_config.ledger_config(),
            );

        let leader_id = leader_keypair.pubkey();
        let validator_info = validator_node.info.clone();

        info!("leader: {:?}", leader_id);
        info!("validator: {:?}", validator_info.id);

        let voting_keypair = VotingKeypair::new_local(&validator_keypair);

        // Start the validator
        let validator = Fullnode::new(
            validator_node,
            &validator_keypair,
            &validator_ledger_path,
            voting_keypair,
            Some(&leader_node.info),
            &fullnode_config,
        );

        let blobs_to_send = slots_per_epoch * ticks_per_slot + ticks_per_slot;

        // Send blobs to the validator from our mock leader
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let blob_sockets: Vec<Arc<UdpSocket>> =
                leader_node.sockets.tvu.into_iter().map(Arc::new).collect();
            let t_responder = responder(
                "test_validator_to_leader_transition",
                blob_sockets[0].clone(),
                r_responder,
            );

            let tvu_address = &validator_info.tvu;

            let msgs =
                make_consecutive_blobs(blobs_to_send, ledger_initial_len, last_id, &tvu_address)
                    .into_iter()
                    .rev()
                    .collect();
            s_responder.send(msgs).expect("send");
            t_responder
        };

        info!("waiting for validator to rotate into the leader role");
        let (rotation_sender, rotation_receiver) = channel();
        let validator_exit = validator.run(Some(rotation_sender));
        let rotation = rotation_receiver.recv().unwrap();
        assert_eq!(
            rotation,
            (FullnodeReturnType::ValidatorToLeaderRotation, blobs_to_send)
        );

        // Close the validator so that rocksdb has locks available
        validator_exit();
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::default()));
        let (bank, entry_height, _, _, _) = new_bank_from_ledger(
            &validator_ledger_path,
            "accounts",
            &BlocktreeConfig::default(),
            &leader_scheduler,
        );

        assert!(bank.tick_height() >= leader_scheduler.read().unwrap().ticks_per_epoch);

        assert!(entry_height >= ledger_initial_len);

        // Shut down
        t_responder.join().expect("responder thread join");
        Blocktree::destroy(&validator_ledger_path)
            .expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&validator_ledger_path).unwrap();
    }

    fn setup_leader_validator(
        leader_keypair: &Arc<Keypair>,
        validator_keypair: &Arc<Keypair>,
        num_genesis_ticks: u64,
        num_ending_ticks: u64,
        test_name: &str,
        blocktree_config: &BlocktreeConfig,
    ) -> (Node, Node, String, u64, Hash) {
        // Make a leader identity
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        // Create validator identity
        assert!(num_genesis_ticks <= blocktree_config.ticks_per_slot);
        let (mint_keypair, ledger_path, tick_height, mut entry_height, last_id, last_entry_id) =
            create_tmp_sample_ledger(
                test_name,
                10_000,
                num_genesis_ticks,
                leader_node.info.id,
                500,
                blocktree_config,
            );

        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

        // Write two entries so that the validator is in the active set:
        let (active_set_entries, _) = make_active_set_entries(
            validator_keypair,
            &mint_keypair,
            10,
            1,
            &last_entry_id,
            &last_id,
            num_ending_ticks,
        );

        let blocktree = Blocktree::open_config(&ledger_path, blocktree_config).unwrap();
        let active_set_entries_len = active_set_entries.len() as u64;
        let last_id = active_set_entries.last().unwrap().id;

        blocktree
            .write_entries(
                DEFAULT_SLOT_HEIGHT,
                tick_height,
                entry_height,
                active_set_entries,
            )
            .unwrap();

        entry_height += active_set_entries_len;

        (
            leader_node,
            validator_node,
            ledger_path,
            entry_height,
            last_id,
        )
    }
}
