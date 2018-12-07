#[macro_use]
extern crate log;
extern crate bincode;
extern crate chrono;
#[macro_use]
extern crate serde_json;
extern crate solana;
extern crate solana_sdk;
extern crate solana_vote_signer;

use solana::blob_fetch_stage::BlobFetchStage;
use solana::cluster_info::{ClusterInfo, Node, NodeInfo};
use solana::contact_info::ContactInfo;
use solana::db_ledger::{DbLedger, DEFAULT_SLOT_HEIGHT};
use solana::create_vote_account::*;
use solana::db_ledger::DbLedger;
use solana::entry::{reconstruct_entries_from_blobs, Entry};
use solana::fullnode::{Fullnode, FullnodeReturnType};
use solana::gossip_service::GossipService;
use solana::leader_scheduler::{make_active_set_entries, LeaderScheduler, LeaderSchedulerConfig};
use solana::ledger::{create_tmp_genesis, create_tmp_sample_ledger, tmp_copy_ledger};

use solana::mint::Mint;
use solana::packet::SharedBlob;
use solana::poh_service::NUM_TICKS_PER_SECOND;
use solana::result;
use solana::rpc_request::{RpcClient, RpcRequest};
use solana::service::Service;
use solana::thin_client::{retry_get_balance, ThinClient};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing::{duration_as_ms, duration_as_s};
use solana_sdk::transaction::Transaction;
use std::collections::{HashSet, VecDeque};
use std::env;
use std::fs::remove_dir_all;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder, JoinHandle};
use std::time::{Duration, Instant};

fn read_ledger(ledger_path: &str) -> Vec<Entry> {
    let ledger = DbLedger::open(&ledger_path).expect("Unable to open ledger");
    ledger
        .read_ledger()
        .expect("Unable to read ledger")
        .collect()
}

fn make_spy_node(leader: &NodeInfo) -> (GossipService, Arc<RwLock<ClusterInfo>>, Pubkey) {
    let keypair = Keypair::new();
    let exit = Arc::new(AtomicBool::new(false));
    let mut spy = Node::new_localhost_with_pubkey(keypair.pubkey());
    let me = spy.info.id.clone();
    let daddr = "0.0.0.0:0".parse().unwrap();
    spy.info.tvu = daddr;
    let mut spy_cluster_info = ClusterInfo::new_with_keypair(spy.info, Arc::new(keypair));
    spy_cluster_info.insert_info(leader.clone());
    spy_cluster_info.set_leader(leader.id);
    let spy_cluster_info_ref = Arc::new(RwLock::new(spy_cluster_info));
    let gossip_service = GossipService::new(
        &spy_cluster_info_ref,
        None,
        spy.sockets.gossip,
        exit.clone(),
    );

    (gossip_service, spy_cluster_info_ref, me)
}

fn make_listening_node(
    leader: &NodeInfo,
) -> (GossipService, Arc<RwLock<ClusterInfo>>, Node, Pubkey) {
    let keypair = Keypair::new();
    let exit = Arc::new(AtomicBool::new(false));
    let new_node = Node::new_localhost_with_pubkey(keypair.pubkey());
    let new_node_info = new_node.info.clone();
    let me = new_node.info.id.clone();
    let mut new_node_cluster_info = ClusterInfo::new_with_keypair(new_node_info, Arc::new(keypair));
    new_node_cluster_info.insert_info(leader.clone());
    new_node_cluster_info.set_leader(leader.id);
    let new_node_cluster_info_ref = Arc::new(RwLock::new(new_node_cluster_info));
    let gossip_service = GossipService::new(
        &new_node_cluster_info_ref,
        None,
        new_node
            .sockets
            .gossip
            .try_clone()
            .expect("Failed to clone gossip"),
        exit.clone(),
    );

    (gossip_service, new_node_cluster_info_ref, new_node, me)
}

fn converge(leader: &NodeInfo, num_nodes: usize) -> Vec<NodeInfo> {
    //lets spy on the network
    let (gossip_service, spy_ref, _) = make_spy_node(leader);

    //wait for the network to converge
    let mut converged = false;
    let mut rv = vec![];
    for _ in 0..30 {
        let num = spy_ref.read().unwrap().convergence();
        let mut v = spy_ref.read().unwrap().rpc_peers();
        if num >= num_nodes && v.len() >= num_nodes {
            rv.append(&mut v);
            converged = true;
            break;
        }
        sleep(Duration::new(1, 0));
    }
    assert!(converged);
    gossip_service.close().unwrap();
    rv
}

fn make_tiny_test_entries(start_hash: Hash, num: usize) -> Vec<Entry> {
    let mut id = start_hash;
    let mut num_hashes = 0;
    (0..num)
        .map(|_| Entry::new_mut(&mut id, &mut num_hashes, vec![]))
        .collect()
}

fn register_node(signer: SocketAddr, keypair: Arc<Keypair>) -> Pubkey {
    let rpc_client = RpcClient::new_from_socket(signer);

    let msg = "Registering a new node";
    let sig = Signature::new(&keypair.sign(msg.as_bytes()).as_ref());

    let params = json!([keypair.pubkey().to_string(), sig, msg.as_bytes()]);
    let resp = RpcRequest::RegisterNode
        .make_rpc_request(&rpc_client, 1, Some(params))
        .unwrap();
    let vote_account_id: Pubkey = serde_json::from_value(resp).unwrap();
    vote_account_id
}

