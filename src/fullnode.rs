use bank::Bank;
use crdt::{ReplicatedData, TestNode};
use entry_writer;
use server::Server;
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::JoinHandle;
//use std::time::Duration;

pub fn start<'a, R: BufRead, W: Write + Send + 'static>(
    mut node: TestNode,
    leader: bool,
    infile: &'a mut R,
    network_entry_for_validator: Option<SocketAddr>,
    outfile_for_leader: Option<W>,
    exit: Arc<AtomicBool>,
) -> Vec<JoinHandle<()>> {
    eprintln!("creating bank...");
    let entries = entry_writer::read_entries(infile).map(|e| e.expect("failed to parse entry"));
    let bank = Bank::default();

    // entry_height is the network-wide agreed height of the ledger.
    //  initialize it from the input ledger
    eprintln!("processing ledger...");
    let entry_height = bank.process_ledger(entries).expect("process_ledger");
    eprintln!("processed {} ledger...", entry_height);

    eprintln!("creating networking stack...");

    let local_gossip_addr = node.sockets.gossip.local_addr().unwrap();
    let local_requests_addr = node.sockets.requests.local_addr().unwrap();
    info!(
        "starting... local gossip address: {} (advertising {})",
        local_gossip_addr, node.data.gossip_addr
    );
    if !leader {
        let testnet_addr = network_entry_for_validator.expect("validator requires entry");

        let network_entry_point = ReplicatedData::new_entry_point(testnet_addr);
        let s = Server::new_validator(
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
        s.thread_hdls
    } else {
        node.data.current_leader_id = node.data.id.clone();
        let f = outfile_for_leader.expect("outfile is needed for leader");
        let outfile: Box<Write + Send + 'static> = Box::new(f);
        let server = Server::new_leader(
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
            outfile,
        );
        info!(
            "leader ready... local request address: {} (advertising {})",
            local_requests_addr, node.data.requests_addr
        );
        server.thread_hdls
    }
}
