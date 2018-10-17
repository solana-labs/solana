#[macro_use]
extern crate log;
extern crate bincode;
extern crate chrono;
extern crate serde_json;
extern crate solana;
extern crate solana_program_interface;

use solana::cluster_info::{ClusterInfo, Node, NodeInfo};
use solana::entry::Entry;
use solana::fullnode::{Fullnode, FullnodeReturnType};
use solana::hash::Hash;
use solana::leader_scheduler::{make_active_set_entries, LeaderScheduler, LeaderSchedulerConfig};
use solana::ledger::{create_sample_ledger, read_ledger, LedgerWriter};
use solana::logger;
use solana::mint::Mint;
use solana::ncp::Ncp;
use solana::result;
use solana::service::Service;
use solana::signature::{Keypair, KeypairUtil};
use solana::system_transaction::SystemTransaction;
use solana::thin_client::ThinClient;
use solana::timing::{duration_as_ms, duration_as_s};
use solana::transaction::Transaction;
use solana::window::{default_window, WINDOW_SIZE};
use solana_program_interface::pubkey::Pubkey;
use std::collections::{HashSet, VecDeque};
use std::env;
use std::fs::{copy, create_dir_all, remove_dir_all};
use std::net::UdpSocket;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::{Duration, Instant};

fn make_spy_node(leader: &NodeInfo) -> (Ncp, Arc<RwLock<ClusterInfo>>, Pubkey) {
    let exit = Arc::new(AtomicBool::new(false));
    let mut spy = Node::new_localhost();
    let me = spy.info.id.clone();
    let daddr = "0.0.0.0:0".parse().unwrap();
    spy.info.contact_info.tvu = daddr;
    spy.info.contact_info.rpu = spy.sockets.transaction[0].local_addr().unwrap();
    let mut spy_cluster_info = ClusterInfo::new(spy.info).expect("ClusterInfo::new");
    spy_cluster_info.insert(&leader);
    spy_cluster_info.set_leader(leader.id);
    let spy_cluster_info_ref = Arc::new(RwLock::new(spy_cluster_info));
    let spy_window = Arc::new(RwLock::new(default_window()));
    let ncp = Ncp::new(
        &spy_cluster_info_ref,
        spy_window,
        None,
        spy.sockets.gossip,
        exit.clone(),
    );

    (ncp, spy_cluster_info_ref, me)
}

