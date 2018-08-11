#[macro_use]
extern crate log;
extern crate bincode;
extern crate chrono;
extern crate serde_json;
extern crate solana;

use solana::crdt::{Crdt, NodeInfo, TestNode};
use solana::entry::Entry;
use solana::fullnode::FullNode;
use solana::hash::Hash;
use solana::ledger::LedgerWriter;
use solana::logger;
use solana::mint::Mint;
use solana::ncp::Ncp;
use solana::result;
use solana::service::Service;
use solana::signature::{Keypair, KeypairUtil, Pubkey};
use solana::streamer::{default_window, WINDOW_SIZE};
use solana::thin_client::ThinClient;
use solana::timing::duration_as_s;
use std::cmp::max;
use std::env;
use std::fs::{copy, create_dir_all, remove_dir_all};
use std::net::UdpSocket;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::Builder;
use std::time::{Duration, Instant};

fn converge(leader: &NodeInfo, num_nodes: usize) -> Vec<NodeInfo> {
    //lets spy on the network
    let exit = Arc::new(AtomicBool::new(false));
    let mut spy = TestNode::new_localhost();
    let daddr = "0.0.0.0:0".parse().unwrap();
    let me = spy.data.id.clone();
    spy.data.contact_info.tvu = daddr;
    spy.data.contact_info.rpu = daddr;
    let mut spy_crdt = Crdt::new(spy.data).expect("Crdt::new");
    spy_crdt.insert(&leader);
    spy_crdt.set_leader(leader.id);
    let spy_ref = Arc::new(RwLock::new(spy_crdt));
    let spy_window = default_window();
    let ncp = Ncp::new(
        &spy_ref,
        spy_window,
        None,
        spy.sockets.gossip,
        spy.sockets.gossip_send,
        exit.clone(),
    ).unwrap();
    //wait for the network to converge
    let mut converged = false;
    let mut rv = vec![];
    for _ in 0..30 {
        let num = spy_ref.read().unwrap().convergence();
        let mut v: Vec<NodeInfo> = spy_ref
            .read()
            .unwrap()
            .table
            .values()
            .into_iter()
            .filter(|x| x.id != me)
            .filter(|x| Crdt::is_valid_address(x.contact_info.rpu))
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
    ncp.close().unwrap();
    rv
}

fn tmp_ledger_path(name: &str) -> String {
    let keypair = Keypair::new();

    format!("/tmp/tmp-ledger-{}-{}", name, keypair.pubkey())
}

fn genesis(name: &str, num: i64) -> (Mint, String) {
    let mint = Mint::new(num);

    let path = tmp_ledger_path(name);
    let mut writer = LedgerWriter::open(&path, true).unwrap();

    writer.write_entries(mint.create_entries()).unwrap();

    (mint, path)
}

fn tmp_copy_ledger(from: &str, name: &str) -> String {
    let tostr = tmp_ledger_path(name);

    {
        let to = Path::new(&tostr);
        let from = Path::new(&from);

        create_dir_all(to).unwrap();

        copy(from.join("data"), to.join("data")).unwrap();
        copy(from.join("index"), to.join("index")).unwrap();
    }

    tostr
}

fn make_tiny_test_entries(start_hash: Hash, num: usize) -> Vec<Entry> {
    let mut id = start_hash;
    let mut num_hashes = 0;
    (0..num)
        .map(|_| Entry::new_mut(&mut id, &mut num_hashes, vec![], false))
        .collect()
}

#[test]
fn test_multi_node_ledger_window() -> result::Result<()> {
    logger::setup();

    let leader_keypair = Keypair::new();
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.data.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (alice, leader_ledger_path) = genesis("multi_node_ledger_window", 10_000);
    ledger_paths.push(leader_ledger_path.clone());

    // make a copy at zero
    let zero_ledger_path = tmp_copy_ledger(&leader_ledger_path, "multi_node_ledger_window");
    ledger_paths.push(zero_ledger_path.clone());

    // write a bunch more ledger into leader's ledger, this should populate his window
    // and force him to respond to repair from the ledger window
    {
        let entries = make_tiny_test_entries(alice.last_id(), WINDOW_SIZE as usize * 2);
        let mut writer = LedgerWriter::open(&leader_ledger_path, false).unwrap();

        writer.write_entries(entries).unwrap();
    }

    let leader = FullNode::new(leader, true, &leader_ledger_path, leader_keypair, None);

    // Send leader some tokens to vote
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &leader_pubkey, None).unwrap();
    info!("leader balance {}", leader_balance);

    // start up another validator from zero, converge and then check
    // balances
    let keypair = Keypair::new();
    let validator = TestNode::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.data.clone();
    let validator = FullNode::new(
        validator,
        false,
        &zero_ledger_path,
        keypair,
        Some(leader_data.contact_info.ncp),
    );

    // contains the leader and new node
    info!("converging....");
    let _servers = converge(&leader_data, 2);

    // another transaction with leader
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, None).unwrap();
    info!("bob balance on leader {}", leader_balance);
    assert_eq!(leader_balance, 500);

    loop {
        let mut client = mk_client(&validator_data);
        let bal = client.poll_get_balance(&bob_pubkey)?;
        if bal == leader_balance {
            break;
        }
        sleep(Duration::from_millis(300));
        info!("bob balance on validator {}...", bal);
    }
    info!("done!");

    validator.close()?;
    leader.close()?;

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }

    Ok(())
}

