//! The `fullnode` module hosts all the fullnode microservices.

use bank::Bank;
use crdt::{Crdt, NodeInfo, TestNode};
use entry::Entry;
use entry_writer;
use ledger::Block;
use ncp::Ncp;
use packet::BlobRecycler;
use rpu::Rpu;
use service::Service;
use signature::{KeyPair, KeyPairUtil};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{stdin, stdout, BufReader};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{JoinHandle, Result};
use std::time::Duration;
use streamer;
use tpu::Tpu;
use tvu::Tvu;
use untrusted::Input;

//use std::time::Duration;
pub struct FullNode {
    exit: Arc<AtomicBool>,
    thread_hdls: Vec<JoinHandle<()>>,
}

pub enum LedgerFile {
    StdInOut,
    Path(String),
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
            KeyPair::from_pkcs8(Input::from(&pkcs8)).expect("from_pkcs8 in fullnode::Config new");
        let pubkey = keypair.pubkey();
        let node_info = NodeInfo::new_leader_with_pubkey(pubkey, bind_addr);
        Config { node_info, pkcs8 }
    }
    pub fn keypair(&self) -> KeyPair {
        KeyPair::from_pkcs8(Input::from(&self.pkcs8))
            .expect("from_pkcs8 in fullnode::Config keypair")
    }
}

impl FullNode {
    pub fn new(
        mut node: TestNode,
        leader: bool,
        ledger: LedgerFile,
        keypair_for_validator: Option<KeyPair>,
        network_entry_for_validator: Option<SocketAddr>,
    ) -> FullNode {
        info!("creating bank...");
        let bank = Bank::default();
        let (infile, outfile): (Box<Read>, Box<Write + Send>) = match ledger {
            LedgerFile::Path(path) => (
                Box::new(File::open(path.clone()).expect("opening ledger file")),
                Box::new(
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .expect("opening ledger file"),
                ),
            ),
            LedgerFile::StdInOut => (Box::new(stdin()), Box::new(stdout())),
        };
        let reader = BufReader::new(infile);
        let entries = entry_writer::read_entries(reader).map(|e| e.expect("failed to parse entry"));

        info!("processing ledger...");
        let (entry_height, ledger_tail) = bank.process_ledger(entries).expect("process_ledger");
        // entry_height is the network-wide agreed height of the ledger.
        //  initialize it from the input ledger
        info!("processed {} ledger...", entry_height);

        info!("creating networking stack...");

        let local_gossip_addr = node.sockets.gossip.local_addr().unwrap();
        let local_requests_addr = node.sockets.requests.local_addr().unwrap();
        info!(
            "starting... local gossip address: {} (advertising {})",
            local_gossip_addr, node.data.contact_info.ncp
        );
        let requests_addr = node.data.contact_info.rpu;
        let exit = Arc::new(AtomicBool::new(false));
        if !leader {
            let testnet_addr = network_entry_for_validator.expect("validator requires entry");

            let network_entry_point = NodeInfo::new_entry_point(testnet_addr);
            let keypair = keypair_for_validator.expect("validator requires keypair");
            let server = FullNode::new_validator(
                keypair,
                bank,
                entry_height,
                Some(ledger_tail),
                node,
                &network_entry_point,
                exit.clone(),
            );
            info!(
                "validator ready... local request address: {} (advertising {}) connected to: {}",
                local_requests_addr, requests_addr, testnet_addr
            );
            server
        } else {
            node.data.leader_id = node.data.id;

            let server = FullNode::new_leader(
                bank,
                entry_height,
                Some(ledger_tail),
                //Some(Duration::from_millis(1000)),
                None,
                node,
                exit.clone(),
                outfile,
            );
            info!(
                "leader ready... local request address: {} (advertising {})",
                local_requests_addr, requests_addr
            );
            server
        }
    }

    fn new_window(
        ledger_tail: Option<Vec<Entry>>,
        entry_height: u64,
        crdt: &Arc<RwLock<Crdt>>,
        blob_recycler: &BlobRecycler,
    ) -> streamer::Window {
        match ledger_tail {
            Some(ledger_tail) => {
                // convert to blobs
                let mut blobs = VecDeque::new();
                ledger_tail.to_blobs(&blob_recycler, &mut blobs);

                // flatten deque to vec
                let blobs: Vec<_> = blobs.into_iter().collect();
                streamer::initialized_window(&crdt, blobs, entry_height)
            }
            None => streamer::default_window(),
        }
    }

