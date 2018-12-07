//! The `fullnode` module hosts all the fullnode microservices.

use crate::bank::Bank;
use crate::broadcast_service::BroadcastService;
use crate::cluster_info::{ClusterInfo, Node, NodeInfo};
use crate::db_ledger::{write_entries_to_ledger, DbLedger};
use crate::gossip_service::GossipService;
use crate::leader_scheduler::LeaderScheduler;
use crate::ledger::read_ledger;
use crate::rpc::JsonRpcService;
use crate::rpc_pubsub::PubSubService;
use crate::service::Service;
use crate::tpu::{Tpu, TpuReturnType};
use crate::tpu_forwarder::TpuForwarder;
use crate::tvu::{Tvu, TvuReturnType};
use crate::window::{new_window, SharedWindow};
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::timestamp;
use std::net::UdpSocket;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::Result;
use untrusted::Input;

pub enum NodeRole {
    Leader(LeaderServices),
    Validator(ValidatorServices),
}

pub struct LeaderServices {
    tpu: Tpu,
    broadcast_service: BroadcastService,
}

impl LeaderServices {
    fn new(tpu: Tpu, broadcast_service: BroadcastService) -> Self {
        LeaderServices {
            tpu,
            broadcast_service,
        }
    }

    pub fn join(self) -> Result<Option<TpuReturnType>> {
        self.broadcast_service.join()?;
        self.tpu.join()
    }

    pub fn is_exited(&self) -> bool {
        self.tpu.is_exited()
    }

    pub fn exit(&self) {
        self.tpu.exit();
    }
}

pub struct ValidatorServices {
    tvu: Tvu,
    tpu_forwarder: TpuForwarder,
}

impl ValidatorServices {
    fn new(tvu: Tvu, tpu_forwarder: TpuForwarder) -> Self {
        Self { tvu, tpu_forwarder }
    }

    pub fn join(self) -> Result<Option<TvuReturnType>> {
        let ret = self.tvu.join(); // TVU calls the shots, we wait for it to shut down
        self.tpu_forwarder.join()?;
        ret
    }

    pub fn is_exited(&self) -> bool {
        self.tvu.is_exited()
    }

    pub fn exit(&self) {
        self.tvu.exit()
    }
}

#[derive(Debug)]
pub enum FullnodeReturnType {
    LeaderToValidatorRotation,
    ValidatorToLeaderRotation,
}