#[test]
#[ignore]
fn test_multi_node_validator_catchup_from_zero() -> result::Result<()> {
    logger::setup();
    const N: usize = 5;
    trace!("test_multi_node_validator_catchup_from_zero");
    let leader_keypair = Keypair::new();
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.data.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (alice, leader_ledger_path) = genesis("multi_node_validator_catchup_from_zero", 10_000);
    ledger_paths.push(leader_ledger_path.clone());

    let zero_ledger_path = tmp_copy_ledger(
        &leader_ledger_path,
        "multi_node_validator_catchup_from_zero",
    );
    ledger_paths.push(zero_ledger_path.clone());

    let server = FullNode::new(leader, true, &leader_ledger_path, leader_keypair, None);

    // Send leader some tokens to vote
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &leader_pubkey, None).unwrap();
    info!("leader balance {}", leader_balance);

    let mut nodes = vec![server];
    for _ in 0..N {
        let keypair = Keypair::new();
        let validator = TestNode::new_localhost_with_pubkey(keypair.pubkey());
        let ledger_path = tmp_copy_ledger(
            &leader_ledger_path,
            "multi_node_validator_catchup_from_zero_validator",
        );
        ledger_paths.push(ledger_path.clone());

        let mut val = FullNode::new(
            validator,
            false,
            &ledger_path,
            keypair,
            Some(leader_data.contact_info.ncp),
        );
        nodes.push(val);
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
        info!("0server: {:x}", server.debug_id());
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
    // start up another validator from zero, converge and then check everyone's
    // balances
    let keypair = Keypair::new();
    let validator = TestNode::new_localhost_with_pubkey(keypair.pubkey());
    let val = FullNode::new(
        validator,
        false,
        &zero_ledger_path,
        keypair,
        Some(leader_data.contact_info.ncp),
    );
    nodes.push(val);
    //contains the leader and new node
    let servers = converge(&leader_data, N + 2);

    let mut leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, None).unwrap();
    info!("leader balance {}", leader_balance);
    loop {
        let mut client = mk_client(&leader_data);
        leader_balance = client.poll_get_balance(&bob_pubkey)?;
        if leader_balance == 1000 {
            break;
        }
        sleep(Duration::from_millis(300));
    }
    assert_eq!(leader_balance, 1000);

    for server in servers.iter() {
        let mut client = mk_client(server);
        info!("1server: {:x}", server.debug_id());
        for _ in 0..15 {
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

    for node in nodes {
        node.close()?;
    }
    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }

    Ok(())
}

#[test]
#[ignore]
fn test_multi_node_basic() {
    logger::setup();
    const N: usize = 5;
    trace!("test_multi_node_basic");
    let leader_keypair = Keypair::new();
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.data.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (alice, leader_ledger_path) = genesis("multi_node_basic", 10_000);
    ledger_paths.push(leader_ledger_path.clone());
    let server = FullNode::new(leader, true, &leader_ledger_path, leader_keypair, None);

    // Send leader some tokens to vote
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &leader_pubkey, None).unwrap();
    info!("leader balance {}", leader_balance);

    let mut nodes = vec![server];
    for _ in 0..N {
        let keypair = Keypair::new();
        let validator = TestNode::new_localhost_with_pubkey(keypair.pubkey());
        let ledger_path = tmp_copy_ledger(&leader_ledger_path, "multi_node_basic");
        ledger_paths.push(ledger_path.clone());
        let val = FullNode::new(
            validator,
            false,
            &ledger_path,
            keypair,
            Some(leader_data.contact_info.ncp),
        );
        nodes.push(val);
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

    for node in nodes {
        node.close().unwrap();
    }
    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }
}