#[test]
fn test_multi_node_ledger_window() -> result::Result<()> {
    solana_logger::setup();

    let leader_keypair = Arc::new(Keypair::new());
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (alice, leader_ledger_path) =
        create_tmp_genesis("multi_node_ledger_window", 10_000, leader_data.id, 500);
    ledger_paths.push(leader_ledger_path.clone());

    // make a copy at zero
    let zero_ledger_path = tmp_copy_ledger(&leader_ledger_path, "multi_node_ledger_window");
    ledger_paths.push(zero_ledger_path.clone());

    // write a bunch more ledger into leader's ledger, this should populate the leader's window
    // and force it to respond to repair from the ledger window
    {
        let entries = make_tiny_test_entries(alice.last_id(), 100);
        let db_ledger = DbLedger::open(&leader_ledger_path).unwrap();
        db_ledger
            .write_entries(
                DEFAULT_SLOT_HEIGHT,
                solana::mint::NUM_GENESIS_ENTRIES as u64,
                &entries,
            )
            .unwrap();
    }

    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let vote_id = register_node(signer, leader_keypair.clone());
    let leader = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        &vote_id,
        &signer,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        None,
    );

    // start up another validator from zero, converge and then check
    // balances
    let keypair = Arc::new(Keypair::new());
    let validator_pubkey = keypair.pubkey().clone();
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.info.clone();
    let validator_vote_id = register_node(signer, keypair.clone());
    let validator = Fullnode::new(
        validator,
        &zero_ledger_path,
        keypair,
        &validator_vote_id,
        &signer,
        Some(leader_data.gossip),
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        None,
    );

    // Send validator some tokens to vote
    let validator_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &validator_pubkey, 500, None).unwrap();
    info!("validator balance {}", validator_balance);

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

    stop_local_vote_signer_service(t_signer, &signer_exit);

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }

    Ok(())
}

#[test]
#[ignore]
fn test_multi_node_validator_catchup_from_zero() -> result::Result<()> {
    solana_logger::setup();
    const N: usize = 5;
    trace!("test_multi_node_validator_catchup_from_zero");
    let leader_keypair = Arc::new(Keypair::new());
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (alice, leader_ledger_path) = create_tmp_genesis(
        "multi_node_validator_catchup_from_zero",
        10_000,
        leader_data.id,
        500,
    );
    ledger_paths.push(leader_ledger_path.clone());

    let zero_ledger_path = tmp_copy_ledger(
        &leader_ledger_path,
        "multi_node_validator_catchup_from_zero",
    );
    ledger_paths.push(zero_ledger_path.clone());

    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let vote_id = register_node(signer, leader_keypair.clone());
    let server = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        &vote_id,
        &signer,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        None,
    );

    let mut nodes = vec![server];
    for _ in 0..N {
        let keypair = Arc::new(Keypair::new());
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

        let vote_id = register_node(signer, keypair.clone());
        let val = Fullnode::new(
            validator,
            &ledger_path,
            keypair,
            &vote_id,
            &signer,
            Some(leader_data.gossip),
            false,
            LeaderScheduler::from_bootstrap_leader(leader_pubkey),
            None,
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
    let keypair = Arc::new(Keypair::new());
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let vote_id = register_node(signer, keypair.clone());
    let val = Fullnode::new(
        validator,
        &zero_ledger_path,
        keypair,
        &vote_id,
        &signer,
        Some(leader_data.gossip),
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        None,
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

    stop_local_vote_signer_service(t_signer, &signer_exit);

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }

    Ok(())
}

#[test]
#[ignore]
fn test_multi_node_basic() {
    solana_logger::setup();
    const N: usize = 5;
    trace!("test_multi_node_basic");
    let leader_keypair = Arc::new(Keypair::new());
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (alice, leader_ledger_path) =
        create_tmp_genesis("multi_node_basic", 10_000, leader_data.id, 500);
    ledger_paths.push(leader_ledger_path.clone());
    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let vote_id = register_node(signer, leader_keypair.clone());
    let server = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        &vote_id,
        &signer,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        None,
    );

    let mut nodes = vec![server];
    for _ in 0..N {
        let keypair = Arc::new(Keypair::new());
        let validator_pubkey = keypair.pubkey().clone();
        let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
        let ledger_path = tmp_copy_ledger(&leader_ledger_path, "multi_node_basic");
        ledger_paths.push(ledger_path.clone());

        // Send each validator some tokens to vote
        let validator_balance =
            send_tx_and_retry_get_balance(&leader_data, &alice, &validator_pubkey, 500, None)
                .unwrap();
        info!("validator balance {}", validator_balance);
        let vote_id = register_node(signer, keypair.clone());
        let val = Fullnode::new(
            validator,
            &ledger_path,
            keypair,
            &vote_id,
            &signer,
            Some(leader_data.gossip),
            false,
            LeaderScheduler::from_bootstrap_leader(leader_pubkey),
            None,
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
    stop_local_vote_signer_service(t_signer, &signer_exit);

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }
}

#[test]
#[ignore]
fn test_boot_validator_from_file() -> result::Result<()> {
    solana_logger::setup();
    let leader_keypair = Arc::new(Keypair::new());
    let leader_pubkey = leader_keypair.pubkey();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let bob_pubkey = Keypair::new().pubkey();
    let (alice, leader_ledger_path) =
        create_tmp_genesis("boot_validator_from_file", 100_000, leader_pubkey, 1000);
    let mut ledger_paths = Vec::new();
    ledger_paths.push(leader_ledger_path.clone());

    let leader_data = leader.info.clone();
    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let vote_id = register_node(signer, leader_keypair.clone());
    let leader_fullnode = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        &vote_id,
        &signer,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        None,
    );
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(500)).unwrap();
    assert_eq!(leader_balance, 500);
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(1000)).unwrap();
    assert_eq!(leader_balance, 1000);

    let keypair = Arc::new(Keypair::new());
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.info.clone();
    let ledger_path = tmp_copy_ledger(&leader_ledger_path, "boot_validator_from_file");
    ledger_paths.push(ledger_path.clone());
    let vote_id = register_node(signer, keypair.clone());
    let val_fullnode = Fullnode::new(
        validator,
        &ledger_path,
        keypair,
        &vote_id,
        &signer,
        Some(leader_data.gossip),
        false,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        None,
    );
    let mut client = mk_client(&validator_data);
    let getbal = retry_get_balance(&mut client, &bob_pubkey, Some(leader_balance));
    assert!(getbal == Some(leader_balance));

    val_fullnode.close()?;
    leader_fullnode.close()?;
    stop_local_vote_signer_service(t_signer, &signer_exit);

    for path in ledger_paths {
        remove_dir_all(path)?;
    }

    Ok(())
}

