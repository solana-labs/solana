//! The `fullnode` module hosts all the fullnode microservices.

use bank::Bank;
use broadcast_stage::BroadcastStage;
use crdt::{Crdt, Node, NodeInfo};
use drone::DRONE_PORT;
use entry::Entry;
use ledger::read_ledger;
use ncp::Ncp;
use rpc::{JsonRpcService, RPC_PORT};
use rpu::Rpu;
use service::Service;
use signature::{Keypair, KeypairUtil, Pubkey};
use std::net::UdpSocket;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::Result;
use tpu::{Tpu, TpuReturnType};
use tvu::Tvu;
use untrusted::Input;
use window;

pub enum NodeRole {
    Leader(LeaderServices),
    Validator(ValidatorServices),
}

pub struct LeaderServices {
    tpu: Tpu,
    broadcast_stage: BroadcastStage,
}

impl LeaderServices {
    fn new(tpu: Tpu, broadcast_stage: BroadcastStage) -> Self {
        LeaderServices {
            tpu,
            broadcast_stage,
        }
    }

    pub fn join(self) -> Result<Option<TpuReturnType>> {
        self.broadcast_stage.join()?;
        self.tpu.join()
    }

    pub fn exit(&self) -> () {
        self.tpu.exit();
    }
}

pub struct ValidatorServices {
    tvu: Tvu,
}

impl ValidatorServices {
    fn new(tvu: Tvu) -> Self {
        ValidatorServices { tvu }
    }

    pub fn join(self) -> Result<()> {
        self.tvu.join()
    }

    pub fn exit(&self) -> () {
        //TODO: implement exit for Tvu
    }
}

pub enum FullnodeReturnType {
    LeaderRotation,
}

pub struct Fullnode {
    pub node_role: Option<NodeRole>,
    keypair: Arc<Keypair>,
    exit: Arc<AtomicBool>,
    rpu: Option<Rpu>,
    rpc_service: JsonRpcService,
    ncp: Ncp,
    bank: Arc<Bank>,
    crdt: Arc<RwLock<Crdt>>,
    ledger_path: String,
    shared_window: window::SharedWindow,
    replicate_socket: Vec<UdpSocket>,
    repair_socket: UdpSocket,
    retransmit_socket: UdpSocket,
    requests_socket: UdpSocket,
    respond_socket: UdpSocket,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
/// Fullnode configuration to be stored in file
pub struct Config {
    pub node_info: NodeInfo,
    pkcs8: Vec<u8>,
}

/// Structure to be replicated by the network
impl Config {
    pub fn new(bind_addr: &SocketAddr, pkcs8: Vec<u8>) -> Self {
        let keypair =
            Keypair::from_pkcs8(Input::from(&pkcs8)).expect("from_pkcs8 in fullnode::Config new");
        let pubkey = keypair.pubkey();
        let node_info = NodeInfo::new_with_pubkey_socketaddr(pubkey, bind_addr);
        Config { node_info, pkcs8 }
    }
    pub fn keypair(&self) -> Keypair {
        Keypair::from_pkcs8(Input::from(&self.pkcs8))
            .expect("from_pkcs8 in fullnode::Config keypair")
    }
}

impl Fullnode {
    pub fn new(
        node: Node,
        ledger_path: &str,
        keypair: Keypair,
        leader_addr: Option<SocketAddr>,
        sigverify_disabled: bool,
        leader_rotation_interval: Option<u64>,
    ) -> Self {
        info!("creating bank...");
        let (bank, entry_height, ledger_tail) = Self::new_bank_from_ledger(ledger_path);

        info!("creating networking stack...");
        let local_gossip_addr = node.sockets.gossip.local_addr().unwrap();

        info!(
            "starting... local gossip address: {} (advertising {})",
            local_gossip_addr, node.info.contact_info.ncp
        );

        let local_requests_addr = node.sockets.requests.local_addr().unwrap();
        let requests_addr = node.info.contact_info.rpu;
        let leader_info = leader_addr.map(|i| NodeInfo::new_entry_point(&i));
        let server = Self::new_with_bank(
            keypair,
            bank,
            entry_height,
            &ledger_tail,
            node,
            leader_info.as_ref(),
            ledger_path,
            sigverify_disabled,
            leader_rotation_interval,
        );

        match leader_addr {
            Some(leader_addr) => {
                info!(
                    "validator ready... local request address: {} (advertising {}) connected to: {}",
                    local_requests_addr, requests_addr, leader_addr
                );
            }
            None => {
                info!(
                    "leader ready... local request address: {} (advertising {})",
                    local_requests_addr, requests_addr
                );
            }
        }

        server
    }