    /// Create a server instance acting as a leader.
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
    /// ```
    pub fn new_leader<W: Write + Send + 'static>(
        bank: Bank,
        entry_height: u64,
        ledger_tail: Option<Vec<Entry>>,
        tick_duration: Option<Duration>,
        node: TestNode,
        exit: Arc<AtomicBool>,
        writer: W,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(
            &bank.clone(),
            node.sockets.requests,
            node.sockets.respond,
            exit.clone(),
        );
        thread_hdls.extend(rpu.thread_hdls());

        let blob_recycler = BlobRecycler::default();
        let crdt = Arc::new(RwLock::new(Crdt::new(node.data).expect("Crdt::new")));
        let (tpu, blob_receiver) = Tpu::new(
            &bank.clone(),
            &crdt.clone(),
            tick_duration,
            node.sockets.transaction,
            &blob_recycler.clone(),
            exit.clone(),
            writer,
        );
        thread_hdls.extend(tpu.thread_hdls());
        let window = FullNode::new_window(ledger_tail, entry_height, &crdt, &blob_recycler);
        let ncp = Ncp::new(
            &crdt.clone(),
            window.clone(),
            node.sockets.gossip,
            node.sockets.gossip_send,
            exit.clone(),
        ).expect("Ncp::new");
        thread_hdls.extend(ncp.thread_hdls());

        let t_broadcast = streamer::broadcaster(
            node.sockets.broadcast,
            crdt,
            window,
            entry_height,
            blob_recycler.clone(),
            blob_receiver,
        );
        thread_hdls.extend(vec![t_broadcast]);

        FullNode { exit, thread_hdls }
    }

    /// Create a server instance acting as a validator.
    ///
    /// ```text
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
    pub fn new_validator(
        keypair: KeyPair,
        bank: Bank,
        entry_height: u64,
        ledger_tail: Option<Vec<Entry>>,
        node: TestNode,
        entry_point: &NodeInfo,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(
            &bank.clone(),
            node.sockets.requests,
            node.sockets.respond,
            exit.clone(),
        );
        thread_hdls.extend(rpu.thread_hdls());

        let crdt = Arc::new(RwLock::new(Crdt::new(node.data).expect("Crdt::new")));
        crdt.write()
            .expect("'crdt' write lock before insert() in pub fn replicate")
            .insert(&entry_point);

        let blob_recycler = BlobRecycler::default();

        let window = FullNode::new_window(ledger_tail, entry_height, &crdt, &blob_recycler);

        let ncp = Ncp::new(
            &crdt.clone(),
            window.clone(),
            node.sockets.gossip,
            node.sockets.gossip_send,
            exit.clone(),
        ).expect("Ncp::new");

        let tvu = Tvu::new(
            keypair,
            bank.clone(),
            entry_height,
            crdt.clone(),
            window.clone(),
            node.sockets.replicate,
            node.sockets.repair,
            node.sockets.retransmit,
            exit.clone(),
        );
        thread_hdls.extend(tvu.thread_hdls());
        thread_hdls.extend(ncp.thread_hdls());
        FullNode { exit, thread_hdls }
    }

    //used for notifying many nodes in parallel to exit
    pub fn notify_exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }
    pub fn close(self) -> Result<()> {
        self.exit.store(true, Ordering::Relaxed);
        self.join()
    }
}

impl Service for FullNode {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        self.thread_hdls
    }

    fn join(self) -> Result<()> {
        for thread_hdl in self.thread_hdls() {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bank::Bank;
    use crdt::TestNode;
    use fullnode::FullNode;
    use mint::Mint;
    use signature::{KeyPair, KeyPairUtil};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    #[test]
    fn validator_exit() {
        let kp = KeyPair::new();
        let tn = TestNode::new_localhost_with_pubkey(kp.pubkey());
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let exit = Arc::new(AtomicBool::new(false));
        let entry = tn.data.clone();
        let v = FullNode::new_validator(kp, bank, 0, None, tn, &entry, exit);
        v.notify_exit();
        v.close().unwrap();
    }
}