fn create_leader(
    ledger_path: &str,
    leader_keypair: Arc<Keypair>,
    vote_id: &Pubkey,
    signer: &SocketAddr,
) -> (NodeInfo, Fullnode) {
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let leader_fullnode = Fullnode::new(
        leader,
        &ledger_path,
        leader_keypair,
        &vote_id,
        &signer,
        None,
        false,
        LeaderScheduler::from_bootstrap_leader(leader_data.id),
        None,
    );
    (leader_data, leader_fullnode)
}

#[test]
#[ignore]
fn test_leader_restart_validator_start_from_old_ledger() -> result::Result<()> {
    // this test verifies that a freshly started leader makes his ledger available
    //    in the repair window to validators that are started with an older
    //    ledger (currently up to WINDOW_SIZE entries)
    solana_logger::setup();

    let leader_keypair = Arc::new(Keypair::new());
    let initial_leader_balance = 500;
    let (alice, ledger_path) = create_tmp_genesis(
        "leader_restart_validator_start_from_old_ledger",
        100_000 + 500 * solana::window_service::MAX_REPAIR_BACKOFF as u64,
        leader_keypair.pubkey(),
        initial_leader_balance,
    );
    let bob_pubkey = Keypair::new().pubkey();

    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let vote_id = register_node(signer, leader_keypair.clone());
    let (leader_data, leader_fullnode) =
        create_leader(&ledger_path, leader_keypair.clone(), &vote_id, &signer);

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
    let (leader_data, leader_fullnode) =
        create_leader(&ledger_path, leader_keypair.clone(), &vote_id, &signer);

    // lengthen the ledger
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(1000)).unwrap();
    assert_eq!(leader_balance, 1000);

    // restart the leader
    leader_fullnode.close()?;
    let (leader_data, leader_fullnode) =
        create_leader(&ledger_path, leader_keypair, &vote_id, &signer);

    // start validator from old ledger
    let keypair = Arc::new(Keypair::new());
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.info.clone();

    let vote_id = register_node(signer, keypair.clone());
    let val_fullnode = Fullnode::new(
        validator,
        &stale_ledger_path,
        keypair,
        &vote_id,
        &signer,
        Some(leader_data.gossip),
        false,
        LeaderScheduler::from_bootstrap_leader(leader_data.id),
        None,
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
    stop_local_vote_signer_service(t_signer, &signer_exit);
    remove_dir_all(ledger_path)?;
    remove_dir_all(stale_ledger_path)?;

    Ok(())
}

