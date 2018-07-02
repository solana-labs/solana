use bank::Bank;
use crdt::{ReplicatedData, TestNode};
use entry_writer;
use server::Server;
use std::fs::File;
use std::io::{stdin, stdout, BufReader};
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::JoinHandle;
//use std::time::Duration;

pub fn start(
    mut node: TestNode,
    leader: bool,
    infile: Option<String>,
    network_entry_for_validator: Option<SocketAddr>,
    outfile_for_leader: Option<String>,
    exit: Arc<AtomicBool>,
) -> Vec<JoinHandle<()>> {
    info!("creating bank...");
    let bank = Bank::default();
    let entry_height = if let Some(path) = infile {
        let f = File::open(path).unwrap();
        let mut r = BufReader::new(f);
        let entries = entry_writer::read_entries(&mut r).map(|e| e.expect("failed to parse entry"));
        info!("processing ledger...");
        bank.process_ledger(entries).expect("process_ledger")
    } else {
        let mut r = BufReader::new(stdin());
        let entries = entry_writer::read_entries(&mut r).map(|e| e.expect("failed to parse entry"));
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
        let server = if let Some(file) = outfile_for_leader {
            Server::new_leader(
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
            Server::new_leader(
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
        server.thread_hdls
    }
}