fn converge(leader: &NodeInfo, num_nodes: usize) -> Vec<NodeInfo> {
    //lets spy on the network
    let (ncp, spy_ref, _) = make_spy_node(leader);

    //wait for the network to converge
    let mut converged = false;
    let mut rv = vec![];
    for _ in 0..30 {
        let num = spy_ref.read().unwrap().convergence();
        let mut v = spy_ref.read().unwrap().get_valid_peers();
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
    use std::env;
    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
    let keypair = Keypair::new();

    format!("{}/tmp/ledger-{}-{}", out_dir, name, keypair.pubkey());
}

fn genesis(name: &str, num: i64) -> (Mint, String, Vec<Entry>) {
    let mint = Mint::new(num);

    let path = tmp_ledger_path(name);
    let mut writer = LedgerWriter::open(&path, true).unwrap();

    let entries = mint.create_entries();
    writer.write_entries(entries.clone()).unwrap();

    (mint, path, entries)
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
        .map(|_| Entry::new_mut(&mut id, &mut num_hashes, vec![]))
        .collect()
}

#[test]
fn test_multi_node_ledger_window() -> result::Result<()> {
    logger::setup();

    let leader_keypair = Keypair::new();
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (alice, leader_ledger_path, _) = genesis("multi_node_ledger_window", 10_000);
    ledger_paths.push(leader_ledger_path.clone());

    // make a copy at zero
    let zero_ledger_path = tmp_copy_ledger(&leader_ledger_path, "multi_node_ledger_window");
    ledger_paths.push(zero_ledger_path.clone());

    // write a bunch more ledger into leader's ledger, this should populate his window
    // and force him to respond to repair from the ledger window
    {
        let entries = make_tiny_test_entries(alice.last_id(), WINDOW_SIZE as usize);
        let mut writer = LedgerWriter::open(&leader_ledger_path, false).unwrap();

        writer.write_entries(entries).unwrap();
    }

    let leader = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
    );

    // Send leader some tokens to vote
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &leader_pubkey, 500, None).unwrap();
    info!("leader balance {}", leader_balance);

    // start up another validator from zero, converge and then check
    // balances
    let keypair = Keypair::new();
    let validator_pubkey = keypair.pubkey().clone();
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.info.clone();
    let validator = Fullnode::new(
        validator,
        &zero_ledger_path,
        keypair,
        Some(leader_data.contact_info.ncp),
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
    );

    // Send validator some tokens to vote
    let validator_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &validator_pubkey, 500, None).unwrap();
    info!("leader balance {}", validator_balance);

    // contains the leader and new node
    info!("converging....");
    let _servers = converge(&leader_data, 2);
    info!("converged.");

    // another transaction with leader
    let bob_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 1, None).unwrap();
    info!("bob balance on leader {}", bob_balance);
    let mut checks = 1;
    loop {
        let mut client = mk_client(&validator_data);
        let bal = client.poll_get_balance(&bob_pubkey);
        info!(
            "bob balance on validator {:?} after {} checks...",
            bal, checks
        );
        if bal.unwrap_or(0) == bob_balance {
            break;
        }
        checks += 1;
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
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (alice, leader_ledger_path, _) = genesis("multi_node_validator_catchup_from_zero", 10_000);
    ledger_paths.push(leader_ledger_path.clone());

    let zero_ledger_path = tmp_copy_ledger(
        &leader_ledger_path,
        "multi_node_validator_catchup_from_zero",
    );
    ledger_paths.push(zero_ledger_path.clone());

    let server = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
    );

    // Send leader some tokens to vote
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &leader_pubkey, 500, None).unwrap();
    info!("leader balance {}", leader_balance);

    let mut nodes = vec![server];
    for _ in 0..N {
        let keypair = Keypair::new();
        let validator_pubkey = keypair.pubkey().clone();
        let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
        let ledger_path = tmp_copy_ledger(
            &leader_ledger_path,
            "multi_node_validator_catchup_from_zero_validator",
        );
        ledger_paths.push(ledger_path.clone());

        // Send each validator some tokens to vote
        let validator_balance =
            send_tx_and_retry_get_balance(&leader_data, &alice, &validator_pubkey, 500, None)
                .unwrap();
        info!("validator balance {}", validator_balance);

        let mut val = Fullnode::new(
            validator,
            &ledger_path,
            keypair,
            Some(leader_data.contact_info.ncp),
            false,
            LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        );
        nodes.push(val);
    }
    let servers = converge(&leader_data, N + 1);
    //contains the leader addr as well
    assert_eq!(servers.len(), N + 1);
    //verify leader can do transfer
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, None).unwrap();
    assert_eq!(leader_balance, 500);
    //verify validator has the same balance
    let mut success = 0usize;
    for server in servers.iter() {
        info!("0server: {}", server.id);
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
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let val = Fullnode::new(
        validator,
        &zero_ledger_path,
        keypair,
        Some(leader_data.contact_info.ncp),
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
    );
    nodes.push(val);
    //contains the leader and new node
    let servers = converge(&leader_data, N + 2);

    let mut leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, None).unwrap();
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
        info!("1server: {}", server.id);
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
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (alice, leader_ledger_path, _) = genesis("multi_node_basic", 10_000);
    ledger_paths.push(leader_ledger_path.clone());
    let server = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
    );

    // Send leader some tokens to vote
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &leader_pubkey, 500, None).unwrap();
    info!("leader balance {}", leader_balance);

    let mut nodes = vec![server];
    for _ in 0..N {
        let keypair = Keypair::new();
        let validator_pubkey = keypair.pubkey().clone();
        let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
        let ledger_path = tmp_copy_ledger(&leader_ledger_path, "multi_node_basic");
        ledger_paths.push(ledger_path.clone());

        // Send each validator some tokens to vote
        let validator_balance =
            send_tx_and_retry_get_balance(&leader_data, &alice, &validator_pubkey, 500, None)
                .unwrap();
        info!("validator balance {}", validator_balance);

        let val = Fullnode::new(
            validator,
            &ledger_path,
            keypair,
            Some(leader_data.contact_info.ncp),
            false,
            LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        );
        nodes.push(val);
    }
    let servers = converge(&leader_data, N + 1);
    //contains the leader addr as well
    assert_eq!(servers.len(), N + 1);
    //verify leader can do transfer
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, None).unwrap();
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
    let leader_pubkey = leader_keypair.pubkey();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let bob_pubkey = Keypair::new().pubkey();
    let (alice, leader_ledger_path, _) = genesis("boot_validator_from_file", 100_000);
    let mut ledger_paths = Vec::new();
    ledger_paths.push(leader_ledger_path.clone());

    let leader_data = leader.info.clone();
    let leader_fullnode = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
    );
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(500)).unwrap();
    assert_eq!(leader_balance, 500);
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(1000)).unwrap();
    assert_eq!(leader_balance, 1000);

    let keypair = Keypair::new();
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.info.clone();
    let ledger_path = tmp_copy_ledger(&leader_ledger_path, "boot_validator_from_file");
    ledger_paths.push(ledger_path.clone());
    let val_fullnode = Fullnode::new(
        validator,
        &ledger_path,
        keypair,
        Some(leader_data.contact_info.ncp),
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
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

fn create_leader(ledger_path: &str) -> (NodeInfo, Fullnode) {
    let leader_keypair = Keypair::new();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let leader_fullnode = Fullnode::new(
        leader,
        &ledger_path,
        leader_keypair,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_data.id),
    );
    (leader_data, leader_fullnode)
}