//TODO: this test will run a long time so it's disabled for CI
#[test]
#[ignore]
fn test_multi_node_dynamic_network() {
    solana_logger::setup();
    assert!(cfg!(feature = "test"));
    let key = "SOLANA_DYNAMIC_NODES";
    let num_nodes: usize = match env::var(key) {
        Ok(val) => val
            .parse()
            .expect(&format!("env var {} is not parse-able as usize", key)),
        Err(_) => 120,
    };

    let leader_keypair = Arc::new(Keypair::new());
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let bob_pubkey = Keypair::new().pubkey();
    let (alice, leader_ledger_path) =
        create_tmp_genesis("multi_node_dynamic_network", 10_000_000, leader_pubkey, 500);

    let mut ledger_paths = Vec::new();
    ledger_paths.push(leader_ledger_path.clone());

    let alice_arc = Arc::new(RwLock::new(alice));
    let leader_data = leader.info.clone();

    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let vote_id = register_node(signer, leader_keypair.clone());
    let server = Fullnode::new(
        leader,
        &leader_ledger_path,
        leader_keypair,
        &vote_id,
        &signer,
        None,
        true,
        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
        None,
    );

    info!("{} LEADER", leader_data.id);
    let leader_balance = retry_send_tx_and_retry_get_balance(
        &leader_data,
        &alice_arc.read().unwrap(),
        &bob_pubkey,
        Some(500),
    )
    .unwrap();
    assert_eq!(leader_balance, 500);
    let leader_balance = retry_send_tx_and_retry_get_balance(
        &leader_data,
        &alice_arc.read().unwrap(),
        &bob_pubkey,
        Some(1000),
    )
    .unwrap();
    assert_eq!(leader_balance, 1000);

    let t1: Vec<_> = (0..num_nodes)
        .into_iter()
        .map(|n| {
            Builder::new()
                .name("keypair-thread".to_string())
                .spawn(move || {
                    info!("Spawned thread {}", n);
                    Keypair::new()
                })
                .unwrap()
        })
        .collect();

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
                    let keypair = Arc::new(keypair);
                    let vote_id = register_node(signer, keypair.clone());
                    let val = Fullnode::new(
                        validator,
                        &ledger_path,
                        keypair,
                        &vote_id,
                        &signer,
                        Some(leader_data.gossip),
                        true,
                        LeaderScheduler::from_bootstrap_leader(leader_pubkey),
                        None,
                    );
                    (rd, val)
                })
                .unwrap()
        })
        .collect();

    let mut validators: Vec<_> = t2.into_iter().map(|t| t.join().unwrap()).collect();

    let mut client = mk_client(&leader_data);
    let mut last_confirmation = client.get_confirmation_time();
    info!("Last confirmation {}", last_confirmation);
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
            )
            .unwrap();

        expected_balance += 500;

        let e = client.poll_for_signature(&sig);
        assert!(e.is_ok(), "err: {:?}", e);

        let now = Instant::now();
        let mut confirmation = client.get_confirmation_time();

        // Need this to make sure the confirmation is updated
        // (i.e. the node is not returning stale value)
        while last_confirmation == confirmation {
            confirmation = client.get_confirmation_time();
        }

        while duration_as_ms(&now.elapsed()) < confirmation as u64 {
            sleep(Duration::from_millis(100));
            confirmation = client.get_confirmation_time()
        }

        last_confirmation = confirmation;

        let balance = retry_get_balance(&mut client, &bob_pubkey, Some(expected_balance));
        assert_eq!(balance, Some(expected_balance));
        consecutive_success += 1;

        info!(
            "SUCCESS[{}] balance: {}, confirmation: {} ms",
            i, expected_balance, last_confirmation,
        );

        if consecutive_success == 10 {
            info!("Took {} s to converge", duration_as_s(&start.elapsed()),);
            info!("Verifying signature of the last transaction in the validators");

            let mut num_nodes_behind = 0u64;
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

    stop_local_vote_signer_service(t_signer, &signer_exit);
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
    solana_logger::setup();
    let leader_rotation_interval = 20;

    // Make a dummy validator id to be the next leader
    let validator_keypair = Keypair::new();

    // Create the leader node information
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    // Initialize the leader ledger. Make a mint and a genesis entry
    // in the leader ledger
    let num_ending_ticks = 1;
    let (mint, leader_ledger_path, genesis_entries) = create_tmp_sample_ledger(
        "test_leader_to_validator_transition",
        10_000,
        num_ending_ticks,
        leader_info.id,
        500,
    );

    let last_id = genesis_entries
        .last()
        .expect("expected at least one genesis entry")
        .id;

    // Write the bootstrap entries to the ledger that will cause leader rotation
    // after the bootstrap height
    let (bootstrap_entries, _) =
        make_active_set_entries(&validator_keypair, &mint.keypair(), &last_id, &last_id, 0);
    {
        let db_ledger = DbLedger::open(&leader_ledger_path).unwrap();
        db_ledger
            .write_entries(
                DEFAULT_SLOT_HEIGHT,
                genesis_entries.len() as u64,
                &bootstrap_entries,
            )
            .unwrap();
    }

    // Start the leader node
    let bootstrap_height = leader_rotation_interval;
    let leader_scheduler_config = LeaderSchedulerConfig::new(
        Some(bootstrap_height),
        Some(leader_rotation_interval),
        Some(leader_rotation_interval * 2),
        Some(bootstrap_height),
    );

    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let vote_id = register_node(signer, leader_keypair.clone());
    let mut leader = Fullnode::new(
        leader_node,
        &leader_ledger_path,
        leader_keypair,
        &vote_id,
        &signer,
        Some(leader_info.gossip),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
        None,
    );

    // Make an extra node for our leader to broadcast to,
    // who won't vote and mess with our leader's entry count
    let (gossip_service, spy_node, _) = make_spy_node(&leader_info);

    // Wait for the leader to see the spy node
    let mut converged = false;
    for _ in 0..30 {
        let num = spy_node.read().unwrap().convergence();
        let v: Vec<NodeInfo> = spy_node.read().unwrap().rpc_peers();
        // There's only one person excluding the spy node (the leader) who should see
        // two nodes on the network
        if num >= 2 && v.len() >= 1 {
            converged = true;
            break;
        }
        sleep(Duration::new(1, 0));
    }

    assert!(converged);

    // Account that will be the sink for all the test's transactions
    let bob_pubkey = Keypair::new().pubkey();

    // Push transactions until we detect an exit
    let mut i = 1;
    loop {
        // Poll to see that the bank state is updated after every transaction
        // to ensure that each transaction is packaged as a single entry,
        // so that we can be sure leader rotation is triggered
        let result =
            send_tx_and_retry_get_balance(&leader_info, &mint, &bob_pubkey, 1, Some(i as u64));

        // If the transaction wasn't reflected in the node, then we assume
        // the node has transitioned already
        if result != Some(i as u64) {
            break;
        }

        i += 1;
    }

    // Wait for leader to shut down tpu and restart tvu
    match leader.handle_role_transition().unwrap() {
        Some(FullnodeReturnType::LeaderToValidatorRotation) => (),
        _ => panic!("Expected reason for exit to be leader rotation"),
    }

    // Query newly transitioned validator to make sure that they have the proper balances in
    // the after the transitions
    let mut leader_client = mk_client(&leader_info);

    // Leader could have executed transactions in bank but not recorded them, so
    // we only have an upper bound on the balance
    if let Ok(bal) = leader_client.poll_get_balance(&bob_pubkey) {
        assert!(bal <= i);
    }

    // Shut down
    gossip_service.close().unwrap();
    leader.close().unwrap();

    // Check the ledger to make sure it's the right height, we should've
    // transitioned after tick_height == bootstrap_height
    let (bank, _, _) = Fullnode::new_bank_from_ledger(
        &leader_ledger_path,
        Arc::new(RwLock::new(LeaderScheduler::default())),
    );

    assert_eq!(bank.tick_height(), bootstrap_height);
    stop_local_vote_signer_service(t_signer, &signer_exit);
    remove_dir_all(leader_ledger_path).unwrap();
}