    /// Create a fullnode instance acting as a leader or validator.
    ///
    /// ```text
    ///              .---------------------.
    ///              |  Leader             |
    ///              |                     |
    ///  .--------.  |  .-----.            |
    ///  |        |---->|     |            |
    ///  | Client |  |  | RPU |            |
    ///  |        |<----|     |            |
    ///  `----+---`  |  `-----`            |
    ///       |      |     ^               |
    ///       |      |     |               |
    ///       |      |  .--+---.           |
    ///       |      |  | Bank |           |
    ///       |      |  `------`           |
    ///       |      |     ^               |
    ///       |      |     |               |    .------------.
    ///       |      |  .--+--.   .-----.  |    |            |
    ///       `-------->| TPU +-->| NCP +------>| Validators |
    ///              |  `-----`   `-----`  |    |            |
    ///              |                     |    `------------`
    ///              `---------------------`
    ///
    ///               .-------------------------------.
    ///               | Validator                     |
    ///               |                               |
    ///   .--------.  |            .-----.            |
    ///   |        |-------------->|     |            |
    ///   | Client |  |            | RPU |            |
    ///   |        |<--------------|     |            |
    ///   `--------`  |            `-----`            |
    ///               |               ^               |
    ///               |               |               |
    ///               |            .--+---.           |
    ///               |            | Bank |           |
    ///               |            `------`           |
    ///               |               ^               |
    ///   .--------.  |               |               |    .------------.
    ///   |        |  |            .--+--.            |    |            |
    ///   | Leader |<------------->| TVU +<--------------->|            |
    ///   |        |  |            `-----`            |    | Validators |
    ///   |        |  |               ^               |    |            |
    ///   |        |  |               |               |    |            |
    ///   |        |  |            .--+--.            |    |            |
    ///   |        |<------------->| NCP +<--------------->|            |
    ///   |        |  |            `-----`            |    |            |
    ///   `--------`  |                               |    `------------`
    ///               `-------------------------------`
    /// ```
    pub fn new_with_bank(
        keypair: Keypair,
        bank: Bank,
        entry_height: u64,
        ledger_tail: &[Entry],
        mut node: Node,
        leader_info: Option<&NodeInfo>,
        ledger_path: &str,
        sigverify_disabled: bool,
        leader_rotation_interval: Option<u64>,
    ) -> Self {
        if leader_info.is_none() {
            node.info.leader_id = node.info.id;
        }
        let exit = Arc::new(AtomicBool::new(false));
        let bank = Arc::new(bank);

        let rpu = Some(Rpu::new(
            &bank,
            node.sockets
                .requests
                .try_clone()
                .expect("Failed to clone requests socket"),
            node.sockets
                .respond
                .try_clone()
                .expect("Failed to clone respond socket"),
        ));

        // TODO: this code assumes this node is the leader
        let mut drone_addr = node.info.contact_info.tpu;
        drone_addr.set_port(DRONE_PORT);
        let rpc_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(0)), RPC_PORT);
        let rpc_service = JsonRpcService::new(
            &bank,
            node.info.contact_info.tpu,
            drone_addr,
            rpc_addr,
            exit.clone(),
        );

        let window = window::new_window_from_entries(ledger_tail, entry_height, &node.info);
        let shared_window = Arc::new(RwLock::new(window));

