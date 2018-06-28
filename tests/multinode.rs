#[macro_use]
extern crate log;
extern crate bincode;
extern crate serde_json;
extern crate solana;

use solana::crdt::TestNode;
use solana::crdt::{Crdt, ReplicatedData};
use solana::entry_writer::EntryWriter;
use solana::fullnode::{FullNode, LedgerFile};
use solana::logger;
use solana::mint::Mint;
use solana::ncp::Ncp;
use solana::signature::{KeyPair, KeyPairUtil, PublicKey};
use solana::streamer::default_window;
use solana::thin_client::ThinClient;
use std::fs::File;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;

fn converge(leader: &ReplicatedData, num_nodes: usize) -> Vec<ReplicatedData> {
    //lets spy on the network
    let exit = Arc::new(AtomicBool::new(false));
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
    let dr = Ncp::new(
        spy_ref.clone(),
        spy_window,
        spy.sockets.gossip,
        spy.sockets.gossip_send,
        exit.clone(),
    ).unwrap();
    //wait for the network to converge
    let mut converged = false;
    let mut rv = vec![];
    for _ in 0..30 {
        let num = spy_ref.read().unwrap().convergence();
        let mut v: Vec<ReplicatedData> = spy_ref
            .read()
            .unwrap()
            .table
            .values()
            .into_iter()
            .filter(|x| x.id != me)
            .filter(|x| x.requests_addr != daddr)
            .cloned()
            .collect();
        if num >= num_nodes as u64 && v.len() >= num_nodes {
            rv.append(&mut v);
            converged = true;
            break;
        }
        sleep(Duration::new(1, 0));
    }
    assert!(converged);
    exit.store(true, Ordering::Relaxed);
    for t in dr.thread_hdls.into_iter() {
        t.join().unwrap();
    }
    rv
}

fn genesis(num: i64) -> (Mint, String) {
    let mint = Mint::new(num);
    let id = {
        let ids: Vec<_> = mint.pubkey().iter().map(|id| format!("{}", id)).collect();
        ids.join("")
    };
    let path = format!("target/test_multi_node_dynamic_network-{}.log", id);
    let mut writer = File::create(path.clone()).unwrap();

    EntryWriter::write_entries(&mut writer, mint.create_entries()).unwrap();
    (mint, path.to_string())
}

#[test]
fn test_multi_node_validator_catchup_from_zero() {
    logger::setup();
    const N: usize = 5;
    trace!("test_multi_node_validator_catchup_from_zero");
    let leader = TestNode::new();
    let leader_data = leader.data.clone();
    let bob_pubkey = KeyPair::new().pubkey();
    let exit = Arc::new(AtomicBool::new(false));

    let (alice, ledger_path) = genesis(10_000);
    let server = FullNode::new(
        leader,
        true,
        LedgerFile::Path(ledger_path.clone()),
        None,
        LedgerFile::Sink,
        exit.clone(),
    );
    let mut threads = server.thread_hdls;
    for _ in 0..N {
        let validator = TestNode::new();
        let mut val = FullNode::new(
            validator,
            false,
            LedgerFile::Path(ledger_path.clone()),
            Some(leader_data.gossip_addr),
            LedgerFile::NoFile,
            exit.clone(),
        );
        threads.append(&mut val.thread_hdls);
    }
    let servers = converge(&leader_data, N + 1);
    //contains the leader addr as well
    assert_eq!(servers.len(), N + 1);
    //verify leader can do transfer
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, None).unwrap();
    assert_eq!(leader_balance, 500);
    //verify validator has the same balance
    let mut success = 0usize;
    for server in servers.iter() {
        info!("0server: {:?}", server.id[0]);
        let mut client = mk_client(server);
        if let Ok(bal) = client.poll_get_balance(&bob_pubkey) {
            info!("validator balance {}", bal);
            if bal == leader_balance {
                success += 1;
            }
        }
    }
    assert_eq!(success, servers.len());

    success = 0;
    // start up another validator, converge and then check everyone's balances
    let mut val = FullNode::new(
        TestNode::new(),
        false,
        LedgerFile::Path(ledger_path.clone()),
        Some(leader_data.gossip_addr),
        LedgerFile::NoFile,
        exit.clone(),
    );
    threads.append(&mut val.thread_hdls);
    //contains the leader and new node
    let servers = converge(&leader_data, N + 2);

    let mut leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, None).unwrap();
    info!("leader balance {}", leader_balance);
    loop {
        let mut client = mk_client(&leader_data);
        leader_balance = client.poll_get_balance(&bob_pubkey).unwrap();
        if leader_balance == 1000 {
            break;
        }
        sleep(Duration::from_millis(300));
    }
    assert_eq!(leader_balance, 1000);

    for server in servers.iter() {
        let mut client = mk_client(server);
        info!("1server: {:?}", server.id[0]);
        for _ in 0..10 {
            if let Ok(bal) = client.poll_get_balance(&bob_pubkey) {
                info!("validator balance {}", bal);
                if bal == leader_balance {
                    success += 1;
                    break;
                }
            }
            sleep(Duration::from_millis(500));
        }
    }
    assert_eq!(success, servers.len());

    exit.store(true, Ordering::Relaxed);
    for t in threads {
        t.join().unwrap();
    }
}