#[test]
fn test_boot_validator_from_file() -> result::Result<()> {
    logger::setup();
    let leader_keypair = Keypair::new();
    let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
    let bob_pubkey = Keypair::new().pubkey();
    let (alice, leader_ledger_path) = genesis("boot_validator_from_file", 100_000);
    let mut ledger_paths = Vec::new();
    ledger_paths.push(leader_ledger_path.clone());

    let leader_data = leader.data.clone();
    let leader_fullnode = FullNode::new(leader, true, &leader_ledger_path, leader_keypair, None);
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, Some(500)).unwrap();
    assert_eq!(leader_balance, 500);
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, Some(1000)).unwrap();
    assert_eq!(leader_balance, 1000);

    let keypair = Keypair::new();
    let validator = TestNode::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.data.clone();
    let ledger_path = tmp_copy_ledger(&leader_ledger_path, "boot_validator_from_file");
    ledger_paths.push(ledger_path.clone());
    let val_fullnode = FullNode::new(
        validator,
        false,
        &ledger_path,
        keypair,
        Some(leader_data.contact_info.ncp),
    );
    let mut client = mk_client(&validator_data);
    let getbal = retry_get_balance(&mut client, &bob_pubkey, Some(leader_balance));
    assert!(getbal == Some(leader_balance));

    val_fullnode.close()?;
    leader_fullnode.close()?;
    for path in ledger_paths {
        remove_dir_all(path)?;
    }

    Ok(())
}

fn create_leader(ledger_path: &str) -> (NodeInfo, FullNode) {
    let leader_keypair = Keypair::new();
    let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.data.clone();
    let leader_fullnode = FullNode::new(leader, true, &ledger_path, leader_keypair, None);
    (leader_data, leader_fullnode)
}

#[test]
fn test_leader_restart_validator_start_from_old_ledger() -> result::Result<()> {
    // this test verifies that a freshly started leader makes his ledger available
    //    in the repair window to validators that are started with an older
    //    ledger (currently up to WINDOW_SIZE entries)
    logger::setup();

    let (alice, ledger_path) = genesis("leader_restart_validator_start_from_old_ledger", 100_000);
    let bob_pubkey = Keypair::new().pubkey();

    let (leader_data, leader_fullnode) = create_leader(&ledger_path);

    // lengthen the ledger
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, Some(500)).unwrap();
    assert_eq!(leader_balance, 500);

    // create a "stale" ledger by copying current ledger
    let stale_ledger_path = tmp_copy_ledger(
        &ledger_path,
        "leader_restart_validator_start_from_old_ledger",
    );

    // restart the leader
    leader_fullnode.close()?;
    let (leader_data, leader_fullnode) = create_leader(&ledger_path);

    // lengthen the ledger
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, Some(1000)).unwrap();
    assert_eq!(leader_balance, 1000);

    // restart the leader
    leader_fullnode.close()?;
    let (leader_data, leader_fullnode) = create_leader(&ledger_path);

    // start validator from old ledger
    let keypair = Keypair::new();
    let validator = TestNode::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.data.clone();

    let val_fullnode = FullNode::new(
        validator,
        false,
        &stale_ledger_path,
        keypair,
        Some(leader_data.contact_info.ncp),
    );

    // trigger broadcast, validator should catch up from leader, whose window contains
    //   the entries missing from the stale ledger
    //   send requests so the validator eventually sees a gap and requests a repair
    let mut expected = 1500;
    let mut client = mk_client(&validator_data);
    for _ in 0..10 {
        let leader_balance =
            send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, Some(expected))
                .unwrap();
        assert_eq!(leader_balance, expected);
        let getbal = retry_get_balance(&mut client, &bob_pubkey, Some(leader_balance));
        if getbal == Some(leader_balance) {
            break;
        }
        expected += 500;
    }
    let getbal = retry_get_balance(&mut client, &bob_pubkey, Some(expected));
    assert_eq!(getbal, Some(expected));

    val_fullnode.close()?;
    leader_fullnode.close()?;
    remove_dir_all(ledger_path)?;
    remove_dir_all(stale_ledger_path)?;

    Ok(())
}