pub struct Fullnode {
    pub node_role: Option<NodeRole>,
    keypair: Arc<Keypair>,
    vote_account_keypair: Arc<Keypair>,
    exit: Arc<AtomicBool>,
    rpc_service: Option<JsonRpcService>,
    rpc_pubsub_service: Option<PubSubService>,
    gossip_service: GossipService,
    bank: Arc<Bank>,
    cluster_info: Arc<RwLock<ClusterInfo>>,
    ledger_path: String,
    sigverify_disabled: bool,
    shared_window: SharedWindow,
    tvu_sockets: Vec<UdpSocket>,
    repair_socket: UdpSocket,
    retransmit_socket: UdpSocket,
    tpu_sockets: Vec<UdpSocket>,
    broadcast_socket: UdpSocket,
    rpc_addr: SocketAddr,
    rpc_pubsub_addr: SocketAddr,
    db_ledger: Arc<RwLock<DbLedger>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
/// Fullnode configuration to be stored in file
pub struct Config {
    pub node_info: NodeInfo,
    pkcs8: Vec<u8>,
    vote_account_pkcs8: Vec<u8>,
}

impl Config {
    pub fn new(bind_addr: &SocketAddr, pkcs8: Vec<u8>, vote_account_pkcs8: Vec<u8>) -> Self {
        let keypair =
            Keypair::from_pkcs8(Input::from(&pkcs8)).expect("from_pkcs8 in fullnode::Config new");
        let pubkey = keypair.pubkey();
        let node_info = NodeInfo::new_with_pubkey_socketaddr(pubkey, bind_addr);
        Config {
            node_info,
            pkcs8,
            vote_account_pkcs8,
        }
    }
    pub fn keypair(&self) -> Keypair {
        Keypair::from_pkcs8(Input::from(&self.pkcs8))
            .expect("from_pkcs8 in fullnode::Config keypair")
    }
    pub fn vote_account_keypair(&self) -> Keypair {
        Keypair::from_pkcs8(Input::from(&self.vote_account_pkcs8))
            .expect("from_pkcs8 in fullnode::Config vote_account_keypair")
    }
}

impl Fullnode {
    pub fn new(
        node: Node,
        ledger_path: &str,
        keypair: Arc<Keypair>,
        vote_account_keypair: Arc<Keypair>,
        leader_addr: Option<SocketAddr>,
        sigverify_disabled: bool,
        leader_scheduler: LeaderScheduler,
        rpc_port: Option<u16>,
    ) -> Self {
        let leader_scheduler = Arc::new(RwLock::new(leader_scheduler));

        info!("creating bank...");

        let (bank, entry_height, last_entry_id) =
            Self::new_bank_from_ledger(ledger_path, leader_scheduler);

        info!("creating networking stack...");
        let local_gossip_addr = node.sockets.gossip.local_addr().unwrap();

        info!(
            "starting... local gossip address: {} (advertising {})",
            local_gossip_addr, node.info.gossip
        );
        let mut rpc_addr = node.info.rpc;
        if let Some(port) = rpc_port {
            rpc_addr.set_port(port);
        }

        let leader_info = leader_addr.map(|i| NodeInfo::new_entry_point(&i));
        let server = Self::new_with_bank(
            keypair,
            vote_account_keypair,
            bank,
            entry_height,
            &last_entry_id,
            node,
            leader_info.as_ref(),
            ledger_path,
            sigverify_disabled,
            rpc_port,
        );

        match leader_addr {
            Some(leader_addr) => {
                info!(
                    "validator ready... rpc address: {}, connected to: {}",
                    rpc_addr, leader_addr
                );
            }
            None => {
                info!("leader ready... rpc address: {}", rpc_addr);
            }
        }

        server
    }