#[test]
fn test_leader_restart_validator_start_from_old_ledger() -> result::Result<()> {
    // this test verifies that a freshly started leader makes his ledger available
    //    in the repair window to validators that are started with an older
    //    ledger (currently up to WINDOW_SIZE entries)
    logger::setup();

    let (alice, ledger_path, _) = genesis(
        "leader_restart_validator_start_from_old_ledger",
        100_000 + 500 * solana::window_service::MAX_REPAIR_BACKOFF as i64,
    );
    let bob_pubkey = Keypair::new().pubkey();

    let (leader_data, leader_fullnode) = create_leader(&ledger_path);

    // lengthen the ledger
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(500)).unwrap();
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
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(1000)).unwrap();
    assert_eq!(leader_balance, 1000);

    // restart the leader
    leader_fullnode.close()?;
    let (leader_data, leader_fullnode) = create_leader(&ledger_path);

    // start validator from old ledger
    let keypair = Keypair::new();
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.info.clone();

    let val_fullnode = Fullnode::new(
        validator,
        &stale_ledger_path,
        keypair,
        Some(leader_data.contact_info.ncp),
        false,
        LeaderScheduler::from_bootstrap_leader(leader_data.id),
    );

    // trigger broadcast, validator should catch up from leader, whose window contains
    //   the entries missing from the stale ledger
    //   send requests so the validator eventually sees a gap and requests a repair
    let mut expected = 1500;
    let mut client = mk_client(&validator_data);
    for _ in 0..solana::window_service::MAX_REPAIR_BACKOFF {
        let leader_balance =
            send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(expected))
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
    assert!(cfg!(feature = "test"));
    let key = "SOLANA_DYNAMIC_NODES";
    let num_nodes: usize = match env::var(key) {
        Ok(val) => val
            .parse()
            .expect(&format!("env var {} is not parse-able as usize", key)),
        Err(_) => 120,
    };

    let leader_keypair = Keypair::new();
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let bob_pubkey = Keypair::new().pubkey();
    let (alice, leader_ledger_path, _) = genesis("multi_node_dynamic_network", 10_000_000);

    let mut ledger_paths = Vec::new();
    ledger_paths.push(leader_ledger_path.clone());

    let alice_arc = Arc::new(RwLock::new(alice));
    let leader_data = leader.info.clone();

    let server = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        None,
        true,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
    );

    // Send leader some tokens to vote
    let leader_balance = send_tx_and_retry_get_balance(
        &leader_data,
        &alice_arc.read().unwrap(),
        &leader_pubkey,
        500,
        None,
    ).unwrap();
    info!("leader balance {}", leader_balance);

    info!("{} LEADER", leader_data.id);
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
            Builder::new()
                .name("keypair-thread".to_string())
                .spawn(move || {
                    info!("Spawned thread {}", n);
                    Keypair::new()
                }).unwrap()
        }).collect();

    info!("Waiting for keypairs to be created");
    let keypairs: Vec<_> = t1.into_iter().map(|t| t.join().unwrap()).collect();
    info!("keypairs created");
    keypairs.iter().enumerate().for_each(|(n, keypair)| {
        //send some tokens to the new validators
        let bal = retry_send_tx_and_retry_get_balance(
            &leader_data,
            &alice_arc.read().unwrap(),
            &keypair.pubkey(),
            Some(500),
        );
        assert_eq!(bal, Some(500));
        info!("sent balance to [{}/{}] {}", n, num_nodes, keypair.pubkey());
    });
    let t2: Vec<_> = keypairs
        .into_iter()
        .map(|keypair| {
            let leader_data = leader_data.clone();
            let ledger_path = tmp_copy_ledger(&leader_ledger_path, "multi_node_dynamic_network");
            ledger_paths.push(ledger_path.clone());
            Builder::new()
                .name("validator-launch-thread".to_string())
                .spawn(move || {
                    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
                    let rd = validator.info.clone();
                    info!("starting {} {}", keypair.pubkey(), rd.id);
                    let val = Fullnode::new(
                        validator,
                        &ledger_path,
                        keypair,
                        Some(leader_data.contact_info.ncp),
                        true,
                        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
                    );
                    (rd, val)
                }).unwrap()
        }).collect();

    let mut validators: Vec<_> = t2.into_iter().map(|t| t.join().unwrap()).collect();

    let mut client = mk_client(&leader_data);
    let mut last_finality = client.get_finality();
    info!("Last finality {}", last_finality);
    let start = Instant::now();
    let mut consecutive_success = 0;
    let mut expected_balance = leader_balance;
    for i in 0..std::cmp::max(20, num_nodes) {
        trace!("getting leader last_id");
        let last_id = client.get_last_id();
        trace!("executing leader transfer");
        let sig = client
            .transfer(
                500,
                &alice_arc.read().unwrap().keypair(),
                bob_pubkey,
                &last_id,
            ).unwrap();

        expected_balance += 500;

        let e = client.poll_for_signature(&sig);
        assert!(e.is_ok(), "err: {:?}", e);

        let now = Instant::now();
        let mut finality = client.get_finality();

        // Need this to make sure the finality is updated
        // (i.e. the node is not returning stale value)
        while last_finality == finality {
            finality = client.get_finality();
        }

        while duration_as_ms(&now.elapsed()) < finality as u64 {
            sleep(Duration::from_millis(100));
            finality = client.get_finality()
        }

        last_finality = finality;

        let balance = retry_get_balance(&mut client, &bob_pubkey, Some(expected_balance));
        assert_eq!(balance, Some(expected_balance));
        consecutive_success += 1;

        info!(
            "SUCCESS[{}] balance: {}, finality: {} ms",
            i, expected_balance, last_finality,
        );

        if consecutive_success == 10 {
            info!("Took {} s to converge", duration_as_s(&start.elapsed()),);
            info!("Verifying signature of the last transaction in the validators");

            let mut num_nodes_behind = 0i64;
            validators.retain(|server| {
                let mut client = mk_client(&server.0);
                trace!("{} checking signature", server.0.id);
                num_nodes_behind += if client.check_signature(&sig) { 0 } else { 1 };
                server.1.exit();
                true
            });

            info!(
                "Validators lagging: {}/{}",
                num_nodes_behind,
                validators.len(),
            );
            break;
        }
    }

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