#[test]
fn test_leader_validator_basic() {
    solana_logger::setup();
    let leader_rotation_interval = 10;

    // Account that will be the sink for all the test's transactions
    let bob_pubkey = Keypair::new().pubkey();

    // Create the leader node information
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    // Create the validator node information
    let validator_keypair = Arc::new(Keypair::new());
    let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

    // Make a common mint and a genesis entry for both leader + validator ledgers
    let num_ending_ticks = 1;
    let (mint, leader_ledger_path, genesis_entries) = create_tmp_sample_ledger(
        "test_leader_validator_basic",
        10_000,
        num_ending_ticks,
        leader_info.id,
        500,
    );

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
    let (active_set_entries, vote_account_keypair) =
        make_active_set_entries(&validator_keypair, &mint.keypair(), &last_id, &last_id, 0);
    {
        let db_ledger = DbLedger::open(&leader_ledger_path).unwrap();
        db_ledger
            .write_entries(
                DEFAULT_SLOT_HEIGHT,
                genesis_entries.len() as u64,
                &active_set_entries,
            )
            .unwrap();
    }

    // Create the leader scheduler config
    let num_bootstrap_slots = 2;
    let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;
    let leader_scheduler_config = LeaderSchedulerConfig::new(
        Some(bootstrap_height),
        Some(leader_rotation_interval),
        Some(leader_rotation_interval * 2),
        Some(bootstrap_height),
    );

    // Start the validator node
    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let vote_id = register_node(signer, validator_keypair.clone());
    let mut validator = Fullnode::new(
        validator_node,
        &validator_ledger_path,
        validator_keypair,
        &vote_id,
        &signer,
        Some(leader_info.gossip),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
        None,
    );

    // Start the leader fullnode
    let vote_id = register_node(signer, leader_keypair.clone());
    let mut leader = Fullnode::new(
        leader_node,
        &leader_ledger_path,
        leader_keypair,
        &vote_id,
        &signer,
        Some(leader_info.gossip),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
        None,
    );

    // Wait for convergence
    let servers = converge(&leader_info, 2);
    assert_eq!(servers.len(), 2);

    // Push transactions until we detect the nodes exit
    let mut i = 1;
    loop {
        // Poll to see that the bank state is updated after every transaction
        // to ensure that each transaction is packaged as a single entry,
        // so that we can be sure leader rotation is triggered
        let result = send_tx_and_retry_get_balance(&leader_info, &mint, &bob_pubkey, 1, None);

        // If the transaction wasn't reflected in the node, then we assume
        // the node has transitioned already
        if result != Some(i as u64) {
            break;
        }

        i += 1;
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

    // Query newly transitioned validator to make sure they have the proper balances
    // in the bank after the transitions
    let mut leader_client = mk_client(&leader_info);

    // Leader could have executed transactions in bank but not recorded them, so
    // we only have an upper bound on the balance
    if let Ok(bal) = leader_client.poll_get_balance(&bob_pubkey) {
        assert!(bal <= i);
    }

    // Shut down
    validator
        .close()
        .expect("Expected successful validator close");
    leader.close().expect("Expected successful leader close");

    // Check the ledger of the validator to make sure the entry height is correct
    // and that the old leader and the new leader's ledgers agree up to the point
    // of leader rotation
    let validator_entries: Vec<Entry> = read_ledger(&validator_ledger_path);

    let leader_entries = read_ledger(&leader_ledger_path);

    assert_eq!(leader_entries.len(), validator_entries.len());
    assert!(leader_entries.len() as u64 >= bootstrap_height);

    for (v, l) in validator_entries.iter().zip(leader_entries) {
        assert_eq!(*v, l);
    }
    stop_local_vote_signer_service(t_signer, &signer_exit);

    for path in ledger_paths {
        DbLedger::destroy(&path).expect("Expected successful database destruction");
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
        })
        .unwrap()
}