    /// Create a fullnode instance acting as a leader or validator.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_bank(
        keypair: Arc<Keypair>,
        vote_account_keypair: Arc<Keypair>,
        bank: Bank,
        entry_height: u64,
        last_entry_id: &Hash,
        mut node: Node,
        bootstrap_leader_info_option: Option<&NodeInfo>,
        ledger_path: &str,
        sigverify_disabled: bool,
        rpc_port: Option<u16>,
    ) -> Self {
        let mut rpc_addr = node.info.rpc;
        let mut rpc_pubsub_addr = node.info.rpc_pubsub;
        // Use custom RPC port, if provided (`Some(port)`)
        // RPC port may be any valid open port on the node
        // If rpc_port == `None`, node will listen on the ports set in NodeInfo
        if let Some(port) = rpc_port {
            rpc_addr.set_port(port);
            node.info.rpc = rpc_addr;
            rpc_pubsub_addr.set_port(port + 1);
            node.info.rpc_pubsub = rpc_pubsub_addr;
        }

        let exit = Arc::new(AtomicBool::new(false));
        let bank = Arc::new(bank);

        let window = new_window(32 * 1024);
        let shared_window = Arc::new(RwLock::new(window));
        node.info.wallclock = timestamp();
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_keypair(
            node.info,
            keypair.clone(),
        )));

        let (rpc_service, rpc_pubsub_service) =
            Self::startup_rpc_services(rpc_addr, rpc_pubsub_addr, &bank, &cluster_info);

        let gossip_service = GossipService::new(
            &cluster_info,
            shared_window.clone(),
            Some(ledger_path),
            node.sockets.gossip,
            exit.clone(),
        );

        // Insert the bootstrap leader info, should only be None if this node
        // is the bootstrap leader
        if let Some(bootstrap_leader_info) = bootstrap_leader_info_option {
            cluster_info
                .write()
                .unwrap()
                .insert_info(bootstrap_leader_info.clone());
        }

        // Get the scheduled leader
        let (scheduled_leader, leader_slot) = bank
            .get_current_leader()
            .expect("Leader not known after processing bank");

        cluster_info.write().unwrap().set_leader(scheduled_leader);

        // Create the RocksDb ledger
        let db_ledger = Self::make_db_ledger(ledger_path);

        let node_role = if scheduled_leader != keypair.pubkey() {
            // Start in validator mode.
            let tvu = Tvu::new(
                keypair.clone(),
                vote_account_keypair.clone(),
                &bank,
                entry_height,
                *last_entry_id,
                cluster_info.clone(),
                node.sockets
                    .tvu
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TVU sockets"))
                    .collect(),
                node.sockets
                    .repair
                    .try_clone()
                    .expect("Failed to clone repair socket"),
                node.sockets
                    .retransmit
                    .try_clone()
                    .expect("Failed to clone retransmit socket"),
                Some(ledger_path),
                db_ledger.clone(),
            );
            let tpu_forwarder = TpuForwarder::new(
                node.sockets
                    .tpu
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                    .collect(),
                cluster_info.clone(),
            );

            let validator_state = ValidatorServices::new(tvu, tpu_forwarder);
            Some(NodeRole::Validator(validator_state))
        } else {
            let max_tick_height = {
                let ls_lock = bank.leader_scheduler.read().unwrap();
                ls_lock.max_height_for_leader(bank.tick_height() + 1)
            };

            // Start in leader mode.
            let (tpu, entry_receiver, tpu_exit) = Tpu::new(
                &bank,
                Default::default(),
                node.sockets
                    .tpu
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                    .collect(),
                ledger_path,
                sigverify_disabled,
                max_tick_height,
                last_entry_id,
                scheduled_leader,
            );

            let broadcast_service = BroadcastService::new(
                node.sockets
                    .broadcast
                    .try_clone()
                    .expect("Failed to clone broadcast socket"),
                cluster_info.clone(),
                shared_window.clone(),
                entry_height,
                leader_slot,
                entry_receiver,
                max_tick_height,
                bank.tick_height(),
                tpu_exit,
            );
            let leader_state = LeaderServices::new(tpu, broadcast_service);
            Some(NodeRole::Leader(leader_state))
        };

        Fullnode {
            keypair,
            vote_account_keypair,
            cluster_info,
            shared_window,
            bank,
            sigverify_disabled,
            gossip_service,
            rpc_service: Some(rpc_service),
            rpc_pubsub_service: Some(rpc_pubsub_service),
            node_role,
            ledger_path: ledger_path.to_owned(),
            exit,
            tvu_sockets: node.sockets.tvu,
            repair_socket: node.sockets.repair,
            retransmit_socket: node.sockets.retransmit,
            tpu_sockets: node.sockets.tpu,
            broadcast_socket: node.sockets.broadcast,
            rpc_addr,
            rpc_pubsub_addr,
            db_ledger,
        }
    }

    fn leader_to_validator(&mut self) -> Result<()> {
        // Close down any services that could have a reference to the bank
        if self.rpc_service.is_some() {
            let old_rpc_service = self.rpc_service.take().unwrap();
            old_rpc_service.close()?;
        }

        if self.rpc_pubsub_service.is_some() {
            let old_rpc_pubsub_service = self.rpc_pubsub_service.take().unwrap();
            old_rpc_pubsub_service.close()?;
        }

        // Correctness check: Ensure that references to the bank and leader scheduler are no
        // longer held by any running thread
        let mut new_leader_scheduler = self.bank.leader_scheduler.read().unwrap().clone();

        // Clear the leader scheduler
        new_leader_scheduler.reset();

        let (new_bank, scheduled_leader, entry_height, last_entry_id) = {
            // TODO: We can avoid building the bank again once RecordStage is
            // integrated with BankingStage
            let (new_bank, entry_height, last_id) = Self::new_bank_from_ledger(
                &self.ledger_path,
                Arc::new(RwLock::new(new_leader_scheduler)),
            );

            let new_bank = Arc::new(new_bank);
            let (scheduled_leader, _) = new_bank
                .get_current_leader()
                .expect("Scheduled leader should exist after rebuilding bank");

            (new_bank, scheduled_leader, entry_height, last_id)
        };

        self.cluster_info
            .write()
            .unwrap()
            .set_leader(scheduled_leader);

        // Spin up new versions of all the services that relied on the bank, passing in the
        // new bank
        let (rpc_service, rpc_pubsub_service) = Self::startup_rpc_services(
            self.rpc_addr,
            self.rpc_pubsub_addr,
            &new_bank,
            &self.cluster_info,
        );
        self.rpc_service = Some(rpc_service);
        self.rpc_pubsub_service = Some(rpc_pubsub_service);
        self.bank = new_bank;

        // In the rare case that the leader exited on a multiple of seed_rotation_interval
        // when the new leader schedule was being generated, and there are no other validators
        // in the active set, then the leader scheduler will pick the same leader again, so
        // check for that
        if scheduled_leader == self.keypair.pubkey() {
            let tick_height = self.bank.tick_height();
            self.validator_to_leader(tick_height, entry_height, last_entry_id);
            Ok(())
        } else {
            let tvu = Tvu::new(
                self.keypair.clone(),
                self.vote_account_keypair.clone(),
                &self.bank,
                entry_height,
                last_entry_id,
                self.cluster_info.clone(),
                self.tvu_sockets
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TVU sockets"))
                    .collect(),
                self.repair_socket
                    .try_clone()
                    .expect("Failed to clone repair socket"),
                self.retransmit_socket
                    .try_clone()
                    .expect("Failed to clone retransmit socket"),
                Some(&self.ledger_path),
                self.db_ledger.clone(),
            );
            let tpu_forwarder = TpuForwarder::new(
                self.tpu_sockets
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                    .collect(),
                self.cluster_info.clone(),
            );

            let validator_state = ValidatorServices::new(tvu, tpu_forwarder);
            self.node_role = Some(NodeRole::Validator(validator_state));
            Ok(())
        }
    }

    fn validator_to_leader(&mut self, tick_height: u64, entry_height: u64, last_id: Hash) {
        self.cluster_info
            .write()
            .unwrap()
            .set_leader(self.keypair.pubkey());

        let max_tick_height = {
            let ls_lock = self.bank.leader_scheduler.read().unwrap();
            ls_lock.max_height_for_leader(tick_height + 1)
        };

        let (tpu, blob_receiver, tpu_exit) = Tpu::new(
            &self.bank,
            Default::default(),
            self.tpu_sockets
                .iter()
                .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                .collect(),
            &self.ledger_path,
            self.sigverify_disabled,
            max_tick_height,
            // We pass the last_entry_id from the replay stage because we can't trust that
            // the window didn't overwrite the slot at for the last entry that the replay stage
            // processed. We also want to avoid reading processing the ledger for the last id.
            &last_id,
            self.keypair.pubkey(),
        );

        let broadcast_service = BroadcastService::new(
            self.broadcast_socket
                .try_clone()
                .expect("Failed to clone broadcast socket"),
            self.cluster_info.clone(),
            self.shared_window.clone(),
            entry_height,
            0, // TODO: get real leader slot from leader_scheduler
            blob_receiver,
            max_tick_height,
            tick_height,
            tpu_exit,
        );
        let leader_state = LeaderServices::new(tpu, broadcast_service);
        self.node_role = Some(NodeRole::Leader(leader_state));
    }

    pub fn check_role_exited(&self) -> bool {
        match self.node_role {
            Some(NodeRole::Leader(ref leader_services)) => leader_services.is_exited(),
            Some(NodeRole::Validator(ref validator_services)) => validator_services.is_exited(),
            None => false,
        }
    }

    pub fn handle_role_transition(&mut self) -> Result<Option<FullnodeReturnType>> {
        let node_role = self.node_role.take();
        match node_role {
            Some(NodeRole::Leader(leader_services)) => match leader_services.join()? {
                Some(TpuReturnType::LeaderRotation) => {
                    self.leader_to_validator()?;
                    Ok(Some(FullnodeReturnType::LeaderToValidatorRotation))
                }
                _ => Ok(None),
            },
            Some(NodeRole::Validator(validator_services)) => match validator_services.join()? {
                Some(TvuReturnType::LeaderRotation(tick_height, entry_height, last_entry_id)) => {
                    //TODO: Fix this to return actual poh height.
                    self.validator_to_leader(tick_height, entry_height, last_entry_id);
                    Ok(Some(FullnodeReturnType::ValidatorToLeaderRotation))
                }
                _ => Ok(None),
            },
            None => Ok(None),
        }
    }

    //used for notifying many nodes in parallel to exit
    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
        if let Some(ref rpc_service) = self.rpc_service {
            rpc_service.exit();
        }
        if let Some(ref rpc_pubsub_service) = self.rpc_pubsub_service {
            rpc_pubsub_service.exit();
        }
        match self.node_role {
            Some(NodeRole::Leader(ref leader_services)) => leader_services.exit(),
            Some(NodeRole::Validator(ref validator_services)) => validator_services.exit(),
            _ => (),
        }
    }

    pub fn close(self) -> Result<(Option<FullnodeReturnType>)> {
        self.exit();
        self.join()
    }

    pub fn new_bank_from_ledger(
        ledger_path: &str,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
    ) -> (Bank, u64, Hash) {
        let mut bank = Bank::new_with_builtin_programs();
        bank.leader_scheduler = leader_scheduler;
        let entries = read_ledger(ledger_path, true).expect("opening ledger");
        let entries = entries
            .map(|e| e.unwrap_or_else(|err| panic!("failed to parse entry. error: {}", err)));
        info!("processing ledger...");

        let (entry_height, last_entry_id) = bank.process_ledger(entries).expect("process_ledger");
        // entry_height is the network-wide agreed height of the ledger.
        //  initialize it from the input ledger
        info!("processed {} ledger...", entry_height);
        (bank, entry_height, last_entry_id)
    }

    pub fn get_leader_scheduler(&self) -> &Arc<RwLock<LeaderScheduler>> {
        &self.bank.leader_scheduler
    }

    fn startup_rpc_services(
        rpc_addr: SocketAddr,
        rpc_pubsub_addr: SocketAddr,
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) -> (JsonRpcService, PubSubService) {
        let rpc_port = rpc_addr.port();
        let rpc_pubsub_port = rpc_pubsub_addr.port();
        // TODO: The RPC service assumes that there is a drone running on the leader
        // Drone location/id will need to be handled a different way as soon as leader rotation begins
        (
            JsonRpcService::new(
                bank,
                cluster_info,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), rpc_port),
            ),
            PubSubService::new(
                bank,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), rpc_pubsub_port),
            ),
        )
    }

    fn make_db_ledger(ledger_path: &str) -> Arc<RwLock<DbLedger>> {
        // Destroy any existing instances of the RocksDb ledger
        DbLedger::destroy(&ledger_path).expect("Expected successful database destruction");
        let ledger_entries = read_ledger(ledger_path, true)
            .expect("opening ledger")
            .map(|entry| entry.unwrap());

        write_entries_to_ledger(&[ledger_path], ledger_entries);
        let db =
            DbLedger::open(ledger_path).expect("Expected to successfully open database ledger");
        Arc::new(RwLock::new(db))
    }
}

