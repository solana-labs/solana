//! The `fullnode` module hosts all the fullnode microservices.

use bank::Bank;
use crdt::{Crdt, ReplicatedData, TestNode};
use entry_writer;
use ncp::Ncp;
use packet::BlobRecycler;
use rpu::Rpu;
use std::fs::File;
use std::io::{sink, stdin, stdout, BufReader};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;
use streamer;
use tpu::Tpu;
use tvu::Tvu;

//use std::time::Duration;
pub struct FullNode {
    pub thread_hdls: Vec<JoinHandle<()>>,
}

pub enum InFile {
    StdIn,
    Path(String),
}

pub enum OutFile {
    StdOut,
    Path(String),
}

impl FullNode {
    pub fn new(
        mut node: TestNode,
        leader: bool,
        infile: InFile,
        network_entry_for_validator: Option<SocketAddr>,
        outfile_for_leader: Option<OutFile>,
        exit: Arc<AtomicBool>,
    ) -> FullNode {
        info!("creating bank...");
        let bank = Bank::default();
        let infile: Box<Read> = match infile {
            InFile::Path(path) => Box::new(File::open(path).unwrap()),
            InFile::StdIn => Box::new(stdin()),
        };
        let reader = BufReader::new(infile);
        let entries = entry_writer::read_entries(reader).map(|e| e.expect("failed to parse entry"));
        info!("processing ledger...");
        let entry_height = bank.process_ledger(entries).expect("process_ledger");

        // entry_height is the network-wide agreed height of the ledger.
        //  initialize it from the input ledger
        info!("processed {} ledger...", entry_height);

        info!("creating networking stack...");

        let local_gossip_addr = node.sockets.gossip.local_addr().unwrap();
        let local_requests_addr = node.sockets.requests.local_addr().unwrap();
        info!(
            "starting... local gossip address: {} (advertising {})",
            local_gossip_addr, node.data.gossip_addr
        );
        let requests_addr = node.data.requests_addr.clone();
        if !leader {
            let testnet_addr = network_entry_for_validator.expect("validator requires entry");

            let network_entry_point = ReplicatedData::new_entry_point(testnet_addr);
            let server = FullNode::new_validator(
                bank,
                entry_height,
                node,
                network_entry_point,
                exit.clone(),
            );
            info!(
                "validator ready... local request address: {} (advertising {}) connected to: {}",
                local_requests_addr, requests_addr, testnet_addr
            );
            server
        } else {
            node.data.current_leader_id = node.data.id.clone();
            let outfile_for_leader: Box<Write + Send> = match outfile_for_leader {
                Some(OutFile::Path(file)) => {
                    Box::new(File::create(file).expect("opening ledger file"))
                }
                Some(OutFile::StdOut) => Box::new(stdout()),
                None => Box::new(sink()),
            };

            let server = FullNode::new_leader(
                bank,
                entry_height,
                //Some(Duration::from_millis(1000)),
                None,
                node,
                exit.clone(),
                outfile_for_leader,
            );
            info!(
                "leader ready... local request address: {} (advertising {})",
                local_requests_addr, requests_addr
            );
            server
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
        tick_duration: Option<Duration>,
        node: TestNode,
        exit: Arc<AtomicBool>,
        writer: W,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(
            bank.clone(),
            node.sockets.requests,
            node.sockets.respond,
            exit.clone(),
        );
        thread_hdls.extend(rpu.thread_hdls);

        let blob_recycler = BlobRecycler::default();
        let (tpu, blob_receiver) = Tpu::new(
            bank.clone(),
            tick_duration,
            node.sockets.transaction,
            blob_recycler.clone(),
            exit.clone(),
            writer,
        );
        thread_hdls.extend(tpu.thread_hdls);
        let crdt = Arc::new(RwLock::new(Crdt::new(node.data)));
        let window = streamer::default_window();
        let ncp = Ncp::new(
            crdt.clone(),
            window.clone(),
            node.sockets.gossip,
            node.sockets.gossip_send,
            exit.clone(),
        ).expect("Ncp::new");
        thread_hdls.extend(ncp.thread_hdls);

        let t_broadcast = streamer::broadcaster(
            node.sockets.broadcast,
            exit.clone(),
            crdt,
            window,
            entry_height,
            blob_recycler.clone(),
            blob_receiver,
        );
        thread_hdls.extend(vec![t_broadcast]);

        FullNode { thread_hdls }
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
        bank: Bank,
        entry_height: u64,
        node: TestNode,
        entry_point: ReplicatedData,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(
            bank.clone(),
            node.sockets.requests,
            node.sockets.respond,
            exit.clone(),
        );
        thread_hdls.extend(rpu.thread_hdls);

        let crdt = Arc::new(RwLock::new(Crdt::new(node.data)));
        crdt.write()
            .expect("'crdt' write lock before insert() in pub fn replicate")
            .insert(&entry_point);
        let window = streamer::default_window();
        let ncp = Ncp::new(
            crdt.clone(),
            window.clone(),
            node.sockets.gossip,
            node.sockets.gossip_send,
            exit.clone(),
        ).expect("Ncp::new");

        let tvu = Tvu::new(
            bank.clone(),
            entry_height,
            crdt.clone(),
            window.clone(),
            node.sockets.replicate,
            node.sockets.repair,
            node.sockets.retransmit,
            exit.clone(),
        );
        thread_hdls.extend(tvu.thread_hdls);
        thread_hdls.extend(ncp.thread_hdls);
        FullNode { thread_hdls }
    }
}
#[cfg(test)]
mod tests {
    use bank::Bank;
    use crdt::TestNode;
    use fullnode::FullNode;
    use mint::Mint;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    #[test]
    fn validator_exit() {
        let tn = TestNode::new();
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let exit = Arc::new(AtomicBool::new(false));
        let entry = tn.data.clone();
        let v = FullNode::new_validator(bank, 0, tn, entry, exit.clone());
        exit.store(true, Ordering::Relaxed);
        for t in v.thread_hdls {
            t.join().unwrap();
        }
    }
}
