//! The `fullnode` module hosts all the fullnode microservices.

use bank::Bank;
use crdt::{Crdt, ReplicatedData, TestNode};
use entry_writer;
use ncp::Ncp;
use packet::BlobRecycler;
use rpu::Rpu;
use std::fs::File;
use std::io::Write;
use std::io::{stdin, stdout, BufReader};
use std::net::SocketAddr;
use std::net::UdpSocket;
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

impl FullNode {
    pub fn new(
        mut node: TestNode,
        leader: bool,
        infile: Option<String>,
        network_entry_for_validator: Option<SocketAddr>,
        outfile_for_leader: Option<String>,
        exit: Arc<AtomicBool>,
    ) -> FullNode {
        info!("creating bank...");
        let bank = Bank::default();
        let entry_height = if let Some(path) = infile {
            let f = File::open(path).unwrap();
            let mut r = BufReader::new(f);
            let entries =
                entry_writer::read_entries(&mut r).map(|e| e.expect("failed to parse entry"));
            info!("processing ledger...");
            bank.process_ledger(entries).expect("process_ledger")
        } else {
            let mut r = BufReader::new(stdin());
            let entries =
                entry_writer::read_entries(&mut r).map(|e| e.expect("failed to parse entry"));
            info!("processing ledger...");
            bank.process_ledger(entries).expect("process_ledger")
        };

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
        if !leader {
            let testnet_addr = network_entry_for_validator.expect("validator requires entry");

            let network_entry_point = ReplicatedData::new_entry_point(testnet_addr);
            let server = FullNode::new_validator(
                bank,
                entry_height,
                node.data.clone(),
                node.sockets.requests,
                node.sockets.respond,
                node.sockets.replicate,
                node.sockets.gossip,
                node.sockets.repair,
                network_entry_point,
                exit.clone(),
            );
            info!(
                "validator ready... local request address: {} (advertising {}) connected to: {}",
                local_requests_addr, node.data.requests_addr, testnet_addr
            );
            server
        } else {
            node.data.current_leader_id = node.data.id.clone();
            let server = if let Some(file) = outfile_for_leader {
                FullNode::new_leader(
                    bank,
                    entry_height,
                    //Some(Duration::from_millis(1000)),
                    None,
                    node.data.clone(),
                    node.sockets.requests,
                    node.sockets.transaction,
                    node.sockets.broadcast,
                    node.sockets.respond,
                    node.sockets.gossip,
                    exit.clone(),
                    File::create(file).expect("opening ledger file"),
                )
            } else {
                FullNode::new_leader(
                    bank,
                    entry_height,
                    //Some(Duration::from_millis(1000)),
                    None,
                    node.data.clone(),
                    node.sockets.requests,
                    node.sockets.transaction,
                    node.sockets.broadcast,
                    node.sockets.respond,
                    node.sockets.gossip,
                    exit.clone(),
                    stdout(),
                )
            };
            info!(
                "leader ready... local request address: {} (advertising {})",
                local_requests_addr, node.data.requests_addr
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
        me: ReplicatedData,
        requests_socket: UdpSocket,
        transactions_socket: UdpSocket,
        broadcast_socket: UdpSocket,
        respond_socket: UdpSocket,
        gossip_socket: UdpSocket,
        exit: Arc<AtomicBool>,
        writer: W,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(bank.clone(), requests_socket, respond_socket, exit.clone());
        thread_hdls.extend(rpu.thread_hdls);

        let blob_recycler = BlobRecycler::default();
        let tpu = Tpu::new(
            bank.clone(),
            tick_duration,
            transactions_socket,
            blob_recycler.clone(),
            exit.clone(),
            writer,
        );
        thread_hdls.extend(tpu.thread_hdls);

        let crdt = Arc::new(RwLock::new(Crdt::new(me)));
        let window = streamer::default_window();
        let gossip_send_socket = UdpSocket::bind("0.0.0.0:0").expect("bind 0");
        let ncp = Ncp::new(
            crdt.clone(),
            window.clone(),
            gossip_socket,
            gossip_send_socket,
            exit.clone(),
        ).expect("Ncp::new");
        thread_hdls.extend(ncp.thread_hdls);

        let t_broadcast = streamer::broadcaster(
            broadcast_socket,
            exit.clone(),
            crdt,
            window,
            entry_height,
            blob_recycler.clone(),
            tpu.blob_receiver,
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
        me: ReplicatedData,
        requests_socket: UdpSocket,
        respond_socket: UdpSocket,
        replicate_socket: UdpSocket,
        gossip_listen_socket: UdpSocket,
        repair_socket: UdpSocket,
        entry_point: ReplicatedData,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(bank.clone(), requests_socket, respond_socket, exit.clone());
        thread_hdls.extend(rpu.thread_hdls);

        let crdt = Arc::new(RwLock::new(Crdt::new(me)));
        crdt.write()
            .expect("'crdt' write lock before insert() in pub fn replicate")
            .insert(&entry_point);
        let window = streamer::default_window();
        let gossip_send_socket = UdpSocket::bind("0.0.0.0:0").expect("bind 0");
        let retransmit_socket = UdpSocket::bind("0.0.0.0:0").expect("bind 0");
        let ncp = Ncp::new(
            crdt.clone(),
            window.clone(),
            gossip_listen_socket,
            gossip_send_socket,
            exit.clone(),
        ).expect("Ncp::new");

        let tvu = Tvu::new(
            bank.clone(),
            entry_height,
            crdt.clone(),
            window.clone(),
            replicate_socket,
            repair_socket,
            retransmit_socket,
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
        let v = FullNode::new_validator(
            bank,
            0,
            tn.data.clone(),
            tn.sockets.requests,
            tn.sockets.respond,
            tn.sockets.replicate,
            tn.sockets.gossip,
            tn.sockets.repair,
            tn.data,
            exit.clone(),
        );
        exit.store(true, Ordering::Relaxed);
        for t in v.thread_hdls {
            t.join().unwrap();
        }
    }
}