        let mut crdt = Crdt::new(node.info).expect("Crdt::new");
        if let Some(interval) = leader_rotation_interval {
            crdt.set_leader_rotation_interval(interval);
        }
        let crdt = Arc::new(RwLock::new(crdt));

        let ncp = Ncp::new(
            &crdt,
            shared_window.clone(),
            Some(ledger_path),
            node.sockets.gossip,
            exit.clone(),
        );

        let keypair = Arc::new(keypair);
        let node_role;
        match leader_info {
            Some(leader_info) => {
                // Start in validator mode.
                // TODO: let Crdt get that data from the network?
                crdt.write().unwrap().insert(leader_info);
                let tvu = Tvu::new(
                    keypair.clone(),
                    &bank,
                    entry_height,
                    crdt.clone(),
                    shared_window.clone(),
                    node.sockets
                        .replicate
                        .iter()
                        .map(|s| s.try_clone().expect("Failed to clone replicate sockets"))
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
                    exit.clone(),
                );
                let validator_state = ValidatorServices::new(tvu);
                node_role = Some(NodeRole::Validator(validator_state));
            }
            None => {
                // Start in leader mode.
                let tick_duration = None;
                // TODO: To light up PoH, uncomment the following line:
                //let tick_duration = Some(Duration::from_millis(1000));
                let (tpu, entry_receiver, tpu_exit) = Tpu::new(
                    keypair.clone(),
                    &bank,
                    &crdt,
                    tick_duration,
                    node.sockets
                        .transaction
                        .iter()
                        .map(|s| s.try_clone().expect("Failed to clone transaction sockets"))
                        .collect(),
                    ledger_path,
                    sigverify_disabled,
                    entry_height,
                );

                let broadcast_stage = BroadcastStage::new(
                    node.sockets
                        .broadcast
                        .try_clone()
                        .expect("Failed to clone broadcast socket"),
                    crdt.clone(),
                    shared_window.clone(),
                    entry_height,
                    entry_receiver,
                    tpu_exit,
                );
                let leader_state = LeaderServices::new(tpu, broadcast_stage);
                node_role = Some(NodeRole::Leader(leader_state));
            }
        }

