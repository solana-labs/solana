//! The `fullnode` module hosts all the fullnode microservices.

use crate::bank::Bank;
use crate::cluster_info::{ClusterInfo, Node, NodeInfo};
use crate::counter::Counter;
use crate::db_ledger::DbLedger;
use crate::genesis_block::GenesisBlock;
use crate::gossip_service::GossipService;
use crate::leader_scheduler::LeaderSchedulerConfig;
use crate::rpc::JsonRpcService;
use crate::rpc_pubsub::PubSubService;
use crate::service::Service;
use crate::storage_stage::StorageState;
use crate::streamer::BlobSender;
use crate::tpu::{Tpu, TpuReturnType};
use crate::tvu::{Sockets, Tvu, TvuReturnType};
use crate::voting_keypair::VotingKeypair;
use log::Level;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::{duration_as_ms, timestamp};
use std::net::UdpSocket;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{spawn, Result};
use std::time::Instant;

pub type TvuRotationSender = Sender<TvuReturnType>;
pub type TvuRotationReceiver = Receiver<TvuReturnType>;
pub type TpuRotationSender = Sender<TpuReturnType>;
pub type TpuRotationReceiver = Receiver<TpuReturnType>;

pub struct NodeServices {
    tpu: Tpu,
    tvu: Tvu,
}

impl NodeServices {
    fn new(tpu: Tpu, tvu: Tvu) -> Self {
        NodeServices { tpu, tvu }
    }

    pub fn join(self) -> Result<()> {
        self.tpu.join()?;
        //tvu will never stop unless exit is signaled
        self.tvu.join()?;
        Ok(())
    }

    pub fn is_exited(&self) -> bool {
        self.tpu.is_exited() && self.tvu.is_exited()
    }

    pub fn exit(&self) {
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
            leader_scheduler_config: Default::default(),
        }
    }
}

pub struct Fullnode {
    id: Pubkey,
    exit: Arc<AtomicBool>,
    rpc_service: Option<JsonRpcService>,
    rpc_pubsub_service: Option<PubSubService>,
    gossip_service: GossipService,
    bank: Arc<Bank>,
    cluster_info: Arc<RwLock<ClusterInfo>>,
    sigverify_disabled: bool,
    tpu_sockets: Vec<UdpSocket>,
    broadcast_socket: UdpSocket,
    pub node_services: NodeServices,
    pub role_notifiers: (TvuRotationReceiver, TpuRotationReceiver),
    blob_sender: BlobSender,
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
        let id = keypair.pubkey();
        assert_eq!(id, node.info.id);

        let (
            bank,
            entry_height,
            last_entry_id,
            db_ledger,
            ledger_signal_sender,
            ledger_signal_receiver,
        ) = Self::new_bank_from_ledger(ledger_path, &config.leader_scheduler_config);

        info!("node info: {:?}", node.info);
        info!("node entrypoint_info: {:?}", entrypoint_info_option);
        info!(
            "node local gossip address: {}",
            node.sockets.gossip.local_addr().unwrap()
        );

        let exit = Arc::new(AtomicBool::new(false));
        let bank = Arc::new(bank);
        let db_ledger = Arc::new(db_ledger);

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