#[test]
fn test_leader_to_validator_transition() {
    logger::setup();
    let leader_rotation_interval = 20;

    // Make a dummy validator id to be the next leader and
    // sink for this test's mock transactions
    let validator_keypair = Keypair::new();
    let validator_id = validator_keypair.pubkey();

    // Create the leader node information
    let leader_keypair = Keypair::new();
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    // Initialize the leader ledger. Make a mint and a genesis entry
    // in the leader ledger
    let (mint, leader_ledger_path, genesis_entries) =
        create_sample_ledger("test_leader_to_validator_transition", 10_000);

    let last_id = genesis_entries
        .last()
        .expect("expected at least one genesis entry")
        .id;

    // Write the bootstrap entries to the ledger that will cause leader rotation
    // after the bootstrap height
    let mut ledger_writer = LedgerWriter::open(&leader_ledger_path, false).unwrap();
    let bootstrap_entries =
        make_active_set_entries(&validator_keypair, &mint.keypair(), &last_id, &last_id);
    let bootstrap_entries_len = bootstrap_entries.len();
    ledger_writer.write_entries(bootstrap_entries).unwrap();
    let ledger_initial_len = (genesis_entries.len() + bootstrap_entries_len) as u64;

    // Start the leader node
    let bootstrap_height = leader_rotation_interval;
    let leader_scheduler_config = LeaderSchedulerConfig::new(
        leader_info.id,
        Some(bootstrap_height),
        Some(leader_rotation_interval),
        Some(leader_rotation_interval * 2),
        Some(bootstrap_height),
    );

    let mut leader = Fullnode::new(
        leader_node,
        &leader_ledger_path,
        leader_keypair,
        Some(leader_info.contact_info.ncp),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
    );

    // Make an extra node for our leader to broadcast to,
    // who won't vote and mess with our leader's entry count
    let (ncp, spy_node, _) = make_spy_node(&leader_info);

    // Wait for the leader to see the spy node
    let mut converged = false;
    for _ in 0..30 {
        let num = spy_node.read().unwrap().convergence();
        let mut v: Vec<NodeInfo> = spy_node.read().unwrap().get_valid_peers();
        // There's only one person excluding the spy node (the leader) who should see
        // two nodes on the network
        if num >= 2 as u64 && v.len() >= 1 {
            converged = true;
            break;
        }
        sleep(Duration::new(1, 0));
    }

    assert!(converged);

    let extra_transactions = std::cmp::max(bootstrap_height / 3, 1);

    // Push leader "extra_transactions" past bootstrap_height,
    // make sure the leader stops.
    assert!(ledger_initial_len < bootstrap_height);
    for i in ledger_initial_len..(bootstrap_height + extra_transactions) {
        if i < bootstrap_height {
            // Poll to see that the bank state is updated after every transaction
            // to ensure that each transaction is packaged as a single entry,
            // so that we can be sure leader rotation is triggered
            let expected_balance = std::cmp::min(
                bootstrap_height - ledger_initial_len,
                i - ledger_initial_len + 1,
            );
            let result = send_tx_and_retry_get_balance(
                &leader_info,
                &mint,
                &validator_id,
                1,
                Some(expected_balance as i64),
            );

            // If the transaction wasn't reflected in the node, then we assume
            // the node has transitioned already
            if result != Some(expected_balance as i64) {
                break;
            }
        } else {
            // After bootstrap entries, we don't care about the
            // number of entries generated by these transactions. These are just
            // here for testing to make sure the leader stopped at the correct point.
            send_tx_and_retry_get_balance(&leader_info, &mint, &validator_id, 1, None);
        }
    }

    // Wait for leader to shut down tpu and restart tvu
    match leader.handle_role_transition().unwrap() {
        Some(FullnodeReturnType::LeaderToValidatorRotation) => (),
        _ => panic!("Expected reason for exit to be leader rotation"),
    }

    // Query now validator to make sure that he has the proper balances in his bank
    // after the transitions, even though we submitted "extra_transactions"
    // transactions earlier
    let mut leader_client = mk_client(&leader_info);

    let maximum_bal = bootstrap_height - ledger_initial_len;
    let bal = leader_client
        .poll_get_balance(&validator_id)
        .expect("Expected success when polling newly transitioned validator for balance")
        as u64;

    assert!(bal <= maximum_bal);

    // Check the ledger to make sure it's the right height, we should've
    // transitioned after the bootstrap_height entry
    let (_, entry_height, _) =
        Fullnode::new_bank_from_ledger(&leader_ledger_path, &mut LeaderScheduler::default());

    assert_eq!(entry_height, bootstrap_height);

    // Shut down
    ncp.close().unwrap();
    leader.close().unwrap();
    remove_dir_all(leader_ledger_path).unwrap();
}