//TODO: this test will run a long time so it's disabled for CI
#[test]
#[ignore]
fn test_multi_node_dynamic_network() {
    logger::setup();
    let key = "SOLANA_DYNAMIC_NODES";
    let num_nodes: usize = match env::var(key) {
        Ok(val) => val
            .parse()
            .expect(&format!("env var {} is not parse-able as usize", key)),
        Err(_) => 230,
    };

    let purge_key = "SOLANA_DYNAMIC_NODES_PURGE_LAG";
    let purge_lag: usize = match env::var(purge_key) {
        Ok(val) => val
            .parse()
            .expect(&format!("env var {} is not parse-able as usize", purge_key)),
        Err(_) => std::usize::MAX,
    };

    let leader_keypair = Keypair::new();
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = TestNode::new_localhost_with_pubkey(leader_keypair.pubkey());
    let bob_pubkey = Keypair::new().pubkey();
    let (alice, leader_ledger_path) = genesis("multi_node_dynamic_network", 10_000_000);

    let mut ledger_paths = Vec::new();
    ledger_paths.push(leader_ledger_path.clone());

    let alice_arc = Arc::new(RwLock::new(alice));
    let leader_data = leader.data.clone();

    let server =
        FullNode::new_without_sigverify(leader, true, &leader_ledger_path, leader_keypair, None);

    // Send leader some tokens to vote
    let leader_balance = send_tx_and_retry_get_balance(
        &leader_data,
        &alice_arc.read().unwrap(),
        &leader_pubkey,
        None,
    ).unwrap();
    info!("leader balance {}", leader_balance);

    info!("{:x} LEADER", leader_data.debug_id());
    let leader_balance = retry_send_tx_and_retry_get_balance(
        &leader_data,
        &alice_arc.read().unwrap(),
        &bob_pubkey,
        Some(500),
    ).unwrap();
    assert_eq!(leader_balance, 500);
    let leader_balance = retry_send_tx_and_retry_get_balance(
        &leader_data,
        &alice_arc.read().unwrap(),
        &bob_pubkey,
        Some(1000),
    ).unwrap();
    assert_eq!(leader_balance, 1000);

    let t1: Vec<_> = (0..num_nodes)
        .into_iter()
        .map(|n| {
            let leader_data = leader_data.clone();
            let alice_clone = alice_arc.clone();
            Builder::new()
                .name("keypair-thread".to_string())
                .spawn(move || {
                    info!("Spawned thread {}", n);
                    let keypair = Keypair::new();
                    //send some tokens to the new validator
                    let bal = retry_send_tx_and_retry_get_balance(
                        &leader_data,
                        &alice_clone.read().unwrap(),
                        &keypair.pubkey(),
                        Some(500),
                    );
                    assert_eq!(bal, Some(500));
                    info!("sent balance to[{}/{}] {}", n, num_nodes, keypair.pubkey());
                    keypair
                })
                .unwrap()
        })
        .collect();

    info!("Waiting for keypairs to be created");
    let keypairs: Vec<_> = t1.into_iter().map(|t| t.join().unwrap()).collect();
    info!("keypairs created");

    let t2: Vec<_> = keypairs
        .into_iter()
        .map(|keypair| {
            let leader_data = leader_data.clone();
            let ledger_path = tmp_copy_ledger(&leader_ledger_path, "multi_node_dynamic_network");
            ledger_paths.push(ledger_path.clone());
            Builder::new()
                .name("validator-launch-thread".to_string())
                .spawn(move || {
                    let validator = TestNode::new_localhost_with_pubkey(keypair.pubkey());
                    let rd = validator.data.clone();
                    info!("starting {} {:x}", keypair.pubkey(), rd.debug_id());
                    let val = FullNode::new_without_sigverify(
                        validator,
                        false,
                        &ledger_path,
                        keypair,
                        Some(leader_data.contact_info.ncp),
                    );
                    (rd, val)
                })
                .unwrap()
        })
        .collect();

    let mut validators: Vec<_> = t2.into_iter().map(|t| t.join().unwrap()).collect();

    let now = Instant::now();
    let mut consecutive_success = 0;
    let mut failures = 0;
    let mut max_distance_increase = 0i64;
    for i in 0..num_nodes {
        //verify leader can do transfer
        let expected = ((i + 3) * 500) as i64;
        let leader_balance = retry_send_tx_and_retry_get_balance(
            &leader_data,
            &alice_arc.read().unwrap(),
            &bob_pubkey,
            Some(expected),
        ).unwrap();
        if leader_balance != expected {
            info!(
                "leader dropped transaction {} {:?} {:?}",
                i, leader_balance, expected
            );
        }
        //verify all validators have the same balance
        {
            let mut success = 0usize;
            let mut max_distance = 0i64;
            let mut total_distance = 0i64;
            let mut num_nodes_behind = 0i64;
            validators.retain(|server| {
                let mut retain_me = true;
                let mut client = mk_client(&server.0);
                trace!("{:x} {} get_balance start", server.0.debug_id(), i);
                let getbal = retry_get_balance(&mut client, &bob_pubkey, Some(leader_balance));
                trace!(
                    "{:x} {} get_balance: {:?} leader_balance: {}",
                    server.0.debug_id(),
                    i,
                    getbal,
                    leader_balance
                );
                let bal = getbal.unwrap_or(0);
                let distance = (leader_balance - bal) / 500;
                max_distance = max(distance, max_distance);
                total_distance += distance;
                if distance > max_distance_increase {
                    info!("Node {:x} is behind by {}", server.0.debug_id(), distance);
                    max_distance_increase = distance;
                    if max_distance_increase as u64 > purge_lag as u64 {
                        server.1.exit();
                        info!("Node {:x} is exiting", server.0.debug_id());
                        retain_me = false;
                    }
                }
                if distance > 0 {
                    num_nodes_behind += 1;
                }
                if let Some(bal) = getbal {
                    if bal == leader_balance {
                        success += 1;
                    }
                }
                retain_me
            });
            if num_nodes_behind != 0 {
                info!("{} nodes are lagging behind leader", num_nodes_behind);
            }
            info!(
                "SUCCESS[{}] {} out of {} distance: {} max_distance: {}",
                i,
                success,
                validators.len(),
                total_distance,
                max_distance
            );
            if success == validators.len() && total_distance == 0 {
                consecutive_success += 1;
            } else {
                consecutive_success = 0;
                failures += 1;
            }
            if consecutive_success == 10 {
                break;
            }
        }
    }
    info!(
        "Took {} s to converge total failures: {}",
        duration_as_s(&now.elapsed()),
        failures
    );
    assert_eq!(consecutive_success, 10);
    for (_, node) in &validators {
        node.exit();
    }
    server.exit();
    for (_, node) in validators {
        node.join().unwrap();
    }
    server.join().unwrap();

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }
}