impl Service for Fullnode {
    type JoinReturnType = Option<FullnodeReturnType>;

    fn join(self) -> Result<Option<FullnodeReturnType>> {
        if let Some(rpc_service) = self.rpc_service {
            rpc_service.join()?;
        }
        if let Some(rpc_pubsub_service) = self.rpc_pubsub_service {
            rpc_pubsub_service.join()?;
        }

        self.gossip_service.join()?;

        match self.node_role {
            Some(NodeRole::Validator(validator_service)) => {
                if let Some(TvuReturnType::LeaderRotation(_, _, _)) = validator_service.join()? {
                    return Ok(Some(FullnodeReturnType::ValidatorToLeaderRotation));
                }
            }
            Some(NodeRole::Leader(leader_service)) => {
                if let Some(TpuReturnType::LeaderRotation) = leader_service.join()? {
                    return Ok(Some(FullnodeReturnType::LeaderToValidatorRotation));
                }
            }
            _ => (),
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crate::bank::Bank;
    use crate::cluster_info::Node;
    use crate::db_ledger::*;
    use crate::fullnode::{Fullnode, FullnodeReturnType, NodeRole, TvuReturnType};
    use crate::leader_scheduler::{
        make_active_set_entries, LeaderScheduler, LeaderSchedulerConfig,
    };
    use crate::ledger::{
        create_tmp_genesis, create_tmp_sample_ledger, tmp_copy_ledger, LedgerWriter,
    };
    use crate::packet::make_consecutive_blobs;
    use crate::service::Service;
    use crate::streamer::responder;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::cmp;
    use std::fs::remove_dir_all;
    use std::net::UdpSocket;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};

    #[test]
    fn validator_exit() {
        let keypair = Keypair::new();
        let tn = Node::new_localhost_with_pubkey(keypair.pubkey());
        let (mint, validator_ledger_path) =
            create_tmp_genesis("validator_exit", 10_000, keypair.pubkey(), 1000);
        let mut bank = Bank::new(&mint);
        let entry = tn.info.clone();
        let genesis_entries = &mint.create_entries();
        let entry_height = genesis_entries.len() as u64;

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            entry.id,
        )));
        bank.leader_scheduler = leader_scheduler;

        let last_id = bank.last_id();
        let v = Fullnode::new_with_bank(
            Arc::new(keypair),
            Arc::new(Keypair::new()),
            bank,
            entry_height,
            &last_id,
            tn,
            Some(&entry),
            &validator_ledger_path,
            false,
            None,
        );
        v.close().unwrap();
        remove_dir_all(validator_ledger_path).unwrap();
    }

    #[test]
    fn validator_parallel_exit() {
        let mut ledger_paths = vec![];
        let vals: Vec<Fullnode> = (0..2)
            .map(|i| {
                let keypair = Keypair::new();
                let tn = Node::new_localhost_with_pubkey(keypair.pubkey());
                let (mint, validator_ledger_path) = create_tmp_genesis(
                    &format!("validator_parallel_exit_{}", i),
                    10_000,
                    keypair.pubkey(),
                    1000,
                );
                ledger_paths.push(validator_ledger_path.clone());
                let mut bank = Bank::new(&mint);
                let entry = tn.info.clone();

                let leader_scheduler = Arc::new(RwLock::new(
                    LeaderScheduler::from_bootstrap_leader(entry.id),
                ));
                bank.leader_scheduler = leader_scheduler;

                let entry_height = mint.create_entries().len() as u64;
                let last_id = bank.last_id();
                Fullnode::new_with_bank(
                    Arc::new(keypair),
                    Arc::new(Keypair::new()),
                    bank,
                    entry_height,
                    &last_id,
                    tn,
                    Some(&entry),
                    &validator_ledger_path,
                    false,
                    None,
                )
            })
            .collect();

        //each validator can exit in parallel to speed many sequential calls to `join`
        vals.iter().for_each(|v| v.exit());
        //while join is called sequentially, the above exit call notified all the
        //validators to exit from all their threads
        vals.into_iter().for_each(|v| {
            v.join().unwrap();
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
        let num_ending_ticks = 1;
        let (_, bootstrap_leader_ledger_path, genesis_entries) = create_tmp_sample_ledger(
            "test_leader_to_leader_transition",
            10_000,
            num_ending_ticks,
            bootstrap_leader_keypair.pubkey(),
            500,
        );

        let initial_tick_height = genesis_entries
            .iter()
            .skip(2)
            .fold(0, |tick_count, entry| tick_count + entry.is_tick() as u64);

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
        let bootstrap_height = initial_tick_height + 1;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height as u64),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window_length),
        );

        // Start up the leader
        let mut bootstrap_leader = Fullnode::new(
            bootstrap_leader_node,
            &bootstrap_leader_ledger_path,
            Arc::new(bootstrap_leader_keypair),
            Arc::new(Keypair::new()),
            Some(bootstrap_leader_info.gossip),
            false,
            LeaderScheduler::new(&leader_scheduler_config),
            None,
        );

        // Wait for the leader to transition, ticks should cause the leader to
        // reach the height for leader rotation
        match bootstrap_leader.handle_role_transition().unwrap() {
            Some(FullnodeReturnType::LeaderToValidatorRotation) => (),
            _ => {
                panic!("Expected a leader transition");
            }
        }

        match bootstrap_leader.node_role {
            Some(NodeRole::Leader(_)) => (),
            _ => {
                panic!("Expected bootstrap leader to be a leader");
            }
        }
    }

    #[test]
    fn test_wrong_role_transition() {
        // Create the leader node information
        let bootstrap_leader_keypair = Arc::new(Keypair::new());
        let bootstrap_leader_node =
            Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());
        let bootstrap_leader_info = bootstrap_leader_node.info.clone();

        // Create the validator node information
        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

        // Make a common mint and a genesis entry for both leader + validator's ledgers
        let num_ending_ticks = 1;
        let (mint, bootstrap_leader_ledger_path, genesis_entries) = create_tmp_sample_ledger(
            "test_wrong_role_transition",
            10_000,
            num_ending_ticks,
            bootstrap_leader_keypair.pubkey(),
            500,
        );

        let last_id = genesis_entries
            .last()
            .expect("expected at least one genesis entry")
            .id;

        // Write the entries to the ledger that will cause leader rotation
        // after the bootstrap height
        let mut ledger_writer = LedgerWriter::open(&bootstrap_leader_ledger_path, false).unwrap();
        let (active_set_entries, validator_vote_account_keypair) = make_active_set_entries(
            &validator_keypair,
            &mint.keypair(),
            &last_id,
            &last_id,
            num_ending_ticks,
        );

        let genesis_tick_height = genesis_entries
            .iter()
            .skip(2)
            .fold(0, |tick_count, entry| tick_count + entry.is_tick() as u64)
            + num_ending_ticks as u64;
        ledger_writer.write_entries(&active_set_entries).unwrap();

        let validator_ledger_path =
            tmp_copy_ledger(&bootstrap_leader_ledger_path, "test_wrong_role_transition");
        let ledger_paths = vec![
            bootstrap_leader_ledger_path.clone(),
            validator_ledger_path.clone(),
        ];

        // Create the common leader scheduling configuration
        let num_slots_per_epoch = 3;
        let leader_rotation_interval = 5;
        let seed_rotation_interval = num_slots_per_epoch * leader_rotation_interval;

        // Set the bootstrap height exactly the current tick height, so that we can
        // test if the bootstrap leader knows to immediately transition to a validator
        // after parsing the ledger during startup
        let bootstrap_height = genesis_tick_height;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(genesis_tick_height),
        );

        {
            // Test that a node knows to transition to a validator based on parsing the ledger
            let leader_vote_account_keypair = Arc::new(Keypair::new());
            let bootstrap_leader = Fullnode::new(
                bootstrap_leader_node,
                &bootstrap_leader_ledger_path,
                bootstrap_leader_keypair,
                leader_vote_account_keypair,
                Some(bootstrap_leader_info.gossip),
                false,
                LeaderScheduler::new(&leader_scheduler_config),
                None,
            );

            match bootstrap_leader.node_role {
                Some(NodeRole::Validator(_)) => (),
                _ => {
                    panic!("Expected bootstrap leader to be a validator");
                }
            }

            // Test that a node knows to transition to a leader based on parsing the ledger
            let validator = Fullnode::new(
                validator_node,
                &validator_ledger_path,
                Arc::new(validator_keypair),
                Arc::new(validator_vote_account_keypair),
                Some(bootstrap_leader_info.gossip),
                false,
                LeaderScheduler::new(&leader_scheduler_config),
                None,
            );

            match validator.node_role {
                Some(NodeRole::Leader(_)) => (),
                _ => {
                    panic!("Expected node to be the leader");
                }
            }

            validator.close().expect("Expected node to close");
            bootstrap_leader.close().expect("Expected node to close");
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
        let leader_gossip = leader_node.info.gossip;

        // Create validator identity
        let num_ending_ticks = 1;
        let (mint, validator_ledger_path, genesis_entries) = create_tmp_sample_ledger(
            "test_validator_to_leader_transition",
            10_000,
            num_ending_ticks,
            leader_id,
            500,
        );

        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
        let validator_info = validator_node.info.clone();

        let mut last_id = genesis_entries
            .last()
            .expect("expected at least one genesis entry")
            .id;

        // Write two entries so that the validator is in the active set:
        //
        // 1) Give the validator a nonzero number of tokens
        // Write the bootstrap entries to the ledger that will cause leader rotation
        // after the bootstrap height
        //
        // 2) A vote from the validator
        let mut ledger_writer = LedgerWriter::open(&validator_ledger_path, false).unwrap();
        let (active_set_entries, validator_vote_account_keypair) =
            make_active_set_entries(&validator_keypair, &mint.keypair(), &last_id, &last_id, 0);
        let initial_tick_height = genesis_entries
            .iter()
            .skip(2)
            .fold(0, |tick_count, entry| tick_count + entry.is_tick() as u64);
        let initial_non_tick_height = genesis_entries.len() as u64 - initial_tick_height;
        let active_set_entries_len = active_set_entries.len() as u64;
        last_id = active_set_entries.last().unwrap().id;
        ledger_writer.write_entries(&active_set_entries).unwrap();
        let ledger_initial_len = genesis_entries.len() as u64 + active_set_entries_len;

        // Set the leader scheduler for the validator
        let leader_rotation_interval = 10;
        let num_bootstrap_slots = 2;
        let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(leader_rotation_interval * 2),
            Some(bootstrap_height),
        );

        // Start the validator
        let mut validator = Fullnode::new(
            validator_node,
            &validator_ledger_path,
            Arc::new(validator_keypair),
            Arc::new(validator_vote_account_keypair),
            Some(leader_gossip),
            false,
            LeaderScheduler::new(&leader_scheduler_config),
            None,
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
                leader_id,
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

        // Wait for validator to shut down tvu
        let node_role = validator.node_role.take();
        match node_role {
            Some(NodeRole::Validator(validator_services)) => {
                let join_result = validator_services
                    .join()
                    .expect("Expected successful validator join");
                if let Some(TvuReturnType::LeaderRotation(tick_height, _, _)) = join_result {
                    assert_eq!(tick_height, bootstrap_height);
                } else {
                    panic!("Expected validator to have exited due to leader rotation");
                }
            }
            _ => panic!("Role should not be leader"),
        }

        // Check the validator ledger for the correct entry + tick heights, we should've
        // transitioned after tick_height = bootstrap_height.
        let (bank, entry_height, _) = Fullnode::new_bank_from_ledger(
            &validator_ledger_path,
            Arc::new(RwLock::new(LeaderScheduler::new(&leader_scheduler_config))),
        );

        assert_eq!(bank.tick_height(), bootstrap_height);
        assert_eq!(
            entry_height,
            // Only the first genesis entry has num_hashes = 0, every other entry
            // had num_hashes = 1
            bootstrap_height + active_set_entries_len + initial_non_tick_height,
        );

        // Shut down
        t_responder.join().expect("responder thread join");
        validator.close().unwrap();
        DbLedger::destroy(&validator_ledger_path)
            .expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&validator_ledger_path).unwrap();
    }
}