#[test]
fn test_leader_validator_basic() {
    logger::setup();
    let leader_rotation_interval = 10;

    // Account that will be the sink for all the test's transactions
    let bob_pubkey = Keypair::new().pubkey();

    // Create the leader node information
    let leader_keypair = Keypair::new();
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    // Create the validator node information
    let validator_keypair = Keypair::new();
    let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

    // Make a common mint and a genesis entry for both leader + validator ledgers
    let (mint, leader_ledger_path, genesis_entries) =
        create_sample_ledger("test_leader_to_validator_transition", 10_000);

    let validator_ledger_path = tmp_copy_ledger(&leader_ledger_path, "test_leader_validator_basic");

    let last_id = genesis_entries
        .last()
        .expect("expected at least one genesis entry")
        .id;

    // Initialize both leader + validator ledger
    let mut ledger_paths = Vec::new();
    ledger_paths.push(leader_ledger_path.clone());
    ledger_paths.push(validator_ledger_path.clone());

    // Write the bootstrap entries to the ledger that will cause leader rotation
    // after the bootstrap height
    let mut ledger_writer = LedgerWriter::open(&leader_ledger_path, false).unwrap();
    let bootstrap_entries =
        make_active_set_entries(&validator_keypair, &mint.keypair(), &last_id, &last_id);
    let ledger_initial_len = (genesis_entries.len() + bootstrap_entries.len()) as u64;
    ledger_writer.write_entries(bootstrap_entries).unwrap();

    // Create the leader scheduler config
    let num_bootstrap_slots = 2;
    let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;
    let leader_scheduler_config = LeaderSchedulerConfig::new(
        leader_info.id,
        Some(bootstrap_height),
        Some(leader_rotation_interval),
        Some(leader_rotation_interval * 2),
        Some(bootstrap_height),
    );

    // Start the leader fullnode
    let mut leader = Fullnode::new(
        leader_node,
        &leader_ledger_path,
        leader_keypair,
        Some(leader_info.contact_info.ncp),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
    );

    // Start the validator node
    let mut validator = Fullnode::new(
        validator_node,
        &validator_ledger_path,
        validator_keypair,
        Some(leader_info.contact_info.ncp),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
    );

    // Wait for convergence
    let servers = converge(&leader_info, 2);
    assert_eq!(servers.len(), 2);

    // Send transactions to the leader
    let extra_transactions = std::cmp::max(leader_rotation_interval / 3, 1);

    // Push "extra_transactions" past leader_rotation_interval entry height,
    // make sure the validator stops.
    for i in ledger_initial_len..(bootstrap_height + extra_transactions) {
        let expected_balance = std::cmp::min(
            bootstrap_height - ledger_initial_len,
            i - ledger_initial_len + 1,
        );
        let result = send_tx_and_retry_get_balance(&leader_info, &mint, &bob_pubkey, 1, None);
        // If the transaction wasn't reflected in the node, then we assume
        // the node has transitioned already
        if result != Some(expected_balance as i64) {
            break;
        }
    }

    // Wait for validator to shut down tvu and restart tpu
    match validator.handle_role_transition().unwrap() {
        Some(FullnodeReturnType::ValidatorToLeaderRotation) => (),
        _ => panic!("Expected reason for exit to be leader rotation"),
    }

    // Wait for the leader to shut down tpu and restart tvu
    match leader.handle_role_transition().unwrap() {
        Some(FullnodeReturnType::LeaderToValidatorRotation) => (),
        _ => panic!("Expected reason for exit to be leader rotation"),
    }

    // Shut down
    validator.close().unwrap();
    leader.close().unwrap();

    // Check the ledger of the validator to make sure the entry height is correct
    // and that the old leader and the new leader's ledgers agree up to the point
    // of leader rotation
    let validator_entries =
        read_ledger(&validator_ledger_path, true).expect("Expected parsing of validator ledger");
    let leader_entries =
        read_ledger(&leader_ledger_path, true).expect("Expected parsing of leader ledger");

    let mut min_len = 0;
    for (v, l) in validator_entries.zip(leader_entries) {
        min_len += 1;
        assert_eq!(
            v.expect("expected valid validator entry"),
            l.expect("expected valid leader entry")
        );
    }

    assert!(min_len >= bootstrap_height);

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }
}

