//! The `fullnode` module hosts all the fullnode microservices.

use crate::bank::Bank;
use crate::cluster_info::{ClusterInfo, Node, NodeInfo};
use crate::counter::Counter;
use crate::db_ledger::DbLedger;
use crate::genesis_block::GenesisBlock;
use crate::gossip_service::GossipService;
use crate::leader_scheduler::LeaderScheduler;
use crate::rpc::JsonRpcService;
use crate::rpc_pubsub::PubSubService;
use crate::service::Service;
use crate::storage_stage::StorageState;
use crate::tpu::{Tpu, TpuReturnType};
use crate::tvu::{Sockets, Tvu, TvuReturnType};
use crate::vote_signer_proxy::VoteSignerProxy;
use log::Level;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::{duration_as_ms, timestamp};
use std::net::UdpSocket;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread::Result;
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

#[derive(Debug)]
pub enum FullnodeReturnType {
    LeaderToValidatorRotation,
    ValidatorToLeaderRotation,
}

pub struct FullnodeConfig {
    pub sigverify_disabled: bool,
    pub voting_disabled: bool,
    pub entry_stream: Option<String>,
    pub storage_rotate_count: u64,
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
}

impl Fullnode {
    pub fn new(
        mut node: Node,
        keypair: &Arc<Keypair>,
        ledger_path: &str,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        vote_signer: VoteSignerProxy,
        entrypoint_info_option: Option<&NodeInfo>,
        config: FullnodeConfig,
    ) -> Self {
        let id = keypair.pubkey();
        let (genesis_block, db_ledger) = Self::make_db_ledger(ledger_path);
        let (bank, entry_height, last_entry_id) =
            Self::new_bank_from_db_ledger(&genesis_block, &db_ledger, leader_scheduler);

        info!("node info: {:?}", node.info);
        info!("node entrypoint_info: {:?}", entrypoint_info_option);
        info!(
            "node local gossip address: {}",
            node.sockets.gossip.local_addr().unwrap()
        );

        let exit = Arc::new(AtomicBool::new(false));
        let bank = Arc::new(bank);

        node.info.wallclock = timestamp();
        assert_eq!(id, node.info.id);
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
        let (scheduled_leader, _) = bank
            .get_current_leader()
            .expect("Leader not known after processing bank");

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

        let vote_signer_option = if config.voting_disabled {
            None
        } else {
            Some(Arc::new(vote_signer))
        };

        // Setup channels for rotation indications
        let (to_leader_sender, to_leader_receiver) = channel();
        let (to_validator_sender, to_validator_receiver) = channel();

        let tvu = Tvu::new(
            vote_signer_option,
            &bank,
            entry_height,
            last_entry_id,
            &cluster_info,
            sockets,
            db_ledger.clone(),
            config.storage_rotate_count,
            to_leader_sender,
            &storage_state,
            config.entry_stream,
        );
        let max_tick_height = {
            let ls_lock = bank.leader_scheduler.read().unwrap();
            ls_lock.max_height_for_leader(bank.tick_height() + 1)
        };

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
            max_tick_height,
            &last_entry_id,
            id,
            scheduled_leader == id,
            &to_validator_sender,
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
        }
    }

    pub fn leader_to_validator(&mut self) -> Result<()> {
        trace!("leader_to_validator");

        let (scheduled_leader, _) = self.bank.get_current_leader().unwrap();
        self.cluster_info
            .write()
            .unwrap()
            .set_leader(scheduled_leader);
        // In the rare case that the leader exited on a multiple of seed_rotation_interval
        // when the new leader schedule was being generated, and there are no other validators
        // in the active set, then the leader scheduler will pick the same leader again, so
        // check for that
        if scheduled_leader == self.id {
            let (last_entry_id, entry_height) = self.node_services.tvu.get_state();
            self.validator_to_leader(self.bank.tick_height(), entry_height, last_entry_id);
            Ok(())
        } else {
            self.node_services.tpu.switch_to_forwarder(
                self.tpu_sockets
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                    .collect(),
                self.cluster_info.clone(),
            );
            Ok(())
        }
    }

    pub fn validator_to_leader(&mut self, tick_height: u64, entry_height: u64, last_id: Hash) {
        trace!("validator_to_leader");
        self.cluster_info.write().unwrap().set_leader(self.id);

        let max_tick_height = {
            let ls_lock = self.bank.leader_scheduler.read().unwrap();
            ls_lock.max_height_for_leader(tick_height + 1)
        };

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
            &last_id,
            self.id,
            &to_validator_sender,
        )
    }

    pub fn handle_role_transition(&mut self) -> Result<Option<FullnodeReturnType>> {
        loop {
            if self.exit.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let should_be_forwarder = self.role_notifiers.1.try_recv();
            let should_be_leader = self.role_notifiers.0.try_recv();
            match should_be_leader {
                Ok(TvuReturnType::LeaderRotation(tick_height, entry_height, last_entry_id)) => {
                    self.validator_to_leader(tick_height, entry_height, last_entry_id);
                    return Ok(Some(FullnodeReturnType::ValidatorToLeaderRotation));
                }
                _ => match should_be_forwarder {
                    Ok(TpuReturnType::LeaderRotation) => {
                        self.leader_to_validator()?;
                        return Ok(Some(FullnodeReturnType::LeaderToValidatorRotation));
                    }
                    _ => {
                        continue;
                    }
                },
            }
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

    fn new_bank_from_db_ledger(
        genesis_block: &GenesisBlock,
        db_ledger: &DbLedger,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
    ) -> (Bank, u64, Hash) {
        let mut bank = Bank::new(genesis_block);
        leader_scheduler.write().unwrap().bootstrap_leader = genesis_block.bootstrap_leader_id;
        bank.leader_scheduler = leader_scheduler;

        let now = Instant::now();
        let entries = db_ledger.read_ledger().expect("opening ledger");
        info!("processing ledger...");

        let (entry_height, last_entry_id) = bank.process_ledger(entries).expect("process_ledger");
        // entry_height is the network-wide agreed height of the ledger.
        //  initialize it from the input ledger
        info!(
            "processed {} ledger entries in {}ms...",
            entry_height,
            duration_as_ms(&now.elapsed())
        );
        (bank, entry_height, last_entry_id)
    }

    pub fn new_bank_from_ledger(
        ledger_path: &str,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
    ) -> (Bank, u64, Hash) {
        let (genesis_block, db_ledger) = Self::make_db_ledger(ledger_path);
        Self::new_bank_from_db_ledger(&genesis_block, &db_ledger, leader_scheduler)
    }

    pub fn get_leader_scheduler(&self) -> &Arc<RwLock<LeaderScheduler>> {
        &self.bank.leader_scheduler
    }

    fn make_db_ledger(ledger_path: &str) -> (GenesisBlock, Arc<DbLedger>) {
        let db_ledger = Arc::new(
            DbLedger::open(ledger_path).expect("Expected to successfully open database ledger"),
        );

        let genesis_block =
            GenesisBlock::load(ledger_path).expect("Expected to successfully open genesis block");
        (genesis_block, db_ledger)
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
    use crate::cluster_info::Node;
    use crate::db_ledger::*;
    use crate::entry::make_consecutive_blobs;
    use crate::fullnode::{Fullnode, FullnodeReturnType};
    use crate::leader_scheduler::{
        make_active_set_entries, LeaderScheduler, LeaderSchedulerConfig,
    };
    use crate::service::Service;
    use crate::streamer::responder;
    use crate::tpu::TpuReturnType;
    use crate::tvu::TvuReturnType;
    use crate::vote_signer_proxy::VoteSignerProxy;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::cmp;
    use std::fs::remove_dir_all;
    use std::net::UdpSocket;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};

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
            Arc::new(RwLock::new(LeaderScheduler::new(&Default::default()))),
            VoteSignerProxy::new(),
            Some(&leader_node.info),
            Default::default(),
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
                    Arc::new(RwLock::new(LeaderScheduler::new(&Default::default()))),
                    VoteSignerProxy::new(),
                    Some(&leader_node.info),
                    Default::default(),
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
        // Create the leader node information
        let bootstrap_leader_keypair = Keypair::new();
        let bootstrap_leader_node =
            Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());
        let bootstrap_leader_info = bootstrap_leader_node.info.clone();

        // Make a mint and a genesis entries for leader ledger
        let (_mint_keypair, bootstrap_leader_ledger_path, _genesis_entry_height, _last_id) =
            create_tmp_sample_ledger(
                "test_leader_to_leader_transition",
                10_000,
                1,
                bootstrap_leader_keypair.pubkey(),
                500,
            );

        // Create the common leader scheduling configuration
        let num_slots_per_epoch = 3;
        let leader_rotation_interval = 5;
        let seed_rotation_interval = num_slots_per_epoch * leader_rotation_interval;
        let active_window_length = 5;

        // Set the bootstrap height to be bigger than the initial tick height.
        // Once the leader hits the bootstrap height ticks, because there are no other
        // choices in the active set, this leader will remain the leader in the next
        // epoch. In the next epoch, check that the same leader knows to shut down and
        // restart as a leader again.
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            2,
            leader_rotation_interval,
            seed_rotation_interval,
            active_window_length,
        );

        let bootstrap_leader_keypair = Arc::new(bootstrap_leader_keypair);
        let signer = VoteSignerProxy::new_local(&bootstrap_leader_keypair);
        // Start up the leader
        let mut bootstrap_leader = Fullnode::new(
            bootstrap_leader_node,
            &bootstrap_leader_keypair,
            &bootstrap_leader_ledger_path,
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config))),
            signer,
            Some(&bootstrap_leader_info),
            Default::default(),
        );

        // Wait for the leader to transition, ticks should cause the leader to
        // reach the height for leader rotation
        match bootstrap_leader.handle_role_transition().unwrap() {
            Some(FullnodeReturnType::LeaderToValidatorRotation) => (),
            _ => {
                panic!("Expected a leader transition");
            }
        }
        assert!(bootstrap_leader.node_services.tpu.is_leader());
        bootstrap_leader.close().unwrap();
    }

    #[test]
    fn test_wrong_role_transition() {
        solana_logger::setup();

        // Create the leader node information
        let bootstrap_leader_keypair = Arc::new(Keypair::new());
        let bootstrap_leader_node =
            Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());
        let bootstrap_leader_info = bootstrap_leader_node.info.clone();

        // Create the validator node information
        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

        // Make a common mint and a genesis entry for both leader + validator's ledgers
        let (mint_keypair, bootstrap_leader_ledger_path, genesis_entry_height, last_id) =
            create_tmp_sample_ledger(
                "test_wrong_role_transition",
                10_000,
                0,
                bootstrap_leader_keypair.pubkey(),
                500,
            );

        // Write the entries to the ledger that will cause leader rotation
        // after the bootstrap height
        let validator_keypair = Arc::new(validator_keypair);
        let (active_set_entries, _) =
            make_active_set_entries(&validator_keypair, &mint_keypair, &last_id, &last_id, 10);

        {
            let db_ledger = DbLedger::open(&bootstrap_leader_ledger_path).unwrap();
            db_ledger
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    genesis_entry_height,
                    &active_set_entries,
                )
                .unwrap();
        }

        let validator_ledger_path =
            tmp_copy_ledger(&bootstrap_leader_ledger_path, "test_wrong_role_transition");
        let ledger_paths = vec![
            bootstrap_leader_ledger_path.clone(),
            validator_ledger_path.clone(),
        ];

        // Create the common leader scheduling configuration
        let leader_rotation_interval = 3;

        // Set the bootstrap height exactly the current tick height, so that we can
        // test if the bootstrap leader knows to immediately transition to a validator
        // after parsing the ledger during startup
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            1,
            leader_rotation_interval,
            leader_rotation_interval,
            leader_rotation_interval * 10,
        );

        {
            // Test that a node knows to transition to a validator based on parsing the ledger
            let bootstrap_leader = Fullnode::new(
                bootstrap_leader_node,
                &bootstrap_leader_keypair,
                &bootstrap_leader_ledger_path,
                Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config))),
                VoteSignerProxy::new(),
                Some(&bootstrap_leader_info),
                Default::default(),
            );

            assert!(!bootstrap_leader.node_services.tpu.is_leader());

            // Test that a node knows to transition to a leader based on parsing the ledger
            let validator = Fullnode::new(
                validator_node,
                &validator_keypair,
                &validator_ledger_path,
                Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config))),
                VoteSignerProxy::new(),
                Some(&bootstrap_leader_info),
                Default::default(),
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

    #[test]
    fn test_validator_to_leader_transition() {
        // Make a leader identity
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_id = leader_node.info.id;

        // Create validator identity
        let (mint_keypair, validator_ledger_path, genesis_entry_height, last_id) =
            create_tmp_sample_ledger(
                "test_validator_to_leader_transition",
                10_000,
                1,
                leader_id,
                500,
            );

        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
        let validator_info = validator_node.info.clone();

        let validator_keypair = Arc::new(validator_keypair);
        // Write two entries so that the validator is in the active set:
        //
        // 1) Give the validator a nonzero number of tokens
        // Write the bootstrap entries to the ledger that will cause leader rotation
        // after the bootstrap height
        //
        // 2) A vote from the validator
        let (active_set_entries, _) =
            make_active_set_entries(&validator_keypair, &mint_keypair, &last_id, &last_id, 0);
        let active_set_entries_len = active_set_entries.len() as u64;
        let last_id = active_set_entries.last().unwrap().id;

        {
            let db_ledger = DbLedger::open(&validator_ledger_path).unwrap();
            db_ledger
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    genesis_entry_height,
                    &active_set_entries,
                )
                .unwrap();
        }

        let ledger_initial_len = genesis_entry_height + active_set_entries_len;

        // Set the leader scheduler for the validator
        let leader_rotation_interval = 16;
        let num_bootstrap_slots = 2;
        let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            bootstrap_height,
            leader_rotation_interval,
            leader_rotation_interval * 2,
            bootstrap_height,
        );

        let vote_signer = VoteSignerProxy::new_local(&validator_keypair);
        // Start the validator
        let validator = Fullnode::new(
            validator_node,
            &validator_keypair,
            &validator_ledger_path,
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config))),
            vote_signer,
            Some(&leader_node.info),
            Default::default(),
        );

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

            // Send the blobs out of order, in reverse. Also send an extra
            // "extra_blobs" number of blobs to make sure the window stops in the right place.
            let extra_blobs = cmp::max(leader_rotation_interval / 3, 1);
            let total_blobs_to_send = bootstrap_height + extra_blobs;
            let tvu_address = &validator_info.tvu;
            let msgs = make_consecutive_blobs(
                &leader_id,
                total_blobs_to_send,
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

        assert_ne!(validator.bank.get_current_leader().unwrap().0, validator.id);
        loop {
            let should_be_forwarder = validator.role_notifiers.1.try_recv();
            let should_be_leader = validator.role_notifiers.0.try_recv();
            match should_be_leader {
                Ok(TvuReturnType::LeaderRotation(tick_height, entry_height, _)) => {
                    assert_eq!(validator.node_services.tvu.get_state().1, entry_height);
                    assert_eq!(validator.bank.tick_height(), tick_height);
                    assert_eq!(tick_height, bootstrap_height);
                    break;
                }
                _ => match should_be_forwarder {
                    Ok(TpuReturnType::LeaderRotation) => {
                        panic!("shouldn't be rotating to forwarder")
                    }
                    _ => continue,
                },
            }
        }

        //close the validator so that rocksdb has locks available
        validator.close().unwrap();
        let (bank, entry_height, _) = Fullnode::new_bank_from_ledger(
            &validator_ledger_path,
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config))),
        );

        assert!(bank.tick_height() >= bootstrap_height);
        // Only the first genesis entry has num_hashes = 0, every other entry
        // had num_hashes = 1
        assert!(entry_height >= bootstrap_height + active_set_entries_len);

        // Shut down
        t_responder.join().expect("responder thread join");
        DbLedger::destroy(&validator_ledger_path)
            .expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&validator_ledger_path).unwrap();
    }
}