        let rpc_pubsub_service = PubSubService::new(
            &bank,
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                node.info.rpc_pubsub.port(),
            ),
        );

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(db_ledger.clone()),
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

        // Get the scheduled leader
        let (scheduled_leader, max_tpu_tick_height) = {
            let tick_height = bank.tick_height();

            let leader_scheduler = bank.leader_scheduler.read().unwrap();
            let slot = leader_scheduler.tick_height_to_slot(tick_height);
            (
                leader_scheduler
                    .get_leader_for_slot(slot)
                    .expect("Leader not known after processing bank"),
                tick_height + leader_scheduler.num_ticks_left_in_slot(tick_height),
            )
        };

        trace!(
            "scheduled_leader: {} until tick_height {}",
            scheduled_leader,
            max_tpu_tick_height
        );
        cluster_info.write().unwrap().set_leader(scheduled_leader);

        // TODO: always start leader and validator, keep leader side switching between tpu
        // forwarder and regular tpu.
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

        // Setup channels for rotation indications
        let (to_leader_sender, to_leader_receiver) = channel();
        let (to_validator_sender, to_validator_receiver) = channel();

        let (tvu, blob_sender) = Tvu::new(
            voting_keypair_option,
            &bank,
            entry_height,
            last_entry_id,
            &cluster_info,
            sockets,
            db_ledger.clone(),
            config.storage_rotate_count,
            to_leader_sender,
            &storage_state,
            config.entry_stream.as_ref(),
            ledger_signal_sender,
            ledger_signal_receiver,
        );
        let tpu = Tpu::new(
            &Arc::new(bank.copy_for_tpu()),
            Default::default(),
            node.sockets
                .tpu
                .iter()
                .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                .collect(),
            node.sockets
                .broadcast
                .try_clone()
                .expect("Failed to clone broadcast socket"),
            cluster_info.clone(),
            entry_height,
            config.sigverify_disabled,
            max_tpu_tick_height,
            &last_entry_id,
            id,
            scheduled_leader == id,
            &to_validator_sender,
            &blob_sender,
        );

        inc_new_counter_info!("fullnode-new", 1);

        Self {
            id,
            cluster_info,
            bank,
            sigverify_disabled: config.sigverify_disabled,
            gossip_service,
            rpc_service: Some(rpc_service),
            rpc_pubsub_service: Some(rpc_pubsub_service),
            node_services: NodeServices::new(tpu, tvu),
            exit,
            tpu_sockets: node.sockets.tpu,
            broadcast_socket: node.sockets.broadcast,
            role_notifiers: (to_leader_receiver, to_validator_receiver),
            blob_sender,
        }
    }

    pub fn leader_to_validator(&mut self, tick_height: u64) -> FullnodeReturnType {
        trace!(
            "leader_to_validator({:?}): tick_height={}",
            self.id,
            tick_height,
        );

        let scheduled_leader = {
            let mut leader_scheduler = self.bank.leader_scheduler.write().unwrap();

            // A transition is only permitted on the final tick of a slot
            assert_eq!(leader_scheduler.num_ticks_left_in_slot(tick_height), 0);
            let first_tick_of_next_slot = tick_height + 1;

            leader_scheduler.update_tick_height(first_tick_of_next_slot, &self.bank);
            let slot = leader_scheduler.tick_height_to_slot(first_tick_of_next_slot);
            leader_scheduler.get_leader_for_slot(slot).unwrap()
        };
        self.cluster_info
            .write()
            .unwrap()
            .set_leader(scheduled_leader);

        if scheduled_leader == self.id {
            debug!("node is still the leader");
            let (last_entry_id, entry_height) = self.node_services.tvu.get_state();
            self.validator_to_leader(tick_height, entry_height, last_entry_id);
            FullnodeReturnType::LeaderToLeaderRotation
        } else {
            debug!("new leader is {}", scheduled_leader);
            self.node_services.tpu.switch_to_forwarder(
                self.tpu_sockets
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                    .collect(),
                self.cluster_info.clone(),
            );
            FullnodeReturnType::LeaderToValidatorRotation
        }
    }

    pub fn validator_to_leader(
        &mut self,
        tick_height: u64,
        entry_height: u64,
        last_entry_id: Hash,
    ) {
        trace!(
            "validator_to_leader({:?}): tick_height={} entry_height={} last_entry_id={}",
            self.id,
            tick_height,
            entry_height,
            last_entry_id,
        );

        let (scheduled_leader, max_tick_height) = {
            let mut leader_scheduler = self.bank.leader_scheduler.write().unwrap();

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
        assert_eq!(scheduled_leader, self.id, "node is not the leader");
        self.cluster_info.write().unwrap().set_leader(self.id);

        debug!(
            "node scheduled as leader for ticks [{}, {})",
            tick_height + 1,
            max_tick_height
        );

        let (to_validator_sender, to_validator_receiver) = channel();
        self.role_notifiers.1 = to_validator_receiver;
        self.node_services.tpu.switch_to_leader(
            &Arc::new(self.bank.copy_for_tpu()),
            Default::default(),
            self.tpu_sockets
                .iter()
                .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                .collect(),
            self.broadcast_socket
                .try_clone()
                .expect("Failed to clone broadcast socket"),
            self.cluster_info.clone(),
            self.sigverify_disabled,
            max_tick_height,
            entry_height,
            &last_entry_id,
            self.id,
            &to_validator_sender,
            &self.blob_sender,
        )
    }

    fn handle_role_transition(&mut self) -> Option<(FullnodeReturnType, u64)> {
        if self.node_services.tpu.is_leader() {
            let should_be_forwarder = self.role_notifiers.1.recv();
            match should_be_forwarder {
                Ok(TpuReturnType::LeaderRotation(tick_height)) => {
                    Some((self.leader_to_validator(tick_height), tick_height + 1))
                }
                Err(_) => None,
            }
        } else {
            let should_be_leader = self.role_notifiers.0.recv();
            match should_be_leader {
                Ok(TvuReturnType::LeaderRotation(tick_height, entry_height, last_entry_id)) => {
                    self.validator_to_leader(tick_height, entry_height, last_entry_id);
                    Some((
                        FullnodeReturnType::ValidatorToLeaderRotation,
                        tick_height + 1,
                    ))
                }
                Err(_) => None,
            }
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
        spawn(move || loop {
            if self.exit.load(Ordering::Relaxed) {
                debug!("node shutdown requested");
                self.close().expect("Unable to close node");
                sender.send(true).expect("Unable to signal exit");
                break;
            }
            let status = self.handle_role_transition();
            match status {
                None => {
                    debug!("node shutdown requested");
                    self.close().expect("Unable to close node");
                    sender.send(true).expect("Unable to signal exit");
                    break;
                }
                Some(transition) => {
                    debug!("role_transition complete: {:?}", transition);
                    if let Some(ref rotation_notifier) = rotation_notifier {
                        rotation_notifier.send(transition).unwrap();
                    }
                }
            };
        });
        move || {
            exit.store(true, Ordering::Relaxed);
            receiver.recv().unwrap();
            debug!("node shutdown complete");
        }
    }

    // Used for notifying many nodes in parallel to exit
    pub fn exit(&self) {
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

    pub fn new_bank_from_ledger(
        ledger_path: &str,
        leader_scheduler_config: &LeaderSchedulerConfig,
    ) -> (Bank, u64, Hash, DbLedger, SyncSender<bool>, Receiver<bool>) {
        let (db_ledger, ledger_signal_sender, ledger_signal_receiver) =
            DbLedger::open_with_signal(ledger_path)
                .expect("Expected to successfully open database ledger");
        let genesis_block =
            GenesisBlock::load(ledger_path).expect("Expected to successfully open genesis block");
        let mut bank =
            Bank::new_with_leader_scheduler_config(&genesis_block, leader_scheduler_config);

        let now = Instant::now();
        let entries = db_ledger.read_ledger().expect("opening ledger");
        info!("processing ledger...");
        let (entry_height, last_entry_id) = bank.process_ledger(entries).expect("process_ledger");
        info!(
            "processed {} ledger entries in {}ms, tick_height={}...",
            entry_height,
            duration_as_ms(&now.elapsed()),
            bank.tick_height()
        );

        (
            bank,
            entry_height,
            last_entry_id,
            db_ledger,
            ledger_signal_sender,
            ledger_signal_receiver,
        )
    }
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
    use crate::cluster_info::Node;
    use crate::db_ledger::*;
    use crate::entry::make_consecutive_blobs;
    use crate::leader_scheduler::{make_active_set_entries, LeaderSchedulerConfig};
    use crate::service::Service;
    use crate::streamer::responder;
    use crate::voting_keypair::VotingKeypair;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::net::UdpSocket;
    use std::sync::mpsc::channel;
    use std::sync::Arc;

    #[test]
    fn validator_exit() {
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
        let (_, validator_ledger_path, _, _) =
            create_tmp_sample_ledger("validator_exit", 10_000, 0, leader_keypair.pubkey(), 1000);

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
                let (_, validator_ledger_path, _, _) = create_tmp_sample_ledger(
                    &format!("validator_parallel_exit_{}", i),
                    10_000,
                    0,
                    leader_keypair.pubkey(),
                    1000,
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

        let (_mint_keypair, bootstrap_leader_ledger_path, _genesis_entry_height, _last_id) =
            create_tmp_sample_ledger(
                "test_leader_to_leader_transition",
                10_000,
                1,
                bootstrap_leader_keypair.pubkey(),
                500,
            );

        // Once the bootstrap leader hits the second epoch, because there are no other choices in
        // the active set, this leader will remain the leader in the second epoch. In the second
        // epoch, check that the same leader knows to shut down and restart as a leader again.
        let leader_rotation_interval = 5;
        let num_slots_per_epoch = 2;
        let seed_rotation_interval = num_slots_per_epoch * leader_rotation_interval;
        let active_window_length = 10 * seed_rotation_interval;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_rotation_interval,
            seed_rotation_interval,
            active_window_length,
        );

        let bootstrap_leader_keypair = Arc::new(bootstrap_leader_keypair);
        let voting_keypair = VotingKeypair::new_local(&bootstrap_leader_keypair);
        // Start the bootstrap leader
        let mut fullnode_config = FullnodeConfig::default();
        fullnode_config.leader_scheduler_config = leader_scheduler_config;
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
            (
                FullnodeReturnType::LeaderToLeaderRotation,
                leader_rotation_interval
            )
        );
        bootstrap_leader_exit();
    }

    #[test]
    fn test_wrong_role_transition() {
        solana_logger::setup();

        let mut fullnode_config = FullnodeConfig::default();
        let leader_rotation_interval = 16;
        fullnode_config.leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_rotation_interval,
            leader_rotation_interval * 2,
            leader_rotation_interval * 2,
        );

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
                leader_rotation_interval * 4,
                "test_wrong_role_transition",
            );
        let bootstrap_leader_info = bootstrap_leader_node.info.clone();

        let validator_ledger_path =
            tmp_copy_ledger(&bootstrap_leader_ledger_path, "test_wrong_role_transition");

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

            assert!(!bootstrap_leader.node_services.tpu.is_leader());

            // Test that a node knows to transition to a leader based on parsing the ledger
            let validator = Fullnode::new(
                validator_node,
                &validator_keypair,
                &validator_ledger_path,
                VotingKeypair::new(),
                Some(&bootstrap_leader_info),
                &fullnode_config,
            );

            assert!(validator.node_services.tpu.is_leader());
            validator.close().expect("Expected leader node to close");
            bootstrap_leader
                .close()
                .expect("Expected validator node to close");
        }
        for path in ledger_paths {
            DbLedger::destroy(&path).expect("Expected successful database destruction");
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
        let leader_keypair = Arc::new(Keypair::new());
        let validator_keypair = Arc::new(Keypair::new());
        let (leader_node, validator_node, validator_ledger_path, ledger_initial_len, last_id) =
            setup_leader_validator(
                &leader_keypair,
                &validator_keypair,
                0,
                0,
                "test_validator_to_leader_transition",
            );

        let leader_id = leader_keypair.pubkey();
        let validator_info = validator_node.info.clone();

        info!("leader: {:?}", leader_id);
        info!("validator: {:?}", validator_info.id);

        // Set the leader scheduler for the validator
        let leader_rotation_interval = 10;

        let mut fullnode_config = FullnodeConfig::default();
        fullnode_config.leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_rotation_interval,
            leader_rotation_interval * 4,
            leader_rotation_interval * 4,
        );

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

        let blobs_to_send = fullnode_config
            .leader_scheduler_config
            .seed_rotation_interval
            + fullnode_config
                .leader_scheduler_config
                .leader_rotation_interval;

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

            let msgs = make_consecutive_blobs(
                &leader_id,
                blobs_to_send,
                ledger_initial_len,
                last_id,
                &tvu_address,
            )
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
        let (bank, entry_height, _, _, _, _) = Fullnode::new_bank_from_ledger(
            &validator_ledger_path,
            &LeaderSchedulerConfig::default(),
        );

        assert!(
            bank.tick_height()
                >= fullnode_config
                    .leader_scheduler_config
                    .seed_rotation_interval
        );

        assert!(entry_height >= ledger_initial_len);

        // Shut down
        t_responder.join().expect("responder thread join");
        DbLedger::destroy(&validator_ledger_path)
            .expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&validator_ledger_path).unwrap();
    }

    #[test]
    #[ignore] // TODO: Make this test less hacky
    fn test_tvu_behind() {
        solana_logger::setup();

        // Make leader node
        let leader_keypair = Arc::new(Keypair::new());
        let validator_keypair = Arc::new(Keypair::new());

        info!("leader: {:?}", leader_keypair.pubkey());
        info!("validator: {:?}", validator_keypair.pubkey());

        let (leader_node, _, leader_ledger_path, _, _) =
            setup_leader_validator(&leader_keypair, &validator_keypair, 1, 0, "test_tvu_behind");

        let leader_node_info = leader_node.info.clone();

        // Set the leader scheduler for the validator
        let leader_rotation_interval = 5;

        let mut fullnode_config = FullnodeConfig::default();
        fullnode_config.leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_rotation_interval,
            leader_rotation_interval * 2,
            leader_rotation_interval * 2,
        );

        let voting_keypair = VotingKeypair::new_local(&leader_keypair);
        info!("Start the bootstrap leader");
        let mut leader = Fullnode::new(
            leader_node,
            &leader_keypair,
            &leader_ledger_path,
            voting_keypair,
            Some(&leader_node_info),
            &fullnode_config,
        );

        info!("Hold Tvu bank lock to prevent tvu from making progress");
        {
            let w_last_ids = leader.bank.last_ids().write().unwrap();

            info!("Wait for leader -> validator transition");
            let signal = leader
                .role_notifiers
                .1
                .recv()
                .expect("signal for leader -> validator transition");
            let (rn_sender, rn_receiver) = channel();
            rn_sender.send(signal).expect("send");
            leader.role_notifiers = (leader.role_notifiers.0, rn_receiver);

            info!("Make sure the tvu bank is behind");
            assert_eq!(w_last_ids.tick_height, 2);
        }

        // Release tvu bank lock, tvu should start making progress again and should signal a
        // rotate.   After rotation it will still be the slot leader as a new leader schedule has
        // not been computed yet (still in epoch 0)
        info!("Release tvu bank lock");
        let (rotation_sender, rotation_receiver) = channel();
        let leader_exit = leader.run(Some(rotation_sender));
        let expected_rotations = vec![
            (
                FullnodeReturnType::LeaderToLeaderRotation,
                leader_rotation_interval,
            ),
            (
                FullnodeReturnType::LeaderToLeaderRotation,
                2 * leader_rotation_interval,
            ),
            (
                FullnodeReturnType::LeaderToValidatorRotation,
                3 * leader_rotation_interval,
            ),
        ];

        for expected_rotation in expected_rotations {
            loop {
                let transition = rotation_receiver.recv().unwrap();
                info!("leader transition: {:?}", transition);
                assert_eq!(expected_rotation, transition);
                break;
            }
        }

        info!("Shut down");
        leader_exit();
        DbLedger::destroy(&leader_ledger_path).expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&leader_ledger_path).unwrap();
    }

    fn setup_leader_validator(
        leader_keypair: &Arc<Keypair>,
        validator_keypair: &Arc<Keypair>,
        num_genesis_ticks: u64,
        num_ending_ticks: u64,
        test_name: &str,
    ) -> (Node, Node, String, u64, Hash) {
        // Make a leader identity
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        // Create validator identity
        let (mint_keypair, ledger_path, genesis_entry_height, last_id) = create_tmp_sample_ledger(
            test_name,
            10_000,
            num_genesis_ticks,
            leader_node.info.id,
            500,
        );

        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

        // Write two entries so that the validator is in the active set:
        //
        // 1) Give the validator a nonzero number of tokens
        // Write the bootstrap entries to the ledger that will cause leader rotation
        // after the bootstrap height
        //
        // 2) A vote from the validator
        let (active_set_entries, _) = make_active_set_entries(
            validator_keypair,
            &mint_keypair,
            10,
            1,
            &last_id,
            &last_id,
            num_ending_ticks,
        );

        let db_ledger = DbLedger::open(&ledger_path).unwrap();
        db_ledger
            .write_entries(
                DEFAULT_SLOT_HEIGHT,
                genesis_entry_height,
                &active_set_entries,
            )
            .unwrap();

        let entry_height = genesis_entry_height + active_set_entries.len() as u64;
        (
            leader_node,
            validator_node,
            ledger_path,
            entry_height,
            active_set_entries.last().unwrap().id,
        )
    }
}