#[test]
#[ignore]
fn test_dropped_handoff_recovery() {
    solana_logger::setup();
    // The number of validators
    const N: usize = 3;
    assert!(N > 1);
    solana_logger::setup();

    // Create the bootstrap leader node information
    let bootstrap_leader_keypair = Arc::new(Keypair::new());
    let bootstrap_leader_node = Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());
    let bootstrap_leader_info = bootstrap_leader_node.info.clone();

    // Make a common mint and a genesis entry for both leader + validator's ledgers
    let num_ending_ticks = 1;
    let (mint, bootstrap_leader_ledger_path, genesis_entries) = create_tmp_sample_ledger(
        "test_dropped_handoff_recovery",
        10_000,
        num_ending_ticks,
        bootstrap_leader_info.id,
        500,
    );

    let last_id = genesis_entries
        .last()
        .expect("expected at least one genesis entry")
        .id;

    // Create the validator keypair that will be the next leader in line
    let next_leader_keypair = Arc::new(Keypair::new());

    // Create a common ledger with entries in the beginning that will add only
    // the "next_leader" validator to the active set for leader election, guaranteeing
    // they are the next leader after bootstrap_height
    let mut ledger_paths = Vec::new();
    ledger_paths.push(bootstrap_leader_ledger_path.clone());

    // Make the entries to give the next_leader validator some stake so that they will be in
    // leader election active set
    let (active_set_entries, _vote_account_keypair) =
        make_active_set_entries(&next_leader_keypair, &mint.keypair(), &last_id, &last_id, 0);

    // Write the entries
    {
        let db_ledger = DbLedger::open(&bootstrap_leader_ledger_path).unwrap();
        db_ledger
            .write_entries(
                DEFAULT_SLOT_HEIGHT,
                genesis_entries.len() as u64,
                &active_set_entries,
            )
            .unwrap();
    }

    let next_leader_ledger_path = tmp_copy_ledger(
        &bootstrap_leader_ledger_path,
        "test_dropped_handoff_recovery",
    );

    ledger_paths.push(next_leader_ledger_path.clone());

    // Create the common leader scheduling configuration
    let initial_tick_height = genesis_entries
        .iter()
        .fold(0, |tick_count, entry| tick_count + entry.is_tick() as u64);
    let num_slots_per_epoch = (N + 1) as u64;
    let leader_rotation_interval = 5;
    let seed_rotation_interval = num_slots_per_epoch * leader_rotation_interval;
    let bootstrap_height = initial_tick_height + 1;
    let leader_scheduler_config = LeaderSchedulerConfig::new(
        Some(bootstrap_height),
        Some(leader_rotation_interval),
        Some(seed_rotation_interval),
        Some(leader_rotation_interval),
    );

    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let vote_id = register_node(signer, bootstrap_leader_keypair.clone());
    // Start up the bootstrap leader fullnode
    let bootstrap_leader = Fullnode::new(
        bootstrap_leader_node,
        &bootstrap_leader_ledger_path,
        bootstrap_leader_keypair,
        &vote_id,
        &signer,
        Some(bootstrap_leader_info.gossip),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
        None,
    );

    let mut nodes = vec![bootstrap_leader];

    // Start up the validators other than the "next_leader" validator
    for _ in 0..(N - 1) {
        let kp = Arc::new(Keypair::new());
        let validator_ledger_path = tmp_copy_ledger(
            &bootstrap_leader_ledger_path,
            "test_dropped_handoff_recovery",
        );
        ledger_paths.push(validator_ledger_path.clone());
        let validator_id = kp.pubkey();
        let validator_node = Node::new_localhost_with_pubkey(validator_id);
        let vote_id = register_node(signer, kp.clone());
        let validator = Fullnode::new(
            validator_node,
            &validator_ledger_path,
            kp,
            &vote_id,
            &signer,
            Some(bootstrap_leader_info.gossip),
            false,
            LeaderScheduler::new(&leader_scheduler_config),
            None,
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
    let vote_id = register_node(signer, next_leader_keypair.clone());
    let mut next_leader = Fullnode::new(
        next_leader_node,
        &next_leader_ledger_path,
        next_leader_keypair,
        &vote_id,
        &signer,
        Some(bootstrap_leader_info.gossip),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
        None,
    );

    // Wait for catchup
    match next_leader.handle_role_transition().unwrap() {
        Some(FullnodeReturnType::ValidatorToLeaderRotation) => (),
        _ => panic!("Expected reason for exit to be leader rotation"),
    }

    nodes.push(next_leader);

    stop_local_vote_signer_service(t_signer, &signer_exit);
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
    solana_logger::setup();
    // The number of validators
    const N: usize = 5;
    solana_logger::setup();

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
    let num_ending_ticks = 1;
    let (mint, bootstrap_leader_ledger_path, genesis_entries) = create_tmp_sample_ledger(
        "test_full_leader_validator_network",
        10_000,
        num_ending_ticks,
        bootstrap_leader_info.id,
        500,
    );

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

    let mut vote_account_keypairs = VecDeque::new();
    for node_keypair in node_keypairs.iter() {
        // Make entries to give each node some stake so that they will be in the
        // leader election active set
        let (bootstrap_entries, vote_account_keypair) = make_active_set_entries(
            node_keypair,
            &mint.keypair(),
            &last_entry_id,
            &last_tick_id,
            0,
        );

        vote_account_keypairs.push_back(vote_account_keypair);

        // Write the entries
        last_entry_id = bootstrap_entries
            .last()
            .expect("expected at least one genesis entry")
            .id;
        {
            let db_ledger = DbLedger::open(&bootstrap_leader_ledger_path).unwrap();
            db_ledger
                .write_entries(
                    DEFAULT_SLOT_HEIGHT,
                    genesis_entries.len() as u64,
                    &bootstrap_entries,
                )
                .unwrap();
        }
    }

    // Create the common leader scheduling configuration
    let num_slots_per_epoch = (N + 1) as u64;
    let num_bootstrap_slots = 2;
    let leader_rotation_interval = 5;
    let seed_rotation_interval = num_slots_per_epoch * leader_rotation_interval;
    let bootstrap_height = num_bootstrap_slots * leader_rotation_interval;
    let leader_scheduler_config = LeaderSchedulerConfig::new(
        Some(bootstrap_height),
        Some(leader_rotation_interval),
        Some(seed_rotation_interval),
        Some(100),
    );

    let exit = Arc::new(AtomicBool::new(false));

    // Postpone starting the leader until after the validators are up and running
    // to avoid
    // 1) Scenario where leader rotates before validators can start up
    // 2) Modifying the leader ledger which validators are going to be copying
    // during startup
    let leader_keypair = node_keypairs.pop_front().unwrap();
    let _leader_vote_keypair = vote_account_keypairs.pop_front().unwrap();
    let mut nodes: Vec<Arc<RwLock<Fullnode>>> = vec![];
    let mut t_nodes = vec![];

    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    // Start up the validators
    for kp in node_keypairs.into_iter() {
        let validator_ledger_path = tmp_copy_ledger(
            &bootstrap_leader_ledger_path,
            "test_full_leader_validator_network",
        );

        ledger_paths.push(validator_ledger_path.clone());

        let validator_id = kp.pubkey();
        let validator_node = Node::new_localhost_with_pubkey(validator_id);
        let kp = Arc::new(kp);
        let vote_id = register_node(signer, kp.clone());
        let validator = Arc::new(RwLock::new(Fullnode::new(
            validator_node,
            &validator_ledger_path,
            kp,
            &vote_id,
            &signer,
            Some(bootstrap_leader_info.gossip),
            false,
            LeaderScheduler::new(&leader_scheduler_config),
            None,
        )));

        nodes.push(validator.clone());
        t_nodes.push(run_node(validator_id, validator, exit.clone()));
    }

    // Start up the bootstrap leader
    let leader_keypair = Arc::new(leader_keypair);
    let vote_id = register_node(signer, leader_keypair.clone());
    let bootstrap_leader = Arc::new(RwLock::new(Fullnode::new(
        bootstrap_leader_node,
        &bootstrap_leader_ledger_path,
        leader_keypair,
        &vote_id,
        &signer,
        Some(bootstrap_leader_info.gossip),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
        None,
    )));

    nodes.push(bootstrap_leader.clone());
    t_nodes.push(run_node(
        bootstrap_leader_info.id,
        bootstrap_leader,
        exit.clone(),
    ));

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
            let ls_lock = node_lock.get_leader_scheduler();
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
        let entries = read_ledger(ledger_path);
        node_entries.push(entries.into_iter());
    }

    let mut shortest = None;
    let mut length = 0;
    loop {
        let mut expected_entry_option = None;
        let mut empty_iterators = HashSet::new();
        for (i, entries_for_specific_node) in node_entries.iter_mut().enumerate() {
            if let Some(next_entry) = entries_for_specific_node.next() {
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
            })
            .collect();

        if node_entries.len() == 0 {
            break;
        }

        length += 1;
    }

    assert!(shortest.unwrap() >= target_height);

    stop_local_vote_signer_service(t_signer, &signer_exit);
    for path in ledger_paths {
        DbLedger::destroy(&path).expect("Expected successful database destruction");
        remove_dir_all(path).unwrap();
    }
}

