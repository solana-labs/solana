//! The `fullnode` module hosts all the fullnode microservices.

use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::blocktree_processor::{self, BankForksInfo};
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
use crate::tvu::{Sockets, Tvu, TvuRotationInfo, TvuRotationReceiver};
use log::Level;
use solana_metrics::counter::Counter;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::timestamp;
use std::net::UdpSocket;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{spawn, Result};
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
    pub blockstream: Option<String>,
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
            blockstream: None,
            storage_rotate_count: NUM_HASHES_FOR_STORAGE_ROTATE,
            leader_scheduler_config: LeaderSchedulerConfig::default(),
            tick_config: PohServiceConfig::default(),
        }
    }
}

impl FullnodeConfig {
    pub fn ticks_per_slot(&self) -> u64 {
        self.leader_scheduler_config.ticks_per_slot
    }
}

pub struct Fullnode {
    id: Pubkey,
    exit: Arc<AtomicBool>,
    rpc_service: Option<JsonRpcService>,
    rpc_pubsub_service: Option<PubSubService>,
    gossip_service: GossipService,
    sigverify_disabled: bool,
    tpu_sockets: Vec<UdpSocket>,
    broadcast_socket: UdpSocket,
    node_services: NodeServices,
    rotation_receiver: TvuRotationReceiver,
    blocktree: Arc<Blocktree>,
    leader_scheduler: Arc<RwLock<LeaderScheduler>>,
}

impl Fullnode {
    pub fn new<T>(
        mut node: Node,
        keypair: &Arc<Keypair>,
        ledger_path: &str,
        voting_keypair: T,
        entrypoint_info_option: Option<&NodeInfo>,
        config: &FullnodeConfig,
    ) -> Self
    where
        T: 'static + KeypairUtil + Sync + Send,
    {
        info!("creating bank...");

        let id = keypair.pubkey();
        assert_eq!(id, node.info.id);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::new(
            &config.leader_scheduler_config,
        )));
        let (bank_forks, bank_forks_info, blocktree, ledger_signal_receiver) =
            new_banks_from_blocktree(ledger_path, config.ticks_per_slot(), &leader_scheduler);

        info!("node info: {:?}", node.info);
        info!("node entrypoint_info: {:?}", entrypoint_info_option);
        info!(
            "node local gossip address: {}",
            node.sockets.gossip.local_addr().unwrap()
        );

        let exit = Arc::new(AtomicBool::new(false));
        let blocktree = Arc::new(blocktree);
        let bank_forks = Arc::new(RwLock::new(bank_forks));

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
            &bank_forks,
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
            Some(bank_forks.clone()),
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
            &bank_forks,
            &bank_forks_info,
            &cluster_info,
            sockets,
            blocktree.clone(),
            config.storage_rotate_count,
            &rotation_sender,
            &storage_state,
            config.blockstream.as_ref(),
            ledger_signal_receiver,
            leader_scheduler.clone(),
            &subscriptions,
        );
        let tpu = Tpu::new(id, &cluster_info);

        inc_new_counter_info!("fullnode-new", 1);
        Self {
            id,
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
        }
    }

    fn rotate(&mut self, rotation_info: TvuRotationInfo) -> FullnodeReturnType {
        trace!(
            "{:?}: rotate for slot={} to leader={:?} using last_entry_id={:?}",
            self.id,
            rotation_info.slot,
            rotation_info.leader_id,
            rotation_info.last_entry_id,
        );

        if rotation_info.leader_id == self.id {
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
            self.node_services.tpu.switch_to_leader(
                Arc::new(rotation_info.bank),
                PohServiceConfig::default(),
                self.tpu_sockets
                    .iter()
                    .map(|s| s.try_clone().expect("Failed to clone TPU sockets"))
                    .collect(),
                self.broadcast_socket
                    .try_clone()
                    .expect("Failed to clone broadcast socket"),
                self.sigverify_disabled,
                rotation_info.slot,
                rotation_info.last_entry_id,
                &self.blocktree,
                &self.leader_scheduler,
            );
            transition
        } else {
            debug!("{:?} rotating to validator role", self.id);
            self.node_services.tpu.switch_to_forwarder(
                rotation_info.leader_id,
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
                Ok(rotation_info) => {
                    let slot = rotation_info.slot;
                    let transition = self.rotate(rotation_info);
                    debug!("role transition complete: {:?}", transition);
                    if let Some(ref rotation_notifier) = rotation_notifier {
                        rotation_notifier.send((transition, slot)).unwrap();
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

pub fn new_banks_from_blocktree(
    blocktree_path: &str,
    ticks_per_slot: u64,
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> (BankForks, Vec<BankForksInfo>, Blocktree, Receiver<bool>) {
    let (blocktree, ledger_signal_receiver) =
        Blocktree::open_with_config_signal(blocktree_path, ticks_per_slot)
            .expect("Expected to successfully open database ledger");
    let genesis_block =
        GenesisBlock::load(blocktree_path).expect("Expected to successfully open genesis block");

    let (bank_forks, bank_forks_info) =
        blocktree_processor::process_blocktree(&genesis_block, &blocktree, leader_scheduler)
            .expect("process_blocktree failed");

    (
        bank_forks,
        bank_forks_info,
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