fn run_node(
    id: Pubkey,
    fullnode: Arc<RwLock<Fullnode>>,
    should_exit: Arc<AtomicBool>,
) -> JoinHandle<()> {
    Builder::new()
        .name(format!("run_node-{:?}", id).to_string())
        .spawn(move || loop {
            if should_exit.load(Ordering::Relaxed) {
                return;
            }
            if fullnode.read().unwrap().check_role_exited() {
                match fullnode.write().unwrap().handle_role_transition().unwrap() {
                    Some(FullnodeReturnType::LeaderToValidatorRotation) => (),
                    Some(FullnodeReturnType::ValidatorToLeaderRotation) => (),
                    _ => {
                        panic!("Expected reason for exit to be leader rotation");
                    }
                };
            }
            sleep(Duration::new(1, 0));
        }).unwrap()
}

#[test]
#[ignore]
fn test_dropped_handoff_recovery() {
    logger::setup();
    // The number of validators
    const N: usize = 3;
    assert!(N > 1);
    logger::setup();

    // Create the bootstrap leader node information
    let bootstrap_leader_keypair = Keypair::new();
    let bootstrap_leader_node = Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());
    let bootstrap_leader_info = bootstrap_leader_node.info.clone();

    // Make a common mint and a genesis entry for both leader + validator's ledgers
    let (mint, bootstrap_leader_ledger_path, genesis_entries) =
        create_sample_ledger("test_dropped_handoff_recovery", 10_000);

    let last_id = genesis_entries
        .last()
        .expect("expected at least one genesis entry")
        .id;

    // Create the validator keypair that will be the next leader in line
    let next_leader_keypair = Keypair::new();

    // Create a common ledger with entries in the beginning that will add only
    // the "next_leader" validator to the active set for leader election, guaranteeing
    // he is the next leader after bootstrap_height
    let mut ledger_paths = Vec::new();
    ledger_paths.push(bootstrap_leader_ledger_path.clone());

    // Make the entries to give the next_leader validator some stake so that he will be in
    // leader election active set
    let first_entries =
        make_active_set_entries(&next_leader_keypair, &mint.keypair(), &last_id, &last_id);
    let first_entries_len = first_entries.len();

    // Write the entries
    let mut ledger_writer = LedgerWriter::open(&bootstrap_leader_ledger_path, false).unwrap();
    ledger_writer.write_entries(first_entries).unwrap();

    let ledger_initial_len = (genesis_entries.len() + first_entries_len) as u64;

    let next_leader_ledger_path = tmp_copy_ledger(
        &bootstrap_leader_ledger_path,
        "test_dropped_handoff_recovery",
    );

    ledger_paths.push(next_leader_ledger_path.clone());

    // Create the common leader scheduling configuration
    let num_slots_per_epoch = (N + 1) as u64;
    let leader_rotation_interval = 5;
    let seed_rotation_interval = num_slots_per_epoch * leader_rotation_interval;
    let bootstrap_height = ledger_initial_len + 1;
    let leader_scheduler_config = LeaderSchedulerConfig::new(
        bootstrap_leader_info.id,
        Some(bootstrap_height),
        Some(leader_rotation_interval),
        Some(seed_rotation_interval),
        Some(leader_rotation_interval),
    );

    // Start up the bootstrap leader fullnode
    let bootstrap_leader = Fullnode::new(
        bootstrap_leader_node,
        &bootstrap_leader_ledger_path,
        bootstrap_leader_keypair,
        Some(bootstrap_leader_info.contact_info.ncp),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
    );

    let mut nodes = vec![bootstrap_leader];

    // Start up the validators other than the "next_leader" validator
    for _ in 0..(N - 1) {
        let kp = Keypair::new();
        let validator_ledger_path = tmp_copy_ledger(
            &bootstrap_leader_ledger_path,
            "test_dropped_handoff_recovery",
        );
        ledger_paths.push(validator_ledger_path.clone());
        let validator_id = kp.pubkey();
        let validator_node = Node::new_localhost_with_pubkey(validator_id);
        let validator = Fullnode::new(
            validator_node,
            &validator_ledger_path,
            kp,
            Some(bootstrap_leader_info.contact_info.ncp),
            false,
            LeaderScheduler::new(&leader_scheduler_config),
        );

        nodes.push(validator);
    }

    // Wait for convergence
    let num_converged = converge(&bootstrap_leader_info, N).len();
    assert_eq!(num_converged, N);

    // Wait for leader transition
    match nodes[0].handle_role_transition().unwrap() {
        Some(FullnodeReturnType::LeaderToValidatorRotation) => (),
        _ => panic!("Expected reason for exit to be leader rotation"),
    }

    // Now start up the "next leader" node
    let next_leader_node = Node::new_localhost_with_pubkey(next_leader_keypair.pubkey());
    let mut next_leader = Fullnode::new(
        next_leader_node,
        &next_leader_ledger_path,
        next_leader_keypair,
        Some(bootstrap_leader_info.contact_info.ncp),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
    );

    // Wait for catchup
    match next_leader.handle_role_transition().unwrap() {
        Some(FullnodeReturnType::ValidatorToLeaderRotation) => (),
        _ => panic!("Expected reason for exit to be leader rotation"),
    }

    nodes.push(next_leader);

    for node in nodes {
        node.close().unwrap();
    }

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }
}

