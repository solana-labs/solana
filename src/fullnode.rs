//! The `fullnode` module hosts all the fullnode microservices.

use bank::Bank;
use broadcast_stage::BroadcastStage;
use crdt::{Crdt, Node, NodeInfo};
use drone::DRONE_PORT;
use entry::Entry;
use ledger::read_ledger;
use ncp::Ncp;
use packet::BlobRecycler;
use rpc::{JsonRpcService, RPC_PORT};
use rpu::Rpu;
use service::Service;
use signature::{Keypair, KeypairUtil};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::Result;
use tpu::Tpu;
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

    fn join(self) -> Result<()> {
        self.tpu.join()?;
        self.broadcast_stage.join()
    }
}

pub struct ValidatorServices {
    tvu: Tvu,
}

impl ValidatorServices {
    fn new(tvu: Tvu) -> Self {
        ValidatorServices { tvu }
    }

    fn join(self) -> Result<()> {
        self.tvu.join()
    }
}

pub enum FullNodeReturnType {
    LeaderRotation,
}

pub struct Fullnode {
    exit: Arc<AtomicBool>,
    rpu: Rpu,
    rpc_service: JsonRpcService,
    ncp: Ncp,
    pub node_role: NodeRole,
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
    ) -> Self {
        info!("creating bank...");
        let bank = Bank::new_default(leader_addr.is_none());

        let entries = read_ledger(ledger_path, true).expect("opening ledger");

        let entries = entries
            .map(|e| e.unwrap_or_else(|err| panic!("failed to parse entry. error: {}", err)));

        info!("processing ledger...");
        let (entry_height, ledger_tail) = bank.process_ledger(entries).expect("process_ledger");
        // entry_height is the network-wide agreed height of the ledger.
        //  initialize it from the input ledger
        info!("processed {} ledger...", entry_height);

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
            Some(ledger_path),
            sigverify_disabled,
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
        ledger_path: Option<&str>,
        sigverify_disabled: bool,
    ) -> Self {
        if leader_info.is_none() {
            node.info.leader_id = node.info.id;
        }
        let exit = Arc::new(AtomicBool::new(false));
        let bank = Arc::new(bank);
        let blob_recycler = BlobRecycler::default();

        let rpu = Rpu::new(
            &bank,
            node.sockets.requests,
            node.sockets.respond,
            &blob_recycler,
            exit.clone(),
        );

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

        let window =
            window::new_window_from_entries(ledger_tail, entry_height, &node.info, &blob_recycler);
        let shared_window = Arc::new(RwLock::new(window));

        let crdt = Arc::new(RwLock::new(Crdt::new(node.info).expect("Crdt::new")));

        let ncp = Ncp::new(
            &crdt,
            shared_window.clone(),
            blob_recycler.clone(),
            ledger_path,
            node.sockets.gossip,
            exit.clone(),
        );

        let node_role;
        match leader_info {
            Some(leader_info) => {
                // Start in validator mode.
                // TODO: let Crdt get that data from the network?
                crdt.write().unwrap().insert(leader_info);
                let tvu = Tvu::new(
                    keypair,
                    &bank,
                    entry_height,
                    crdt,
                    shared_window,
                    blob_recycler.clone(),
                    node.sockets.replicate,
                    node.sockets.repair,
                    node.sockets.retransmit,
                    ledger_path,
                    exit.clone(),
                );
                let validator_state = ValidatorServices::new(tvu);
                node_role = NodeRole::Validator(validator_state);
            }
            None => {
                // Start in leader mode.
                let ledger_path = ledger_path.expect("ledger path");
                let tick_duration = None;
                // TODO: To light up PoH, uncomment the following line:
                //let tick_duration = Some(Duration::from_millis(1000));

                let (tpu, blob_receiver) = Tpu::new(
                    keypair,
                    &bank,
                    &crdt,
                    tick_duration,
                    node.sockets.transaction,
                    blob_recycler.clone(),
                    exit.clone(),
                    ledger_path,
                    sigverify_disabled,
                );

                let broadcast_stage = BroadcastStage::new(
                    node.sockets.broadcast,
                    crdt,
                    shared_window,
                    entry_height,
                    blob_recycler.clone(),
                    blob_receiver,
                );
                let leader_state = LeaderServices::new(tpu, broadcast_stage);
                node_role = NodeRole::Leader(leader_state);
            }
        }

        Fullnode {
            rpu,
            ncp,
            rpc_service,
            node_role,
            exit,
        }
    }

    //used for notifying many nodes in parallel to exit
    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> Result<(Option<FullNodeReturnType>)> {
        self.exit();
        self.join()
    }
}

impl Service for Fullnode {
    type JoinReturnType = Option<FullNodeReturnType>;

    fn join(self) -> Result<Option<FullNodeReturnType>> {
        self.rpu.join()?;
        self.ncp.join()?;
        self.rpc_service.join()?;
        match self.node_role {
            NodeRole::Validator(validator_service) => {
                validator_service.join()?;
            }
            NodeRole::Leader(leader_service) => {
                leader_service.join()?;
            }
        }

        // TODO: Case on join values above to determine if
        // a leader rotation was in order, and propagate up a
        // signal to reflect that
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use bank::Bank;
    use crdt::Node;
    use fullnode::Fullnode;
    use mint::Mint;
    use service::Service;
    use signature::{Keypair, KeypairUtil};

    #[test]
    fn validator_exit() {
        let keypair = Keypair::new();
        let tn = Node::new_localhost_with_pubkey(keypair.pubkey());
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let entry = tn.info.clone();
        let v = Fullnode::new_with_bank(keypair, bank, 0, &[], tn, Some(&entry), None, false);
        v.close().unwrap();
    }
    #[test]
    fn validator_parallel_exit() {
        let vals: Vec<Fullnode> = (0..2)
            .map(|_| {
                let keypair = Keypair::new();
                let tn = Node::new_localhost_with_pubkey(keypair.pubkey());
                let alice = Mint::new(10_000);
                let bank = Bank::new(&alice);
                let entry = tn.info.clone();
                Fullnode::new_with_bank(keypair, bank, 0, &[], tn, Some(&entry), None, false)
            })
            .collect();
        //each validator can exit in parallel to speed many sequential calls to `join`
        vals.iter().for_each(|v| v.exit());
        //while join is called sequentially, the above exit call notified all the
        //validators to exit from all their threads
        vals.into_iter().for_each(|v| {
            v.join().unwrap();
        });
    }
}