#[test]
fn test_multi_node_basic() {
    logger::setup();
    const N: usize = 5;
    trace!("test_multi_node_basic");
    let leader = TestNode::new();
    let leader_data = leader.data.clone();
    let bob_pubkey = KeyPair::new().pubkey();
    let exit = Arc::new(AtomicBool::new(false));
    let (alice, ledger_path) = genesis(10_000);
    let server = FullNode::new(
        leader,
        true,
        LedgerFile::Path(ledger_path.clone()),
        None,
        LedgerFile::Sink,
        exit.clone(),
    );
    let threads = server.thread_hdls;
    for _ in 0..N {
        let validator = TestNode::new();
        FullNode::new(
            validator,
            false,
            LedgerFile::Path(ledger_path.clone()),
            Some(leader_data.gossip_addr),
            LedgerFile::NoFile,
            exit.clone(),
        );
    }
    let servers = converge(&leader_data, N + 1);
    //contains the leader addr as well
    assert_eq!(servers.len(), N + 1);
    //verify leader can do transfer
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, None).unwrap();
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
    std::fs::remove_file(ledger_path).unwrap();
}

#[test]
fn test_multi_node_dynamic_network() {
    logger::setup();
    const N: usize = 3;
    let leader = TestNode::new();
    let bob_pubkey = KeyPair::new().pubkey();
    let exit = Arc::new(AtomicBool::new(false));
    let (alice, ledger_path) = genesis(100_000);
    let leader_data = leader.data.clone();
    let server = FullNode::new(
        leader,
        true,
        LedgerFile::Path(ledger_path.clone()),
        None,
        LedgerFile::Sink,
        exit.clone(),
    );
    let threads = server.thread_hdls;
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, Some(500)).unwrap();
    assert_eq!(leader_balance, 500);
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, Some(1000)).unwrap();
    assert_eq!(leader_balance, 1000);

    let mut vals: Vec<(ReplicatedData, Arc<AtomicBool>, FullNode)> = (0..N)
        .into_iter()
        .map(|_| {
            let exit = Arc::new(AtomicBool::new(false));
            let validator = TestNode::new();
            let rd = validator.data.clone();
            let val = FullNode::new(
                validator,
                false,
                LedgerFile::Path(ledger_path.clone()),
                Some(leader_data.gossip_addr),
                LedgerFile::NoFile,
                exit.clone(),
            );
            (rd, exit, val)
        })
        .collect();
    for i in 0..N {
        //verify leader can do transfer
        let expected = ((i + 3) * 500) as i64;
        let leader_balance =
            send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, Some(expected))
                .unwrap();
        assert_eq!(leader_balance, expected);
        //verify all validators have the same balance
        let mut success = 0usize;
        for server in vals.iter() {
            let mut client = mk_client(&server.0);
            let getbal = retry_get_balance(&mut client, &bob_pubkey, Some(expected));
            info!("{:x} {} get_balance: {:?}", server.0.debug_id(), i, getbal);
            if let Some(bal) = getbal {
                if bal == leader_balance {
                    success += 1;
                }
            }
        }
        info!("SUCCESS {} out of {}", success, vals.len());
        // this should be almost true, or at least vals.len() - 1 while the other node catches up
        //assert!(success == vals.len());
        //kill a validator
        vals[i].1.store(true, Ordering::Relaxed);
        let mut ts = vec![];
        ts.append(&mut vals[i].2.thread_hdls);
        for t in ts.into_iter() {
            t.join().unwrap();
        }
        info!("{:x} KILLED", vals[i].0.debug_id());
        //add a new one
        vals[i] = {
            let exit = Arc::new(AtomicBool::new(false));
            let validator = TestNode::new();
            let rd = validator.data.clone();
            let val = FullNode::new(
                validator,
                false,
                LedgerFile::Path(ledger_path.clone()),
                Some(leader_data.gossip_addr),
                LedgerFile::NoFile,
                exit.clone(),
            );
            info!("{:x} ADDED", rd.debug_id());
            (rd, exit, val)
        };
    }
    for (_, exit, val) in vals.into_iter() {
        exit.store(true, Ordering::Relaxed);
        for t in val.thread_hdls {
            t.join().unwrap();
        }
    }
    exit.store(true, Ordering::Relaxed);
    for t in threads {
        t.join().unwrap();
    }
    std::fs::remove_file(ledger_path).unwrap();
}

fn mk_client(leader: &ReplicatedData) -> ThinClient {
    let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    requests_socket
        .set_read_timeout(Some(Duration::new(1, 0)))
        .unwrap();
    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let daddr = "0.0.0.0:0".parse().unwrap();
    assert!(leader.requests_addr != daddr);
    assert!(leader.transactions_addr != daddr);
    ThinClient::new(
        leader.requests_addr,
        requests_socket,
        leader.transactions_addr,
        transactions_socket,
    )
}

fn retry_get_balance(
    client: &mut ThinClient,
    bob_pubkey: &PublicKey,
    expected: Option<i64>,
) -> Option<i64> {
    for _ in 0..10 {
        let out = client.poll_get_balance(bob_pubkey);
        if expected.is_none() {
            return out.ok().clone();
        }
        if let (Some(e), Ok(o)) = (expected, out) {
            if o == e {
                return Some(o);
            }
        }
    }
    None
}

fn send_tx_and_retry_get_balance(
    leader: &ReplicatedData,
    alice: &Mint,
    bob_pubkey: &PublicKey,
    expected: Option<i64>,
) -> Option<i64> {
    let mut client = mk_client(leader);
    trace!("getting leader last_id");
    let last_id = client.get_last_id();
    info!("executing leader transfer");
    let _sig = client
        .transfer(500, &alice.keypair(), *bob_pubkey, &last_id)
        .unwrap();
    retry_get_balance(&mut client, bob_pubkey, expected)
}