        Fullnode {
            keypair,
            crdt,
            shared_window,
            bank,
            rpu,
            ncp,
            rpc_service,
            node_role,
            ledger_path: ledger_path.to_owned(),
            exit,
            replicate_socket: node.sockets.replicate,
            repair_socket: node.sockets.repair,
            retransmit_socket: node.sockets.retransmit,
            requests_socket: node.sockets.requests,
            respond_socket: node.sockets.respond,
        }
    }

    fn leader_to_validator(&mut self) -> Result<()> {
        // TODO: We can avoid building the bank again once RecordStage is
        // integrated with BankingStage
        let (bank, entry_height, _) = Self::new_bank_from_ledger(&self.ledger_path);
        self.bank = Arc::new(bank);

        {
            let mut wcrdt = self.crdt.write().unwrap();
            let scheduled_leader = wcrdt.get_scheduled_leader(entry_height);
            match scheduled_leader {
                //TODO: Handle the case where we don't know who the next
                //scheduled leader is
                None => (),
                Some(leader_id) => wcrdt.set_leader(leader_id),
            }
        }

        // Make a new RPU to serve requests out of the new bank we've created
        // instead of the old one
        if !self.rpu.is_none() {
            let old_rpu = self.rpu.take().unwrap();
            old_rpu.close()?;
            self.rpu = Some(Rpu::new(
                &self.bank,
                self.requests_socket
                    .try_clone()
                    .expect("Failed to clone requests socket"),
                self.respond_socket
                    .try_clone()
                    .expect("Failed to clone respond socket"),
            ));
        }

        let tvu = Tvu::new(
            self.keypair.clone(),
            &self.bank,
            entry_height,
            self.crdt.clone(),
            self.shared_window.clone(),
            self.replicate_socket
                .iter()
                .map(|s| s.try_clone().expect("Failed to clone replicate sockets"))
                .collect(),
            self.repair_socket
                .try_clone()
                .expect("Failed to clone repair socket"),
            self.retransmit_socket
                .try_clone()
                .expect("Failed to clone retransmit socket"),
            Some(&self.ledger_path),
            self.exit.clone(),
        );
        let validator_state = ValidatorServices::new(tvu);
        self.node_role = Some(NodeRole::Validator(validator_state));
        Ok(())
    }

    pub fn handle_role_transition(&mut self) -> Result<Option<FullnodeReturnType>> {
        let node_role = self.node_role.take();
        match node_role {
            Some(NodeRole::Leader(leader_services)) => match leader_services.join()? {
                Some(TpuReturnType::LeaderRotation) => {
                    self.leader_to_validator()?;
                    Ok(Some(FullnodeReturnType::LeaderRotation))
                }
                _ => Ok(None),
            },
            Some(NodeRole::Validator(validator_services)) => match validator_services.join()? {
                _ => Ok(None),
            },
            None => Ok(None),
        }
    }

    //used for notifying many nodes in parallel to exit
    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
        if let Some(ref rpu) = self.rpu {
            rpu.exit();
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

    // TODO: only used for testing, get rid of this once we have actual
    // leader scheduling
    pub fn set_scheduled_leader(&self, leader_id: Pubkey, entry_height: u64) {
        self.crdt
            .write()
            .unwrap()
            .set_scheduled_leader(entry_height, leader_id);
    }

    fn new_bank_from_ledger(ledger_path: &str) -> (Bank, u64, Vec<Entry>) {
        let bank = Bank::new_default(false);
        let entries = read_ledger(ledger_path, true).expect("opening ledger");
        let entries = entries
            .map(|e| e.unwrap_or_else(|err| panic!("failed to parse entry. error: {}", err)));
        info!("processing ledger...");
        let (entry_height, ledger_tail) = bank.process_ledger(entries).expect("process_ledger");
        // entry_height is the network-wide agreed height of the ledger.
        //  initialize it from the input ledger
        info!("processed {} ledger...", entry_height);
        (bank, entry_height, ledger_tail)
    }
}

impl Service for Fullnode {
    type JoinReturnType = Option<FullnodeReturnType>;

    fn join(self) -> Result<Option<FullnodeReturnType>> {
        if let Some(rpu) = self.rpu {
            rpu.join()?;
        }
        self.ncp.join()?;
        self.rpc_service.join()?;

        match self.node_role {
            Some(NodeRole::Validator(validator_service)) => {
                validator_service.join()?;
            }
            Some(NodeRole::Leader(leader_service)) => {
                if let Some(TpuReturnType::LeaderRotation) = leader_service.join()? {
                    return Ok(Some(FullnodeReturnType::LeaderRotation));
                }
            }
            _ => (),
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use bank::Bank;
    use crdt::Node;
    use fullnode::Fullnode;
    use ledger::genesis;
    use service::Service;
    use signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;

    #[test]
    fn validator_exit() {
        let keypair = Keypair::new();
        let tn = Node::new_localhost_with_pubkey(keypair.pubkey());
        let (alice, validator_ledger_path) = genesis("validator_exit", 10_000);
        let bank = Bank::new(&alice);
        let entry = tn.info.clone();
        let v = Fullnode::new_with_bank(
            keypair,
            bank,
            0,
            &[],
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
                let (alice, validator_ledger_path) =
                    genesis(&format!("validator_parallel_exit_{}", i), 10_000);
                ledger_paths.push(validator_ledger_path.clone());
                let bank = Bank::new(&alice);
                let entry = tn.info.clone();
                Fullnode::new_with_bank(
                    keypair,
                    bank,
                    0,
                    &[],
                    tn,
                    Some(&entry),
                    &validator_ledger_path,
                    false,
                    None,
                )
            }).collect();

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
}