#[test]
#[ignore]
//TODO: Ignore for now due to bug exposed by the test "test_dropped_handoff_recovery"
fn test_full_leader_validator_network() {
    logger::setup();
    // The number of validators
    const N: usize = 5;
    logger::setup();

    // Create the bootstrap leader node information
    let bootstrap_leader_keypair = Keypair::new();
    let bootstrap_leader_node = Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());
    let bootstrap_leader_info = bootstrap_leader_node.info.clone();

    let mut node_keypairs = VecDeque::new();
    node_keypairs.push_back(bootstrap_leader_keypair);

    // Create the validator keypairs
    for _ in 0..N {
        let validator_keypair = Keypair::new();
        node_keypairs.push_back(validator_keypair);
    }

    // Make a common mint and a genesis entry for both leader + validator's ledgers
    let (mint, bootstrap_leader_ledger_path, genesis_entries) =
        create_sample_ledger("test_full_leader_validator_network", 10_000);

    let last_tick_id = genesis_entries
        .last()
        .expect("expected at least one genesis entry")
        .id;

    let mut last_entry_id = genesis_entries
        .last()
        .expect("expected at least one genesis entry")
        .id;

    // Create a common ledger with entries in the beginnging that will add all the validators
    // to the active set for leader election. TODO: Leader rotation does not support dynamic
    // stakes for safe leader -> validator transitions due to the unpredictability of
    // bank state due to transactions being in-flight during leader seed calculation in
    // write stage.
    let mut ledger_paths = Vec::new();
    ledger_paths.push(bootstrap_leader_ledger_path.clone());

    for node_keypair in node_keypairs.iter() {
        // Make entries to give the validator some stake so that he will be in
        // leader election active set
        let bootstrap_entries =
            make_active_set_entries(node_keypair, &mint.keypair(), &last_entry_id, &last_tick_id);

        // Write the entries
        let mut ledger_writer = LedgerWriter::open(&bootstrap_leader_ledger_path, false).unwrap();
        last_entry_id = bootstrap_entries
            .last()
            .expect("expected at least one genesis entry")
            .id;
        ledger_writer.write_entries(bootstrap_entries).unwrap();
    }

    // Create the common leader scheduling configuration
    let num_slots_per_epoch = (N + 1) as u64;
    let num_bootstrap_slots = 2;
    let leader_rotation_interval = 5;
    let seed_rotation_interval = num_slots_per_epoch * leader_rotation_interval;
    let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;
    let leader_scheduler_config = LeaderSchedulerConfig::new(
        bootstrap_leader_info.id,
        Some(bootstrap_height),
        Some(leader_rotation_interval),
        Some(seed_rotation_interval),
        Some(leader_rotation_interval),
    );

    let exit = Arc::new(AtomicBool::new(false));
    // Start the bootstrap leader fullnode
    let bootstrap_leader = Arc::new(RwLock::new(Fullnode::new(
        bootstrap_leader_node,
        &bootstrap_leader_ledger_path,
        node_keypairs.pop_front().unwrap(),
        Some(bootstrap_leader_info.contact_info.ncp),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
    )));

    let mut nodes: Vec<Arc<RwLock<Fullnode>>> = vec![bootstrap_leader.clone()];
    let mut t_nodes = vec![run_node(
        bootstrap_leader_info.id,
        bootstrap_leader,
        exit.clone(),
    )];

    // Start up the validators
    for kp in node_keypairs.into_iter() {
        let validator_ledger_path = tmp_copy_ledger(
            &bootstrap_leader_ledger_path,
            "test_full_leader_validator_network",
        );
        ledger_paths.push(validator_ledger_path.clone());
        let validator_id = kp.pubkey();
        let validator_node = Node::new_localhost_with_pubkey(validator_id);
        let validator = Arc::new(RwLock::new(Fullnode::new(
            validator_node,
            &validator_ledger_path,
            kp,
            Some(bootstrap_leader_info.contact_info.ncp),
            false,
            LeaderScheduler::new(&leader_scheduler_config),
        )));

        nodes.push(validator.clone());
        t_nodes.push(run_node(validator_id, validator, exit.clone()));
    }

    // Wait for convergence
    let num_converged = converge(&bootstrap_leader_info, N + 1).len();
    assert_eq!(num_converged, N + 1);

    // Wait for each node to hit a specific target height in the leader schedule.
    // Once all the nodes hit that height, exit them all together. They must
    // only quit once they've all confirmed to reach that specific target height.
    // Otherwise, some nodes may never reach the target height if a critical
    // next leader node exits first, and stops generating entries. (We don't
    // have a timeout mechanism).
    let target_height = bootstrap_height + seed_rotation_interval;
    let mut num_reached_target_height = 0;

    while num_reached_target_height != N + 1 {
        num_reached_target_height = 0;
        for n in nodes.iter() {
            let node_lock = n.read().unwrap();
            let ls_lock = &node_lock.leader_scheduler;
            if let Some(sh) = ls_lock.read().unwrap().last_seed_height {
                if sh >= target_height {
                    num_reached_target_height += 1;
                }
            }
            drop(ls_lock);
        }

        sleep(Duration::new(1, 0));
    }

    exit.store(true, Ordering::Relaxed);

    // Wait for threads running the nodes to exit
    for t in t_nodes {
        t.join().unwrap();
    }

    // Exit all fullnodes
    for n in nodes {
        let result = Arc::try_unwrap(n);
        match result {
            Ok(lock) => {
                let f = lock
                    .into_inner()
                    .expect("RwLock for fullnode is still locked");
                f.close().unwrap();
            }
            Err(_) => panic!("Multiple references to RwLock<FullNode> still exist"),
        }
    }

    let mut node_entries = vec![];
    // Check that all the ledgers match
    for ledger_path in ledger_paths.iter() {
        let entries = read_ledger(ledger_path, true).expect("Expected parsing of node ledger");
        node_entries.push(entries);
    }

    let mut shortest = None;
    let mut length = 0;
    loop {
        let mut expected_entry_option = None;
        let mut empty_iterators = HashSet::new();
        for (i, entries_for_specific_node) in node_entries.iter_mut().enumerate() {
            if let Some(next_entry_option) = entries_for_specific_node.next() {
                // If this ledger iterator has another entry, make sure that the
                // ledger reader parsed it correctly
                let next_entry = next_entry_option.expect("expected valid ledger entry");

                // Check if another earlier ledger iterator had another entry. If so, make
                // sure they match
                if let Some(ref expected_entry) = expected_entry_option {
                    assert_eq!(*expected_entry, next_entry);
                } else {
                    expected_entry_option = Some(next_entry);
                }
            } else {
                // The shortest iterator is the first one to return a None when
                // calling next()
                if shortest.is_none() {
                    shortest = Some(length);
                }
                empty_iterators.insert(i);
            }
        }

        // Remove the empty iterators
        node_entries = node_entries
            .into_iter()
            .enumerate()
            .filter_map(|(i, x)| match empty_iterators.get(&i) {
                None => Some(x),
                _ => None,
            }).collect();

        if node_entries.len() == 0 {
            break;
        }

        length += 1;
    }

    assert!(shortest.unwrap() >= target_height);
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
    assert!(ClusterInfo::is_valid_address(&leader.contact_info.rpu));
    assert!(ClusterInfo::is_valid_address(&leader.contact_info.tpu));
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
    transfer_amount: i64,
    expected: Option<i64>,
) -> Option<i64> {
    let mut client = mk_client(leader);
    trace!("getting leader last_id");
    let last_id = client.get_last_id();
    let tx = Transaction::system_new(&alice.keypair(), *bob_pubkey, transfer_amount, last_id);
    info!("executing leader transfer");
    let _res = client.retry_transfer_signed(&tx, 30);
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