#[test]
fn test_broadcast_last_tick() {
    solana_logger::setup();
    // The number of validators
    const N: usize = 5;
    solana_logger::setup();

    // Create the bootstrap leader node information
    let bootstrap_leader_keypair = Keypair::new();
    let bootstrap_leader_node = Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());
    let bootstrap_leader_info = bootstrap_leader_node.info.clone();

    // Create leader ledger
    let (_, bootstrap_leader_ledger_path, genesis_entries) = create_tmp_sample_ledger(
        "test_broadcast_last_tick",
        10_000,
        0,
        bootstrap_leader_info.id,
        500,
    );

    let num_ending_ticks = genesis_entries
        .iter()
        .skip(2)
        .fold(0, |tick_count, entry| tick_count + entry.is_tick() as u64);

    let genesis_ledger_len = genesis_entries.len() as u64 - num_ending_ticks;
    let blob_receiver_exit = Arc::new(AtomicBool::new(false));

    // Create the listeners
    let mut listening_nodes: Vec<_> = (0..N)
        .map(|_| make_listening_node(&bootstrap_leader_info))
        .collect();

    let blob_fetch_stages: Vec<_> = listening_nodes
        .iter_mut()
        .map(|(_, _, node, _)| {
            BlobFetchStage::new(
                Arc::new(node.sockets.tvu.pop().unwrap()),
                blob_receiver_exit.clone(),
            )
        })
        .collect();

    // Create fullnode, should take 20 seconds to reach end of bootstrap period
    let bootstrap_height = (NUM_TICKS_PER_SECOND * 20) as u64;
    let leader_rotation_interval = 100;
    let seed_rotation_interval = 200;
    let leader_scheduler_config = LeaderSchedulerConfig::new(
        Some(bootstrap_height),
        Some(leader_rotation_interval),
        Some(seed_rotation_interval),
        Some(leader_rotation_interval),
    );

    // Start up the bootstrap leader fullnode
    let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
    let bootstrap_leader_keypair = Arc::new(bootstrap_leader_keypair);
    let vote_id = register_node(signer, bootstrap_leader_keypair.clone());
    let mut bootstrap_leader = Fullnode::new(
        bootstrap_leader_node,
        &bootstrap_leader_ledger_path,
        bootstrap_leader_keypair,
        &vote_id,
        &signer,
        Some(bootstrap_leader_info.gossip),
        false,
        LeaderScheduler::new(&leader_scheduler_config),
        None,
    );

    // Wait for convergence
    let servers = converge(&bootstrap_leader_info, N + 1);
    assert_eq!(servers.len(), N + 1);

    // Wait for leader rotation
    match bootstrap_leader.handle_role_transition().unwrap() {
        Some(FullnodeReturnType::LeaderToValidatorRotation) => (),
        _ => panic!("Expected reason for exit to be leader rotation"),
    }

    // Shut down the leader
    bootstrap_leader.close().unwrap();

    let last_tick_entry_height = genesis_ledger_len as u64 + bootstrap_height;
    let entries = read_ledger(&bootstrap_leader_ledger_path);
    assert!(entries.len() >= last_tick_entry_height as usize);
    let expected_last_tick = &entries[last_tick_entry_height as usize - 1];
    // Check that the nodes got the last broadcasted blob
    for (_, receiver) in blob_fetch_stages.iter() {
        let mut last_tick_blob: SharedBlob = SharedBlob::default();
        while let Ok(new_blobs) = receiver.try_recv() {
            let last_blob = new_blobs.into_iter().find(|b| {
                b.read().unwrap().index().expect("Expected index in blob")
                    == last_tick_entry_height - 1
            });
            if let Some(last_blob) = last_blob {
                last_tick_blob = last_blob;
                break;
            }
        }
        let actual_last_tick =
            &reconstruct_entries_from_blobs(vec![&*last_tick_blob.read().unwrap()])
                .expect("Expected to be able to reconstruct entries from blob")
                .0[0];
        assert_eq!(actual_last_tick, expected_last_tick);
    }

    // Shut down blob fetch stages
    blob_receiver_exit.store(true, Ordering::Relaxed);
    for (bf, _) in blob_fetch_stages {
        bf.join().unwrap();
    }

    stop_local_vote_signer_service(t_signer, &signer_exit);
    // Shut down the listeners
    for node in listening_nodes {
        node.0.close().unwrap();
    }
    remove_dir_all(bootstrap_leader_ledger_path).unwrap();
}

fn mk_client(leader: &NodeInfo) -> ThinClient {
    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    assert!(ContactInfo::is_valid_address(&leader.tpu));
    ThinClient::new(leader.rpc, leader.tpu, transactions_socket)
}

fn send_tx_and_retry_get_balance(
    leader: &NodeInfo,
    alice: &Mint,
    bob_pubkey: &Pubkey,
    transfer_amount: u64,
    expected: Option<u64>,
) -> Option<u64> {
    let mut client = mk_client(leader);
    trace!("getting leader last_id");
    let last_id = client.get_last_id();
    let mut tx = Transaction::system_new(&alice.keypair(), *bob_pubkey, transfer_amount, last_id);
    info!("executing leader transfer");
    let _res = client.retry_transfer(&alice.keypair(), &mut tx, 30);
    retry_get_balance(&mut client, bob_pubkey, expected)
}

fn retry_send_tx_and_retry_get_balance(
    leader: &NodeInfo,
    alice: &Mint,
    bob_pubkey: &Pubkey,
    expected: Option<u64>,
) -> Option<u64> {
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
