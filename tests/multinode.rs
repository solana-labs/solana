#[macro_use]
extern crate log;
extern crate bincode;
extern crate futures;
extern crate solana;

use futures::Future;
use solana::bank::Bank;
use solana::crdt::{Crdt, ReplicatedData};
use solana::logger;
use solana::mint::Mint;
use solana::server::Server;
use solana::signature::{KeyPair, KeyPairUtil, PublicKey};
use solana::streamer::default_window;
use solana::thin_client::ThinClient;
use solana::tvu::TestNode;
use std::io;
use std::io::sink;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::thread::sleep;
use std::time::Duration;

fn validator(
    leader: &ReplicatedData,
    exit: Arc<AtomicBool>,
    alice: &Mint,
    threads: &mut Vec<JoinHandle<()>>,
) {
    let validator = TestNode::new();
    let replicant_bank = Bank::new(&alice);
    let mut ts = Server::new_validator(
        replicant_bank,
        validator.data.clone(),
        validator.sockets.requests,
        validator.sockets.respond,
        validator.sockets.replicate,
        validator.sockets.gossip,
        leader.clone(),
        exit.clone(),
    );
    threads.append(&mut ts.thread_hdls);
}

fn converge(
    leader: &ReplicatedData,
    exit: Arc<AtomicBool>,
    num_nodes: usize,
    threads: &mut Vec<JoinHandle<()>>,
) -> Vec<ReplicatedData> {
    //lets spy on the network
    let mut spy = TestNode::new();
    let daddr = "0.0.0.0:0".parse().unwrap();
    let me = spy.data.id.clone();
    spy.data.replicate_addr = daddr;
    spy.data.requests_addr = daddr;
    let mut spy_crdt = Crdt::new(spy.data);
    spy_crdt.insert(&leader);
    spy_crdt.set_leader(leader.id);

    let spy_ref = Arc::new(RwLock::new(spy_crdt));
    let spy_window = default_window();
    let t_spy_listen = Crdt::listen(
        spy_ref.clone(),
        spy_window,
        spy.sockets.gossip,
        exit.clone(),
    );
    let t_spy_gossip = Crdt::gossip(spy_ref.clone(), exit.clone());
    //wait for the network to converge
    let mut converged = false;
    for _ in 0..30 {
        let num = spy_ref.read().unwrap().convergence();
        if num == num_nodes as u64 {
            converged = true;
            break;
        }
        sleep(Duration::new(1, 0));
    }
    assert!(converged);
    threads.push(t_spy_listen);
    threads.push(t_spy_gossip);
    let v: Vec<ReplicatedData> = spy_ref
        .read()
        .unwrap()
        .table
        .values()
        .into_iter()
        .filter(|x| x.id != me)
        .map(|x| x.clone())
        .collect();
    v.clone()
}

#[test]
fn test_multi_node() {
    logger::setup();
    const N: usize = 5;
    trace!("test_multi_accountant_stub");
    let leader = TestNode::new();
    let alice = Mint::new(10_000);
    let bob_pubkey = KeyPair::new().pubkey();
    let exit = Arc::new(AtomicBool::new(false));

    let leader_bank = Bank::new(&alice);
    let server = Server::new_leader(
        leader_bank,
        alice.last_id(),
        None,
        leader.data.clone(),
        leader.sockets.requests,
        leader.sockets.transaction,
        leader.sockets.broadcast,
        leader.sockets.respond,
        leader.sockets.gossip,
        exit.clone(),
        sink(),
    );

    let mut threads = server.thread_hdls;
    for _ in 0..N {
        validator(&leader.data, exit.clone(), &alice, &mut threads);
    }
    let servers = converge(&leader.data, exit.clone(), N + 2, &mut threads);
    //contains the leader addr as well
    assert_eq!(servers.len(), N + 1);
    //verify leader can do transfer
    let leader_balance = tx_and_retry_get_balance(&leader.data, &alice, &bob_pubkey).unwrap();
    assert_eq!(leader_balance, 500);
    //verify validator has the same balance
    let mut success = 0usize;
    for server in servers.iter() {
        let mut client = mk_client(server);
        if let Ok(bal) = client.poll_get_balance(&bob_pubkey) {
            trace!("validator balance {}", bal);
            if bal == leader_balance {
                success += 1;
            }
        }
    }
    assert_eq!(success, servers.len());
    exit.store(true, Ordering::Relaxed);
    for t in threads {
        t.join().unwrap();
    }
}

fn mk_client(leader: &ReplicatedData) -> ThinClient {
    let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    requests_socket
        .set_read_timeout(Some(Duration::new(1, 0)))
        .unwrap();
    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

    ThinClient::new(
        leader.requests_addr,
        requests_socket,
        leader.transactions_addr,
        transactions_socket,
    )
}

fn tx_and_retry_get_balance(
    leader: &ReplicatedData,
    alice: &Mint,
    bob_pubkey: &PublicKey,
) -> io::Result<i64> {
    let mut client = mk_client(leader);
    trace!("getting leader last_id");
    let last_id = client.get_last_id().wait().unwrap();
    info!("executing leader transer");
    let _sig = client
        .transfer(500, &alice.keypair(), *bob_pubkey, &last_id)
        .unwrap();
    client.poll_get_balance(bob_pubkey)
}