fn mk_client(leader: &NodeInfo) -> ThinClient {
    let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    requests_socket
        .set_read_timeout(Some(Duration::new(1, 0)))
        .unwrap();
    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    assert!(Crdt::is_valid_address(leader.contact_info.rpu));
    assert!(Crdt::is_valid_address(leader.contact_info.tpu));
    ThinClient::new(
        leader.contact_info.rpu,
        requests_socket,
        leader.contact_info.tpu,
        transactions_socket,
    )
}

fn retry_get_balance(
    client: &mut ThinClient,
    bob_pubkey: &Pubkey,
    expected: Option<i64>,
) -> Option<i64> {
    const LAST: usize = 30;
    for run in 0..(LAST + 1) {
        let out = client.poll_get_balance(bob_pubkey);
        if expected.is_none() || run == LAST {
            return out.ok().clone();
        }
        trace!("retry_get_balance[{}] {:?} {:?}", run, out, expected);
        if let (Some(e), Ok(o)) = (expected, out) {
            if o == e {
                return Some(o);
            }
        }
    }
    None
}

fn send_tx_and_retry_get_balance(
    leader: &NodeInfo,
    alice: &Mint,
    bob_pubkey: &Pubkey,
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

fn retry_send_tx_and_retry_get_balance(
    leader: &NodeInfo,
    alice: &Mint,
    bob_pubkey: &Pubkey,
    expected: Option<i64>,
) -> Option<i64> {
    let mut client = mk_client(leader);
    trace!("getting leader last_id");
    let last_id = client.get_last_id();
    info!("executing leader transfer");
    const LAST: usize = 30;
    for run in 0..(LAST + 1) {
        let _sig = client
            .transfer(500, &alice.keypair(), *bob_pubkey, &last_id)
            .unwrap();
        let out = client.poll_get_balance(bob_pubkey);
        if expected.is_none() || run == LAST {
            return out.ok().clone();
        }
        trace!(
            "retry_send_tx_and_retry_get_balance[{}] {:?} {:?}",
            run,
            out,
            expected
        );
        if let (Some(e), Ok(o)) = (expected, out) {
            if o == e {
                return Some(o);
            }
        }
        sleep(Duration::from_millis(20));
    }
    None
}
