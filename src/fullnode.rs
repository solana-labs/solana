//! The `fullnode` module hosts all the fullnode microservices.

use crate::bank::Bank;
use crate::blocktree::{Blocktree, BlocktreeConfig};
use crate::cluster_info::{ClusterInfo, Node, NodeInfo};
use crate::counter::Counter;
use crate::genesis_block::GenesisBlock;
use crate::gossip_service::GossipService;
use crate::leader_scheduler::LeaderSchedulerConfig;
use crate::poh_service::PohServiceConfig;
use crate::rpc::JsonRpcService;
use crate::rpc_pubsub::PubSubService;
use crate::service::Service;
use crate::storage_stage::StorageState;
use crate::streamer::BlobSender;
use crate::tpu::{Tpu, TpuRotationReceiver, TpuRotationSender};
use crate::tvu::{Sockets, Tvu};
use crate::voting_keypair::VotingKeypair;
use log::Level;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::{duration_as_ms, timestamp};
use std::net::UdpSocket;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, spawn, Result};
use std::time::{Duration, Instant};

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
    cluster_info: Arc<RwLock<ClusterInfo>>,
    sigverify_disabled: bool,
    tpu_sockets: Vec<UdpSocket>,
    broadcast_socket: UdpSocket,
    node_services: NodeServices,
    rotation_sender: TpuRotationSender,
    rotation_receiver: TpuRotationReceiver,
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
        info!("creating bank...");

        let id = keypair.pubkey();
        assert_eq!(id, node.info.id);

        let (
            bank,
            entry_height,
            last_entry_id,
            blocktree,
            ledger_signal_sender,
            ledger_signal_receiver,
        ) = new_bank_from_ledger(
            ledger_path,
            config.ledger_config(),
            &config.leader_scheduler_config,
        );

        info!("node info: {:?}", node.info);
        info!("node entrypoint_info: {:?}", entrypoint_info_option);
        info!(
            "node local gossip address: {}",
            node.sockets.gossip.local_addr().unwrap()
        );

        let exit = Arc::new(AtomicBool::new(false));
        let bank = Arc::new(bank);
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

        let rpc_pubsub_service = PubSubService::new(
            &bank,
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                node.info.rpc_pubsub.port(),
            ),
        );

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(blocktree.clone()),
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
        let (scheduled_leader, slot_height, max_tpu_tick_height) = {
            let tick_height = bank.tick_height();

            let leader_scheduler = bank.leader_scheduler.read().unwrap();
            let slot = leader_scheduler.tick_height_to_slot(tick_height);
            (
                leader_scheduler
                    .get_leader_for_slot(slot)
                    .expect("Leader not known after processing bank"),
                slot,
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

        // Setup channel for rotation indications
        let (rotation_sender, rotation_receiver) = channel();

        let (tvu, blob_sender) = Tvu::new(
            voting_keypair_option,
            &bank,
            entry_height,
            last_entry_id,
            &cluster_info,
            sockets,
            blocktree.clone(),
            config.storage_rotate_count,
            &rotation_sender,
            &storage_state,
            config.entry_stream.as_ref(),
            ledger_signal_sender,
            ledger_signal_receiver,
        );

        let tpu = Tpu::new(
            &Arc::new(bank.copy_for_tpu()),
            PohServiceConfig::default(),
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
            config.sigverify_disabled,
            max_tpu_tick_height,
            Self::get_consumed_for_slot(&blocktree, slot_height),
            &last_entry_id,
            id,
            &rotation_sender,
            &blob_sender,
            scheduled_leader == id,
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
            rotation_sender,
            rotation_receiver,
            blob_sender,
        }
    }

    fn get_next_leader(&self, tick_height: u64) -> (Pubkey, u64) {
        loop {
            let bank_tick_height = self.bank.tick_height();
            if bank_tick_height >= tick_height {
                break;
            }
            trace!(
                "Waiting for bank tick_height to catch up from {} to {}",
                bank_tick_height,
                tick_height
            );
            sleep(Duration::from_millis(10));
        }

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

        debug!(
            "node {:?} scheduled as leader for ticks [{}, {})",
            scheduled_leader,
            tick_height + 1,
            max_tick_height
        );

        self.cluster_info
            .write()
            .unwrap()
            .set_leader(scheduled_leader);

        (scheduled_leader, max_tick_height)
    }

    fn rotate(&mut self, tick_height: u64) -> FullnodeReturnType {
        trace!("{:?}: rotate at tick_height={}", self.id, tick_height,);
        let was_leader = self.node_services.tpu.is_leader();

        let (scheduled_leader, max_tick_height) = self.get_next_leader(tick_height);
        if scheduled_leader == self.id {
            let transition = if was_leader {
                debug!("{:?} remaining in leader role", self.id);
                FullnodeReturnType::LeaderToLeaderRotation
            } else {
                debug!("{:?} rotating to leader role", self.id);
                FullnodeReturnType::ValidatorToLeaderRotation
            };

            let last_entry_id = self.bank.last_id();

            self.node_services.tpu.switch_to_leader(
                &Arc::new(self.bank.copy_for_tpu()),
                PohServiceConfig::default(),
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
                0,
                &last_entry_id,
                self.id,
                &self.rotation_sender,
                &self.blob_sender,
            );

            transition
        } else {
            debug!("{:?} rotating to validator role", self.id);
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
                    let transition = self.rotate(tick_height);
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

    fn get_consumed_for_slot(blocktree: &Blocktree, slot_index: u64) -> u64 {
        let meta = blocktree.meta(slot_index).expect("Database error");
        if let Some(meta) = meta {
            meta.consumed
        } else {
            0
        }
    }
}

pub fn new_bank_from_ledger(
    ledger_path: &str,
    ledger_config: BlocktreeConfig,
    leader_scheduler_config: &LeaderSchedulerConfig,
) -> (Bank, u64, Hash, Blocktree, SyncSender<bool>, Receiver<bool>) {
    let (blocktree, ledger_signal_sender, ledger_signal_receiver) =
        Blocktree::open_with_config_signal(ledger_path, ledger_config)
            .expect("Expected to successfully open database ledger");
    let genesis_block =
        GenesisBlock::load(ledger_path).expect("Expected to successfully open genesis block");
    let mut bank = Bank::new_with_leader_scheduler_config(&genesis_block, leader_scheduler_config);

    let now = Instant::now();
    let entries = blocktree.read_ledger().expect("opening ledger");
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
        blocktree,
        ledger_signal_sender,
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
    use crate::blob_fetch_stage::BlobFetchStage;
    use crate::blocktree::{create_tmp_sample_ledger, tmp_copy_ledger};
    use crate::entry::make_consecutive_blobs;
    use crate::entry::EntrySlice;
    use crate::gossip_service::{converge, make_listening_node};
    use crate::leader_scheduler::make_active_set_entries;
    use crate::streamer::responder;
    use std::cmp::min;
    use std::fs::remove_dir_all;
    use std::sync::atomic::Ordering;
    use std::thread::sleep;

    #[test]
    fn validator_exit() {
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
        let (_mint_keypair, validator_ledger_path, _last_entry_height, _last_id, _last_entry_id) =
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
                let (
                    _mint_keypair,
                    validator_ledger_path,
                    _last_entry_height,
                    _last_id,
                    _last_entry_id,
                ) = create_tmp_sample_ledger(
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

        let (
            _mint_keypair,
            bootstrap_leader_ledger_path,
            _genesis_entry_height,
            _last_id,
            _last_entry_id,
        ) = create_tmp_sample_ledger(
            "test_leader_to_leader_transition",
            10_000,
            1,
            bootstrap_leader_keypair.pubkey(),
            500,
        );

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
                ticks_per_slot * 4,
                "test_wrong_role_transition",
                ticks_per_slot,
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
        let (leader_node, validator_node, validator_ledger_path, ledger_initial_len, last_id) =
            setup_leader_validator(
                &leader_keypair,
                &validator_keypair,
                0,
                0,
                "test_validator_to_leader_transition",
                ticks_per_slot,
            );

        let leader_id = leader_keypair.pubkey();
        let validator_info = validator_node.info.clone();

        info!("leader: {:?}", leader_id);
        info!("validator: {:?}", validator_info.id);

        // Set the leader scheduler for the validator
        let mut fullnode_config = FullnodeConfig::default();
        fullnode_config.leader_scheduler_config = LeaderSchedulerConfig::new(
            ticks_per_slot,
            slots_per_epoch,
            ticks_per_slot * slots_per_epoch,
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
        let (bank, entry_height, _, _, _, _) = new_bank_from_ledger(
            &validator_ledger_path,
            BlocktreeConfig::default(),
            &LeaderSchedulerConfig::default(),
        );

        assert!(bank.tick_height() >= bank.leader_scheduler.read().unwrap().ticks_per_epoch);

        assert!(entry_height >= ledger_initial_len);

        // Shut down
        t_responder.join().expect("responder thread join");
        Blocktree::destroy(&validator_ledger_path)
            .expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&validator_ledger_path).unwrap();
    }

    #[test]
    fn test_tvu_behind() {
        solana_logger::setup();

        // Make leader node
        let ticks_per_slot = 5;
        let slots_per_epoch = 1;
        let leader_keypair = Arc::new(Keypair::new());
        let validator_keypair = Arc::new(Keypair::new());

        info!("leader: {:?}", leader_keypair.pubkey());
        info!("validator: {:?}", validator_keypair.pubkey());

        let (leader_node, _, leader_ledger_path, _, _) = setup_leader_validator(
            &leader_keypair,
            &validator_keypair,
            1,
            0,
            "test_tvu_behind",
            ticks_per_slot,
        );

        let leader_node_info = leader_node.info.clone();

        // Set the leader scheduler for the validator
        let mut fullnode_config = FullnodeConfig::default();
        fullnode_config.leader_scheduler_config = LeaderSchedulerConfig::new(
            ticks_per_slot,
            slots_per_epoch,
            ticks_per_slot * slots_per_epoch,
        );
        let config = PohServiceConfig::Sleep(Duration::from_millis(200));
        fullnode_config.tick_config = config;

        info!("Start up a listener");
        let blob_receiver_exit = Arc::new(AtomicBool::new(false));
        let (_, _, mut listening_node, _) = make_listening_node(&leader_node.info);
        let (blob_fetch_sender, blob_fetch_receiver) = channel();
        let blob_fetch_stage = BlobFetchStage::new(
            Arc::new(listening_node.sockets.tvu.pop().unwrap()),
            &blob_fetch_sender,
            blob_receiver_exit.clone(),
        );

        let voting_keypair = VotingKeypair::new_local(&leader_keypair);
        info!("Start the bootstrap leader");
        let leader = Fullnode::new(
            leader_node,
            &leader_keypair,
            &leader_ledger_path,
            voting_keypair,
            Some(&leader_node_info),
            &fullnode_config,
        );

        let (rotation_sender, rotation_receiver) = channel();

        info!("Pause the Tvu");
        let pause_tvu = leader.node_services.tvu.get_pause();
        pause_tvu.store(true, Ordering::Relaxed);

        // Wait for convergence
        converge(&leader_node_info, 2);

        info!("Wait for leader -> validator transition");
        let rotation_signal = leader
            .rotation_receiver
            .recv()
            .expect("signal for leader -> validator transition");
        debug!("received rotation signal: {:?}", rotation_signal);
        // Re-send the rotation signal, it'll be received again once the tvu is unpaused
        leader.rotation_sender.send(rotation_signal).expect("send");

        info!("Make sure the tvu bank has not reached the last tick for the slot (the last tick is ticks_per_slot - 1)");
        {
            let w_last_ids = leader.bank.last_ids().write().unwrap();
            assert!(w_last_ids.tick_height < ticks_per_slot - 1);
        }

        // Clear the blobs we've received so far. After this rotation, we should
        // no longer receive blobs from slot 0
        while let Ok(_) = blob_fetch_receiver.try_recv() {}

        let leader_exit = leader.run(Some(rotation_sender));

        // Wait for Tpu bank to progress while the Tvu bank is stuck
        sleep(Duration::from_millis(1000));

        // Tvu bank lock is released here, so tvu should start making progress again and should signal a
        // rotation. After rotation it will still be the slot leader as a new leader schedule has
        // not been computed yet (still in epoch 0). In the next epoch (epoch 1), the node will
        // transition to a validator.
        info!("Unpause the Tvu");
        pause_tvu.store(false, Ordering::Relaxed);
        let expected_rotations = vec![
            (FullnodeReturnType::LeaderToLeaderRotation, ticks_per_slot),
            (
                FullnodeReturnType::LeaderToValidatorRotation,
                2 * ticks_per_slot,
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

        // Make sure that after rotation we don't receive any blobs from slot 0 (make sure
        // broadcast started again at the correct place)
        while let Ok(new_blobs) = blob_fetch_receiver.try_recv() {
            for blob in new_blobs {
                assert_ne!(blob.read().unwrap().slot(), 0);
            }
        }

        // Check the ledger to make sure the PoH chains
        {
            let blocktree = Blocktree::open(&leader_ledger_path).unwrap();
            let entries: Vec<_> = (0..3)
                .flat_map(|slot_height| blocktree.get_slot_entries(slot_height, 0, None).unwrap())
                .collect();

            assert!(entries[1..].verify(&entries[0].id))
        }

        blob_receiver_exit.store(true, Ordering::Relaxed);
        blob_fetch_stage.join().unwrap();

        Blocktree::destroy(&leader_ledger_path).expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&leader_ledger_path).unwrap();
    }

    fn setup_leader_validator(
        leader_keypair: &Arc<Keypair>,
        validator_keypair: &Arc<Keypair>,
        num_genesis_ticks: u64,
        num_ending_ticks: u64,
        test_name: &str,
        ticks_per_block: u64,
    ) -> (Node, Node, String, u64, Hash) {
        // Make a leader identity
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        // Create validator identity
        assert!(num_genesis_ticks <= ticks_per_block);
        let (mint_keypair, ledger_path, genesis_entry_height, last_id, last_entry_id) =
            create_tmp_sample_ledger(
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
            &last_entry_id,
            &last_id,
            num_ending_ticks,
        );

        let non_tick_active_entries_len = active_set_entries.len() - num_ending_ticks as usize;
        let remaining_ticks_in_zeroth_slot = ticks_per_block - num_genesis_ticks;
        let entries_for_zeroth_slot = min(
            active_set_entries.len(),
            non_tick_active_entries_len + remaining_ticks_in_zeroth_slot as usize,
        );
        let entry_chunks: Vec<_> = active_set_entries[entries_for_zeroth_slot..]
            .chunks(ticks_per_block as usize)
            .collect();

        let blocktree = Blocktree::open(&ledger_path).unwrap();

        // Iterate writing slots through 0..entry_chunks.len()
        for i in 0..entry_chunks.len() + 1 {
            let (start_height, entries) = {
                if i == 0 {
                    (
                        genesis_entry_height,
                        &active_set_entries[..entries_for_zeroth_slot],
                    )
                } else {
                    (0, entry_chunks[i - 1])
                }
            };

            blocktree
                .write_entries(i as u64, start_height, entries)
                .unwrap();
        }

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
