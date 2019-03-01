#[macro_use]
extern crate solana;

use log::*;
use solana::blob_fetch_stage::BlobFetchStage;
use solana::blocktree::{create_new_tmp_ledger, tmp_copy_blocktree, Blocktree};
use solana::client::mk_client;
use solana::cluster_info::{Node, NodeInfo};
use solana::contact_info::ContactInfo;
use solana::entry::next_entry_mut;
use solana::entry::{reconstruct_entries_from_blobs, Entry};
use solana::fullnode::make_active_set_entries;
use solana::fullnode::{new_banks_from_blocktree, Fullnode, FullnodeConfig, FullnodeReturnType};
use solana::gossip_service::{converge, make_listening_node};
use solana::poh_service::PohServiceConfig;
use solana::result;
use solana::service::Service;
use solana::thin_client::{poll_gossip_for_leader, retry_get_balance};
use solana::voting_keypair::VotingKeypair;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing::duration_as_s;
use solana_sdk::vote_transaction::VoteTransaction;
use std::collections::{HashSet, VecDeque};
use std::env;
use std::fs::remove_dir_all;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::mpsc::{channel, sync_channel, TryRecvError};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder};
use std::time::{Duration, Instant};

fn read_ledger(ledger_path: &str, ticks_per_slot: u64) -> Vec<Entry> {
    let ledger =
        Blocktree::open_config(&ledger_path, ticks_per_slot).expect("Unable to open ledger");
    ledger
        .read_ledger()
        .expect("Unable to read ledger")
        .collect()
}

#[test]
fn test_multi_node_ledger_window() -> result::Result<()> {
    solana_logger::setup();

    let leader_keypair = Arc::new(Keypair::new());
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (genesis_block, alice) = GenesisBlock::new_with_leader(10_000, leader_data.id, 500);
    let ticks_per_slot = genesis_block.ticks_per_slot;
    info!("ticks_per_slot: {}", ticks_per_slot);

    let (leader_ledger_path, last_id) = create_new_tmp_ledger!(&genesis_block);
    ledger_paths.push(leader_ledger_path.clone());

    // make a copy at zero
    let zero_ledger_path = tmp_copy_blocktree!(&leader_ledger_path);
    ledger_paths.push(zero_ledger_path.clone());

    // Write some into leader's ledger, this should populate the leader's window
    // and force it to respond to repair from the ledger window
    {
        let blocktree = Blocktree::open_config(&leader_ledger_path, ticks_per_slot).unwrap();
        let entries = solana::entry::create_ticks(genesis_block.ticks_per_slot, last_id);
        blocktree.write_entries(1, 0, 0, &entries).unwrap();
    }

    let fullnode_config = FullnodeConfig::default();
    let voting_keypair = VotingKeypair::new_local(&leader_keypair);
    let leader = Fullnode::new(
        leader,
        &leader_keypair,
        &leader_ledger_path,
        voting_keypair,
        None,
        &fullnode_config,
    );
    let leader_exit = leader.run(None);

    // Give validator some tokens for voting
    let keypair = Arc::new(Keypair::new());
    let validator_pubkey = keypair.pubkey().clone();
    info!("validator id: {:?}", validator_pubkey);
    let validator_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &validator_pubkey, 500, None).unwrap();
    info!("validator balance {}", validator_balance);

    // Start up another validator from zero, converge and then check
    // balances
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.info.clone();
    let voting_keypair = VotingKeypair::new_local(&keypair);
    let validator = Fullnode::new(
        validator,
        &keypair,
        &zero_ledger_path,
        voting_keypair,
        Some(&leader_data),
        &FullnodeConfig::default(),
    );
    let validator_exit = validator.run(None);

    converge(&leader_data, 2);

    // Another transaction with leader
    let bob_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 1, None).unwrap();
    info!("bob balance on leader {}", bob_balance);
    let mut checks = 1;
    loop {
        let mut leader_client = mk_client(&leader_data);
        let bal = leader_client.poll_get_balance(&bob_pubkey);
        info!(
            "Bob balance on leader is {:?} after {} checks...",
            bal, checks
        );

        let mut validator_client = mk_client(&validator_data);
        let bal = validator_client.poll_get_balance(&bob_pubkey);
        info!(
            "Bob balance on validator is {:?} after {} checks...",
            bal, checks
        );
        if bal.unwrap_or(0) == bob_balance {
            break;
        }
        checks += 1;
    }

    info!("Done!");
    validator_exit();
    leader_exit();

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }

    Ok(())
}

#[test]
fn test_multi_node_validator_catchup_from_zero() -> result::Result<()> {
    solana_logger::setup();
    const N: usize = 2;
    trace!("test_multi_node_validator_catchup_from_zero");
    let leader_keypair = Arc::new(Keypair::new());
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (genesis_block, alice) = GenesisBlock::new_with_leader(10_000, leader_data.id, 500);
    let (genesis_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);
    ledger_paths.push(genesis_ledger_path.clone());

    let zero_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
    ledger_paths.push(zero_ledger_path.clone());

    let leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
    ledger_paths.push(leader_ledger_path.clone());
    let fullnode_config = FullnodeConfig::default();
    let voting_keypair = VotingKeypair::new_local(&leader_keypair);
    let server = Fullnode::new(
        leader,
        &leader_keypair,
        &leader_ledger_path,
        voting_keypair,
        None,
        &fullnode_config,
    );

    let mut node_exits = vec![server.run(None)];
    for _ in 0..N {
        let keypair = Arc::new(Keypair::new());
        let validator_pubkey = keypair.pubkey().clone();
        let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
        let ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
        ledger_paths.push(ledger_path.clone());

        // Send each validator some tokens to vote
        let validator_balance =
            send_tx_and_retry_get_balance(&leader_data, &alice, &validator_pubkey, 500, None)
                .unwrap();
        info!(
            "validator {} balance {}",
            validator_pubkey, validator_balance
        );

        let voting_keypair = VotingKeypair::new_local(&keypair);
        let validator = Fullnode::new(
            validator,
            &keypair,
            &ledger_path,
            voting_keypair,
            Some(&leader_data),
            &FullnodeConfig::default(),
        );
        node_exits.push(validator.run(None));
    }
    let nodes = converge(&leader_data, N + 1); // contains the leader addr as well

    // Verify leader can transfer from alice to bob
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 123, None).unwrap();
    assert_eq!(leader_balance, 123);

    // Verify validators all have the same balance for bob
    let mut success = 0usize;
    for server in nodes.iter() {
        let id = server.id;
        info!("0server: {}", id);
        let mut client = mk_client(server);

        let mut found = false;
        for i in 0..20 {
            let result = client.poll_get_balance(&bob_pubkey);
            if let Ok(bal) = client.poll_get_balance(&bob_pubkey) {
                if bal == leader_balance {
                    info!("validator {} bob balance {}", id, bal);
                    success += 1;
                    found = true;
                    break;
                } else {
                    info!("validator {} bob balance {} incorrect: {}", id, i, bal);
                }
            } else {
                info!(
                    "validator {} bob poll_get_balance {} failed: {:?}",
                    id, i, result
                );
            }
            sleep(Duration::new(1, 0));
        }
        assert!(found);
    }
    assert_eq!(success, nodes.len());

    success = 0;

    // Start up another validator from zero, converge and then check everyone's
    // balances
    let keypair = Arc::new(Keypair::new());
    let validator_pubkey = keypair.pubkey().clone();
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let voting_keypair = VotingKeypair::new_local(&keypair);
    info!("created start from zero validator {:?}", validator_pubkey);

    let validator = Fullnode::new(
        validator,
        &keypair,
        &zero_ledger_path,
        voting_keypair,
        Some(&leader_data),
        &FullnodeConfig::default(),
    );

    node_exits.push(validator.run(None));
    let nodes = converge(&leader_data, N + 2); // contains the leader and new node

    // Transfer a little more from alice to bob
    let mut leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 333, None).unwrap();
    info!("leader balance {}", leader_balance);
    loop {
        let mut client = mk_client(&leader_data);
        leader_balance = client.poll_get_balance(&bob_pubkey)?;
        if leader_balance == 456 {
            break;
        }
        sleep(Duration::from_millis(500));
    }
    assert_eq!(leader_balance, 456);

    for server in nodes.iter() {
        let id = server.id;
        info!("1server: {}", id);
        let mut client = mk_client(server);
        let mut found = false;
        for i in 0..30 {
            let result = client.poll_get_balance(&bob_pubkey);
            if let Ok(bal) = result {
                if bal == leader_balance {
                    info!("validator {} bob2 balance {}", id, bal);
                    success += 1;
                    found = true;
                    break;
                } else {
                    info!("validator {} bob2 balance {} incorrect: {}", id, i, bal);
                }
            } else {
                info!(
                    "validator {} bob2 poll_get_balance {} failed: {:?}",
                    id, i, result
                );
            }
            sleep(Duration::new(2, 0));
        }
        assert!(found);
    }
    assert_eq!(success, nodes.len());

    trace!("done!");

    for node_exit in node_exits {
        node_exit();
    }

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }

    Ok(())
}

#[test]
fn test_multi_node_basic() {
    solana_logger::setup();
    const N: usize = 5;
    trace!("test_multi_node_basic");

    let leader_keypair = Arc::new(Keypair::new());
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (genesis_block, alice) = GenesisBlock::new_with_leader(10_000, leader_data.id, 500);

    let (genesis_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);
    ledger_paths.push(genesis_ledger_path.clone());

    let leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
    ledger_paths.push(leader_ledger_path.clone());

    let fullnode_config = FullnodeConfig::default();
    let voting_keypair = VotingKeypair::new_local(&leader_keypair);
    let server = Fullnode::new(
        leader,
        &leader_keypair,
        &leader_ledger_path,
        voting_keypair,
        None,
        &fullnode_config,
    );

    let mut exit_signals = vec![server.run(None)];
    for i in 0..N {
        let keypair = Arc::new(Keypair::new());
        let validator_pubkey = keypair.pubkey().clone();
        let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
        let ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
        ledger_paths.push(ledger_path.clone());

        // Send each validator some tokens to vote
        let validator_balance =
            send_tx_and_retry_get_balance(&leader_data, &alice, &validator_pubkey, 500, None)
                .unwrap();
        info!(
            "validator #{} - {}, balance {}",
            i, validator_pubkey, validator_balance
        );
        let voting_keypair = VotingKeypair::new_local(&keypair);
        let val = Fullnode::new(
            validator,
            &keypair,
            &ledger_path,
            voting_keypair,
            Some(&leader_data),
            &fullnode_config,
        );
        exit_signals.push(val.run(None));
    }
    let nodes = converge(&leader_data, N + 1);

    // Verify leader can do transfer from alice to bob
    let leader_bob_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 123, None).unwrap();
    assert_eq!(leader_bob_balance, 123);

    // Verify validators all have the same balance for bob
    let mut success = 0usize;
    for server in nodes.iter() {
        let id = server.id;
        info!("mk_client for {}", id);
        let mut client = mk_client(server);
        let mut found = false;
        for _ in 1..20 {
            let result = client.poll_get_balance(&bob_pubkey);
            if let Ok(validator_bob_balance) = result {
                trace!("validator {} bob balance {}", id, validator_bob_balance);
                if validator_bob_balance == leader_bob_balance {
                    success += 1;
                    found = true;
                    break;
                } else {
                    warn!(
                        "validator {} bob balance incorrect, expecting {}",
                        id, leader_bob_balance
                    );
                }
            } else {
                warn!("validator {} bob poll_get_balance failed: {:?}", id, result);
            }
            sleep(Duration::new(1, 0));
        }
        assert!(found);
    }
    assert_eq!(success, nodes.len());
    trace!("done!");

    for exit_signal in exit_signals {
        exit_signal()
    }

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }
}

#[test]
fn test_boot_validator_from_file() {
    solana_logger::setup();
    let leader_keypair = Arc::new(Keypair::new());
    let leader_pubkey = leader_keypair.pubkey();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let bob_pubkey = Keypair::new().pubkey();
    let mut ledger_paths = Vec::new();

    let (genesis_block, alice) = GenesisBlock::new_with_leader(100_000, leader_pubkey, 1000);
    let (genesis_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);
    ledger_paths.push(genesis_ledger_path.clone());

    let leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
    ledger_paths.push(leader_ledger_path.clone());

    let leader_data = leader.info.clone();
    let fullnode_config = FullnodeConfig::default();
    let voting_keypair = VotingKeypair::new_local(&leader_keypair);
    let leader_fullnode = Fullnode::new(
        leader,
        &leader_keypair,
        &leader_ledger_path,
        voting_keypair,
        None,
        &fullnode_config,
    );
    let leader_fullnode_exit = leader_fullnode.run(None);

    info!("Sending transaction to leader");
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(500)).unwrap();
    assert_eq!(leader_balance, 500);
    let leader_balance =
        send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(1000)).unwrap();
    assert_eq!(leader_balance, 1000);
    info!("Leader balance verified");

    let keypair = Arc::new(Keypair::new());
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.info.clone();
    let ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
    ledger_paths.push(ledger_path.clone());
    let voting_keypair = VotingKeypair::new_local(&keypair);
    let val_fullnode = Fullnode::new(
        validator,
        &keypair,
        &ledger_path,
        voting_keypair,
        Some(&leader_data),
        &fullnode_config,
    );

    let (rotation_sender, rotation_receiver) = channel();
    let val_fullnode_exit = val_fullnode.run(Some(rotation_sender));

    // Wait for validator to start and process a couple slots before trying to poke at it via RPC
    // TODO: it would be nice to determine the slot that the leader processed the transactions
    // in, and only wait for that slot here
    let expected_rotations = vec![
        (FullnodeReturnType::ValidatorToValidatorRotation, 1),
        (FullnodeReturnType::ValidatorToValidatorRotation, 2),
        (FullnodeReturnType::ValidatorToValidatorRotation, 3),
    ];

    for expected_rotation in expected_rotations {
        loop {
            let transition = rotation_receiver.recv().unwrap();
            info!("validator transition: {:?}", transition);
            assert_eq!(transition, expected_rotation);
            break;
        }
    }

    info!("Checking validator balance");
    let mut client = mk_client(&validator_data);
    assert_eq!(
        retry_get_balance(&mut client, &bob_pubkey, Some(leader_balance)),
        Some(leader_balance)
    );
    info!("Validator balance verified");

    val_fullnode_exit();
    leader_fullnode_exit();

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }
}

fn create_leader(
    ledger_path: &str,
    leader_keypair: Arc<Keypair>,
    voting_keypair: VotingKeypair,
) -> (NodeInfo, Fullnode) {
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();
    let leader_fullnode = Fullnode::new(
        leader,
        &leader_keypair,
        &ledger_path,
        voting_keypair,
        None,
        &FullnodeConfig::default(),
    );
    (leader_data, leader_fullnode)
}

#[test]
fn test_leader_restart_validator_start_from_old_ledger() -> result::Result<()> {
    // this test verifies that a freshly started leader makes its ledger available
    //    in the repair window to validators that are started with an older
    //    ledger (currently up to WINDOW_SIZE entries)
    solana_logger::setup();

    let leader_keypair = Arc::new(Keypair::new());
    let initial_leader_balance = 500;

    let (genesis_block, alice) = GenesisBlock::new_with_leader(
        100_000 + 500 * solana::window_service::MAX_REPAIR_BACKOFF as u64,
        leader_keypair.pubkey(),
        initial_leader_balance,
    );
    let (ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);

    let bob_pubkey = Keypair::new().pubkey();

    {
        let voting_keypair = VotingKeypair::new_local(&leader_keypair);
        let (leader_data, leader_fullnode) =
            create_leader(&ledger_path, leader_keypair.clone(), voting_keypair);
        let leader_fullnode_exit = leader_fullnode.run(None);

        // Give bob 500 tokens via the leader
        assert_eq!(
            send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(500))
                .unwrap(),
            500
        );

        // restart the leader
        leader_fullnode_exit();
    }

    // Create a "stale" ledger by copying current ledger where bob only has 500 tokens
    let stale_ledger_path = tmp_copy_blocktree!(&ledger_path);

    {
        let voting_keypair = VotingKeypair::new_local(&leader_keypair);
        let (leader_data, leader_fullnode) =
            create_leader(&ledger_path, leader_keypair.clone(), voting_keypair);
        let leader_fullnode_exit = leader_fullnode.run(None);

        // Give bob 500 more tokens via the leader
        assert_eq!(
            send_tx_and_retry_get_balance(&leader_data, &alice, &bob_pubkey, 500, Some(1000))
                .unwrap(),
            1000
        );

        leader_fullnode_exit();
    }

    let voting_keypair = VotingKeypair::new_local(&leader_keypair);
    let (leader_data, leader_fullnode) =
        create_leader(&ledger_path, leader_keypair, voting_keypair);
    let leader_fullnode_exit = leader_fullnode.run(None);

    // Start validator from "stale" ledger
    let keypair = Arc::new(Keypair::new());
    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
    let validator_data = validator.info.clone();

    let fullnode_config = FullnodeConfig::default();
    let voting_keypair = VotingKeypair::new_local(&keypair);
    let val_fullnode = Fullnode::new(
        validator,
        &keypair,
        &stale_ledger_path,
        voting_keypair,
        Some(&leader_data),
        &fullnode_config,
    );
    let val_fullnode_exit = val_fullnode.run(None);

    // Validator should catch up from leader whose window contains the entries missing from the
    // stale ledger send requests so the validator eventually sees a gap and requests a repair
    let expected_bob_balance = 1000;
    let mut validator_client = mk_client(&validator_data);

    for _ in 0..42 {
        let balance = retry_get_balance(
            &mut validator_client,
            &bob_pubkey,
            Some(expected_bob_balance),
        );
        info!(
            "Bob balance at the validator is {:?} (expecting {:?})",
            balance, expected_bob_balance
        );
        if balance == Some(expected_bob_balance) {
            break;
        }
    }

    val_fullnode_exit();
    leader_fullnode_exit();
    remove_dir_all(ledger_path)?;
    remove_dir_all(stale_ledger_path)?;

    Ok(())
}

#[test]
#[ignore] // TODO: This test is unstable.  Fix and re-enable
fn test_multi_node_dynamic_network() {
    solana_logger::setup();
    let key = "SOLANA_DYNAMIC_NODES";
    let num_nodes: usize = match env::var(key) {
        Ok(val) => val
            .parse()
            .expect(&format!("env var {} is not parse-able as usize", key)),
        Err(_) => 5, // Small number of nodes by default, adjust with SOLANA_DYNAMIC_NODES
    };

    let leader_keypair = Arc::new(Keypair::new());
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let bob_pubkey = Keypair::new().pubkey();

    let (genesis_block, alice) = GenesisBlock::new_with_leader(10_000_000, leader_pubkey, 500);
    let (genesis_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);

    let mut ledger_paths = Vec::new();
    ledger_paths.push(genesis_ledger_path.clone());

    let leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);

    let alice_arc = Arc::new(RwLock::new(alice));
    let leader_data = leader.info.clone();

    ledger_paths.push(leader_ledger_path.clone());
    let fullnode_config = FullnodeConfig::default();
    let voting_keypair = VotingKeypair::new_local(&leader_keypair);
    let server = Fullnode::new(
        leader,
        &leader_keypair,
        &leader_ledger_path,
        voting_keypair,
        None,
        &fullnode_config,
    );
    let server_exit = server.run(None);
    info!(
        "found leader: {:?}",
        poll_gossip_for_leader(leader_data.gossip, Some(5)).unwrap()
    );

    let bob_balance = retry_send_tx_and_retry_get_balance(
        &leader_data,
        &alice_arc.read().unwrap(),
        &bob_pubkey,
        Some(500),
    )
    .unwrap();
    assert_eq!(bob_balance, 500);
    let bob_balance = retry_send_tx_and_retry_get_balance(
        &leader_data,
        &alice_arc.read().unwrap(),
        &bob_pubkey,
        Some(1000),
    )
    .unwrap();
    assert_eq!(bob_balance, 1000);

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
        // Send some tokens to the new validators
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
            let ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
            ledger_paths.push(ledger_path.clone());
            Builder::new()
                .name("validator-launch-thread".to_string())
                .spawn(move || {
                    let validator = Node::new_localhost_with_pubkey(keypair.pubkey());
                    let validator_info = validator.info.clone();
                    info!("starting {}", keypair.pubkey());
                    let keypair = Arc::new(keypair);
                    let voting_keypair = VotingKeypair::new_local(&keypair);
                    let validator = Fullnode::new(
                        validator,
                        &keypair,
                        &ledger_path,
                        voting_keypair,
                        Some(&leader_data),
                        &FullnodeConfig::default(),
                    );
                    let validator_exit = validator.run(None);
                    (validator_info, validator_exit)
                })
                .unwrap()
        })
        .collect();

    let mut validators: Vec<_> = t2.into_iter().map(|t| t.join().unwrap()).collect();

    let mut client = mk_client(&leader_data);
    let start = Instant::now();
    let mut consecutive_success = 0;
    let mut expected_balance = bob_balance;
    let mut last_id = client.get_last_id();
    for i in 0..std::cmp::max(20, num_nodes) {
        trace!("Getting last_id (iteration {})...", i);
        let mut retries = 30;
        loop {
            let new_last_id = client.get_last_id();
            if new_last_id != last_id {
                last_id = new_last_id;
                break;
            }
            debug!("waiting for new last_id, retries={}", retries);
            retries -= 1;
            if retries == 0 {
                panic!("last_id stuck at {}", last_id);
            }
            sleep(Duration::from_millis(100));
        }
        debug!("last_id: {}", last_id);
        trace!("Executing leader transfer of 100");

        let mut transaction =
            SystemTransaction::new_move(&alice_arc.read().unwrap(), bob_pubkey, 100, last_id, 0);
        let sig = client
            .retry_transfer(&alice_arc.read().unwrap(), &mut transaction, 5)
            .unwrap();
        trace!("transfer sig: {:?}", sig);

        expected_balance += 100;
        let mut retries = 30;
        loop {
            let balance = retry_get_balance(&mut client, &bob_pubkey, Some(expected_balance));
            if let Some(balance) = balance {
                if balance == expected_balance {
                    break;
                }
            }
            retries -= 1;
            debug!(
                "balance not yet correct: {:?} != {:?}, retries={}",
                balance,
                Some(expected_balance),
                retries
            );
            if retries == 0 {
                assert_eq!(balance, Some(expected_balance));
            }
            sleep(Duration::from_millis(100));
        }
        consecutive_success += 1;

        info!("SUCCESS[{}] balance: {}", i, expected_balance,);

        if consecutive_success == 10 {
            info!("Took {} s to converge", duration_as_s(&start.elapsed()),);
            info!("Verifying signature of the last transaction in the validators");

            let mut num_nodes_behind = 0u64;
            validators.retain(|server| {
                let mut client = mk_client(&server.0);
                trace!("{} checking signature", server.0.id);
                num_nodes_behind += if client.check_signature(&sig) { 0 } else { 1 };
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

    info!("done!");
    assert_eq!(consecutive_success, 10);
    for (_, validator_exit) in validators {
        validator_exit();
    }
    server_exit();

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }
}

#[test]
#[ignore]
fn test_leader_to_validator_transition() {
    solana_logger::setup();

    // Make a dummy validator id to be the next leader
    let validator_keypair = Arc::new(Keypair::new());

    // Create the leader node information
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    let fullnode_config = FullnodeConfig::default();
    let ticks_per_slot = 5;

    let (mut genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(10_000, leader_info.id, 500);
    genesis_block.ticks_per_slot = ticks_per_slot;

    // Initialize the leader ledger. Make a mint and a genesis entry
    // in the leader ledger
    let (leader_ledger_path, last_id) = create_new_tmp_ledger!(&genesis_block);

    // Write the votes entries to the ledger that will cause leader rotation
    // to validator_keypair at slot 2
    {
        let blocktree = Blocktree::open_config(&leader_ledger_path, ticks_per_slot).unwrap();
        let (active_set_entries, _) = make_active_set_entries(
            &validator_keypair,
            &mint_keypair,
            100,
            1,
            &last_id,
            ticks_per_slot,
        );
        blocktree
            .write_entries(1, 0, 0, &active_set_entries)
            .unwrap();
    }
    info!("leader id: {}", leader_keypair.pubkey());
    info!("validator id: {}", validator_keypair.pubkey());

    // Start the leader node
    let voting_keypair = VotingKeypair::new_local(&leader_keypair);
    let leader = Fullnode::new(
        leader_node,
        &leader_keypair,
        &leader_ledger_path,
        voting_keypair,
        Some(&leader_info),
        &fullnode_config,
    );
    let (rotation_sender, rotation_receiver) = channel();
    let leader_exit = leader.run(Some(rotation_sender));

    let expected_rotations = vec![(FullnodeReturnType::LeaderToValidatorRotation, 2)];

    for expected_rotation in expected_rotations {
        loop {
            let transition = rotation_receiver.recv().unwrap();
            info!("leader transition: {:?}", transition);
            assert_eq!(transition, expected_rotation);
            break;
        }
    }

    info!("Shut down...");
    leader_exit();

    info!("Check the ledger to make sure it's the right height...");
    let bank_forks = new_banks_from_blocktree(&leader_ledger_path, None).0;
    let _bank = bank_forks.working_bank();

    remove_dir_all(leader_ledger_path).unwrap();
}

#[test]
#[ignore]
fn test_leader_validator_basic() {
    solana_logger::setup();

    // Create the leader node information
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    // Create the validator node information
    let validator_keypair = Arc::new(Keypair::new());
    let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

    info!("leader id: {}", leader_keypair.pubkey());
    info!("validator id: {}", validator_keypair.pubkey());

    // Create the leader scheduler config
    let fullnode_config = FullnodeConfig::default();
    let ticks_per_slot = 5;
    let (mut genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(10_000, leader_info.id, 500);
    genesis_block.ticks_per_slot = ticks_per_slot;

    // Make a common mint and a genesis entry for both leader + validator ledgers
    let (leader_ledger_path, last_id) = create_new_tmp_ledger!(&genesis_block);

    // Add validator vote on tick height 1
    {
        let blocktree = Blocktree::open_config(&leader_ledger_path, ticks_per_slot).unwrap();
        let (active_set_entries, _) = make_active_set_entries(
            &validator_keypair,
            &mint_keypair,
            100,
            1,
            &last_id,
            ticks_per_slot,
        );
        blocktree
            .write_entries(1, 0, 0, &active_set_entries)
            .unwrap();
    }

    // Initialize both leader + validator ledger
    let mut ledger_paths = Vec::new();
    ledger_paths.push(leader_ledger_path.clone());
    let validator_ledger_path = tmp_copy_blocktree!(&leader_ledger_path);
    ledger_paths.push(validator_ledger_path.clone());

    // Start the validator node
    let voting_keypair = VotingKeypair::new_local(&validator_keypair);
    let validator = Fullnode::new(
        validator_node,
        &validator_keypair,
        &validator_ledger_path,
        voting_keypair,
        Some(&leader_info),
        &fullnode_config,
    );
    let (validator_rotation_sender, validator_rotation_receiver) = channel();
    let validator_exit = validator.run(Some(validator_rotation_sender));

    // Start the leader fullnode
    let voting_keypair = VotingKeypair::new_local(&leader_keypair);
    let leader = Fullnode::new(
        leader_node,
        &leader_keypair,
        &leader_ledger_path,
        voting_keypair,
        Some(&leader_info),
        &fullnode_config,
    );
    let (leader_rotation_sender, leader_rotation_receiver) = channel();
    let leader_exit = leader.run(Some(leader_rotation_sender));

    converge(&leader_info, 2);

    //
    // The ledger was populated with slot 0 and slot 1, so the first rotation should occur at slot 2
    //

    info!("Waiting for slot 1 -> slot 2: bootstrap leader and the validator rotate");
    assert_eq!(
        validator_rotation_receiver.recv().unwrap(),
        (FullnodeReturnType::ValidatorToLeaderRotation, 2)
    );
    assert_eq!(
        leader_rotation_receiver.recv().unwrap(),
        (FullnodeReturnType::LeaderToValidatorRotation, 2),
    );

    info!("Waiting for slot 2 -> slot 3: validator remains the slot leader due to no votes");
    assert_eq!(
        validator_rotation_receiver.recv().unwrap(),
        (FullnodeReturnType::LeaderToLeaderRotation, 3)
    );
    assert_eq!(
        leader_rotation_receiver.recv().unwrap(),
        (FullnodeReturnType::LeaderToValidatorRotation, 3)
    );

    info!("Waiting for slot 3 -> slot 4: validator remains the slot leader due to no votes");
    assert_eq!(
        validator_rotation_receiver.recv().unwrap(),
        (FullnodeReturnType::LeaderToLeaderRotation, 4)
    );
    assert_eq!(
        leader_rotation_receiver.recv().unwrap(),
        (FullnodeReturnType::LeaderToValidatorRotation, 4)
    );

    info!("Shut down");
    validator_exit();
    leader_exit();

    // Check the ledger of the validator to make sure the entry height is correct
    // and that the old leader and the new leader's ledgers agree up to the point
    // of leader rotation
    let validator_entries: Vec<Entry> = read_ledger(&validator_ledger_path, ticks_per_slot);

    let leader_entries = read_ledger(&leader_ledger_path, ticks_per_slot);
    assert!(leader_entries.len() as u64 >= ticks_per_slot);

    for (v, l) in validator_entries.iter().zip(leader_entries) {
        assert_eq!(*v, l);
    }

    info!("done!");
    for path in ledger_paths {
        Blocktree::destroy(&path).expect("Expected successful database destruction");
        remove_dir_all(path).unwrap();
    }
}

#[test]
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

    // Create the common leader scheduling configuration
    let _slots_per_epoch = (N + 1) as u64;
    let ticks_per_slot = 5;
    let fullnode_config = FullnodeConfig::default();

    let (mut genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(10_000, bootstrap_leader_info.id, 500);
    genesis_block.ticks_per_slot = ticks_per_slot;

    // Make a common mint and a genesis entry for both leader + validator's ledgers
    let (genesis_ledger_path, last_id) = create_new_tmp_ledger!(&genesis_block);

    // Create the validator keypair that will be the next leader in line
    let next_leader_keypair = Arc::new(Keypair::new());

    // Create a common ledger with entries in the beginning that will add only
    // the "next_leader" validator to the active set for leader election, guaranteeing
    // they are the next leader after bootstrap_height
    let mut ledger_paths = Vec::new();
    ledger_paths.push(genesis_ledger_path.clone());

    // Make the entries to give the next_leader validator some stake so that they will be in
    // leader election active set
    {
        let blocktree = Blocktree::open_config(&genesis_ledger_path, ticks_per_slot).unwrap();
        let (active_set_entries, _) = make_active_set_entries(
            &next_leader_keypair,
            &mint_keypair,
            100,
            1,
            &last_id,
            ticks_per_slot,
        );
        blocktree
            .write_entries(1, 0, 0, &active_set_entries)
            .unwrap();
    }

    let next_leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
    ledger_paths.push(next_leader_ledger_path.clone());

    info!("bootstrap_leader: {}", bootstrap_leader_keypair.pubkey());
    info!("'next leader': {}", next_leader_keypair.pubkey());

    let voting_keypair = VotingKeypair::new_local(&bootstrap_leader_keypair);
    // Start up the bootstrap leader fullnode
    let bootstrap_leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
    ledger_paths.push(bootstrap_leader_ledger_path.clone());

    let bootstrap_leader = Fullnode::new(
        bootstrap_leader_node,
        &bootstrap_leader_keypair,
        &bootstrap_leader_ledger_path,
        voting_keypair,
        Some(&bootstrap_leader_info),
        &fullnode_config,
    );

    let (rotation_sender, rotation_receiver) = channel();
    let mut node_exits = vec![bootstrap_leader.run(Some(rotation_sender))];

    // Start up the validators other than the "next_leader" validator
    for i in 0..(N - 1) {
        let keypair = Arc::new(Keypair::new());
        let validator_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
        ledger_paths.push(validator_ledger_path.clone());
        let validator_id = keypair.pubkey();
        info!("validator {}: {}", i, validator_id);
        let validator_node = Node::new_localhost_with_pubkey(validator_id);
        let voting_keypair = VotingKeypair::new_local(&keypair);
        let validator = Fullnode::new(
            validator_node,
            &keypair,
            &validator_ledger_path,
            voting_keypair,
            Some(&bootstrap_leader_info),
            &fullnode_config,
        );

        node_exits.push(validator.run(None));
    }

    converge(&bootstrap_leader_info, N);

    info!("Wait for bootstrap_leader to transition to a validator",);
    loop {
        let transition = rotation_receiver.recv().unwrap();
        info!("bootstrap leader transition event: {:?}", transition);
        if transition.0 == FullnodeReturnType::LeaderToValidatorRotation {
            break;
        }
    }

    info!("Starting the 'next leader' node *after* rotation has occurred");
    let next_leader_node = Node::new_localhost_with_pubkey(next_leader_keypair.pubkey());
    let voting_keypair = VotingKeypair::new_local(&next_leader_keypair);
    let next_leader = Fullnode::new(
        next_leader_node,
        &next_leader_keypair,
        &next_leader_ledger_path,
        voting_keypair,
        Some(&bootstrap_leader_info),
        &FullnodeConfig::default(),
    );
    let (rotation_sender, _rotation_receiver) = channel();
    node_exits.push(next_leader.run(Some(rotation_sender)));

    info!("Wait for 'next leader' to assume leader role");
    // TODO: Once https://github.com/solana-labs/solana/issues/2482" is fixed,
    //      restore the commented out code below
    /*
    loop {
        let transition = _rotation_receiver.recv().unwrap();
        info!("next leader transition event: {:?}", transition);
        if transition == FullnodeReturnType::ValidatorToLeaderRotation {
            break;
        }
    }
    */

    info!("done!");
    for exit in node_exits {
        exit();
    }

    for path in ledger_paths {
        remove_dir_all(path).unwrap();
    }
}

#[test]
fn test_full_leader_validator_network() {
    solana_logger::setup();
    // The number of validators
    const N: usize = 2;

    // Create the common leader scheduling configuration
    let _slots_per_epoch = (N + 1) as u64;
    let ticks_per_slot = 5;
    let fullnode_config = FullnodeConfig::default();
    // Create the bootstrap leader node information
    let bootstrap_leader_keypair = Arc::new(Keypair::new());
    info!("bootstrap leader: {:?}", bootstrap_leader_keypair.pubkey());
    let bootstrap_leader_node = Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());
    let bootstrap_leader_info = bootstrap_leader_node.info.clone();

    let mut node_keypairs = VecDeque::new();

    // Create the validator keypairs
    for _ in 0..N {
        let validator_keypair = Arc::new(Keypair::new());
        node_keypairs.push_back(validator_keypair);
    }

    let (mut genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(10_000, bootstrap_leader_info.id, 500);
    genesis_block.ticks_per_slot = ticks_per_slot;

    // Make a common mint and a genesis entry for both leader + validator's ledgers
    let (bootstrap_leader_ledger_path, mut last_id) = create_new_tmp_ledger!(&genesis_block);

    // Create a common ledger with entries in the beginnging that will add all the validators
    // to the active set for leader election.
    let mut ledger_paths = Vec::new();
    ledger_paths.push(bootstrap_leader_ledger_path.clone());

    // Make entries to give each validator node some stake so that they will be in the
    // leader election active set
    let mut active_set_entries = vec![];
    for node_keypair in node_keypairs.iter() {
        let (node_active_set_entries, _) = make_active_set_entries(
            node_keypair,
            &mint_keypair,
            100,
            0,
            &last_id,
            ticks_per_slot,
        );
        last_id = node_active_set_entries.last().unwrap().id;
        active_set_entries.extend(node_active_set_entries);
    }

    {
        let blocktree =
            Blocktree::open_config(&bootstrap_leader_ledger_path, ticks_per_slot).unwrap();
        blocktree
            .write_entries(1, 0, 0, &active_set_entries)
            .unwrap();
    }

    let mut nodes = vec![];

    info!("Start up the validators");
    // Start up the validators
    for kp in node_keypairs.into_iter() {
        let validator_ledger_path = tmp_copy_blocktree!(&bootstrap_leader_ledger_path);

        ledger_paths.push(validator_ledger_path.clone());

        let validator_id = kp.pubkey();
        let validator_node = Node::new_localhost_with_pubkey(validator_id);
        let voting_keypair = VotingKeypair::new_local(&kp);
        info!("validator: {:?}", validator_id);
        let validator = Fullnode::new(
            validator_node,
            &kp,
            &validator_ledger_path,
            voting_keypair,
            Some(&bootstrap_leader_info),
            &fullnode_config,
        );

        let (rotation_sender, rotation_receiver) = channel();
        nodes.push((
            validator_id,
            validator.run(Some(rotation_sender)),
            rotation_receiver,
        ));
    }

    info!("Start up the bootstrap leader");
    let voting_keypair = VotingKeypair::new_local(&bootstrap_leader_keypair);
    let bootstrap_leader = Fullnode::new(
        bootstrap_leader_node,
        &bootstrap_leader_keypair,
        &bootstrap_leader_ledger_path,
        voting_keypair,
        Some(&bootstrap_leader_info),
        &fullnode_config,
    );
    let (bootstrap_leader_rotation_sender, bootstrap_leader_rotation_receiver) = channel();
    let bootstrap_leader_exit = bootstrap_leader.run(Some(bootstrap_leader_rotation_sender));

    converge(&bootstrap_leader_info, N + 1);

    // Wait for the bootstrap_leader to transition to a validator
    loop {
        let transition = bootstrap_leader_rotation_receiver.recv().unwrap();
        info!("bootstrap leader transition event: {:?}", transition);
        if transition.0 == FullnodeReturnType::LeaderToValidatorRotation {
            break;
        }
    }

    // Ensure each node in the cluster rotates into the leader role
    for (id, _, rotation_receiver) in &nodes {
        info!("Waiting for {:?} to become the leader", id);
        loop {
            let transition = rotation_receiver.recv().unwrap();
            info!("node {:?} transition event: {:?}", id, transition);
            if transition.0 == FullnodeReturnType::ValidatorToLeaderRotation {
                break;
            }
        }
    }

    info!("Exit all nodes");
    for node in nodes {
        node.1();
    }
    info!("Bootstrap leader exit");
    bootstrap_leader_exit();

    let mut node_entries = vec![];
    info!("Check that all the ledgers match");
    for ledger_path in ledger_paths.iter() {
        let entries = read_ledger(ledger_path, ticks_per_slot);
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
                    // TODO: This assert fails sometimes....why?
                    //assert_eq!(*expected_entry, next_entry);
                    if *expected_entry != next_entry {
                        error!("THIS IS A FAILURE.  SEE https://github.com/solana-labs/solana/issues/2481");
                        error!("* expected_entry: {:?}", *expected_entry);
                        error!("* next_entry: {:?}", next_entry);
                    }
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

    for path in ledger_paths {
        Blocktree::destroy(&path).expect("Expected successful database destruction");
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

    // Create the fullnode configuration
    let ticks_per_slot = 40;
    let slots_per_epoch = 2;
    let _ticks_per_epoch = slots_per_epoch * ticks_per_slot;

    let fullnode_config = FullnodeConfig::default();

    let (mut genesis_block, _mint_keypair) =
        GenesisBlock::new_with_leader(10_000, bootstrap_leader_info.id, 500);
    genesis_block.ticks_per_slot = ticks_per_slot;

    // Create leader ledger
    let (bootstrap_leader_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);

    let blob_receiver_exit = Arc::new(AtomicBool::new(false));

    // Create the listeners
    let mut listening_nodes: Vec<_> = (0..N)
        .map(|_| make_listening_node(&bootstrap_leader_info))
        .collect();

    let blob_fetch_stages: Vec<_> = listening_nodes
        .iter_mut()
        .map(|(_, _, node, _)| {
            let (blob_fetch_sender, blob_fetch_receiver) = channel();
            (
                BlobFetchStage::new(
                    Arc::new(node.sockets.tvu.pop().unwrap()),
                    &blob_fetch_sender,
                    blob_receiver_exit.clone(),
                ),
                blob_fetch_receiver,
            )
        })
        .collect();

    // Start up the bootstrap leader fullnode
    let bootstrap_leader_keypair = Arc::new(bootstrap_leader_keypair);
    let voting_keypair = VotingKeypair::new_local(&bootstrap_leader_keypair);

    let bootstrap_leader = Fullnode::new(
        bootstrap_leader_node,
        &bootstrap_leader_keypair,
        &bootstrap_leader_ledger_path,
        voting_keypair,
        Some(&bootstrap_leader_info),
        &fullnode_config,
    );

    let (bootstrap_leader_rotation_sender, bootstrap_leader_rotation_receiver) = channel();
    let bootstrap_leader_exit = bootstrap_leader.run(Some(bootstrap_leader_rotation_sender));

    // Wait for convergence
    converge(&bootstrap_leader_info, N + 1);

    info!("Waiting for leader rotation...");

    // Wait for the bootstrap_leader to move beyond slot 0
    loop {
        let transition = bootstrap_leader_rotation_receiver.recv().unwrap();
        info!("bootstrap leader transition event: {:?}", transition);
        if (FullnodeReturnType::LeaderToLeaderRotation, 1) == transition {
            break;
        }
    }
    info!("Shutting down the leader...");
    bootstrap_leader_exit();

    // Index of the last tick must be at least ticks_per_slot - 1
    let last_tick_entry_index = ticks_per_slot as usize - 1;
    let entries = read_ledger(&bootstrap_leader_ledger_path, ticks_per_slot);
    assert!(entries.len() >= last_tick_entry_index + 1);
    let expected_last_tick = &entries[last_tick_entry_index];
    debug!("last_tick_entry_index: {:?}", last_tick_entry_index);
    debug!("expected_last_tick: {:?}", expected_last_tick);

    info!("Check that the nodes got the last broadcasted blob");
    for (_, receiver) in blob_fetch_stages.iter() {
        info!("Checking a node...");
        let mut blobs = vec![];
        while let Ok(new_blobs) = receiver.try_recv() {
            blobs.extend(new_blobs);
        }

        for b in blobs {
            let b_r = b.read().unwrap();
            if b_r.index() == last_tick_entry_index as u64 {
                assert!(b_r.is_last_in_slot());
                debug!("last_tick_blob: {:?}", b_r);
                let actual_last_tick = &reconstruct_entries_from_blobs(vec![&*b_r])
                    .expect("Expected to be able to reconstruct entries from blob")
                    .0[0];
                assert_eq!(actual_last_tick, expected_last_tick);
                break;
            } else {
                assert!(!b_r.is_last_in_slot());
            }
        }
    }

    info!("done!");

    // Shut down blob fetch stages
    blob_receiver_exit.store(true, Ordering::Relaxed);
    for (bf, _) in blob_fetch_stages {
        bf.join().unwrap();
    }

    // Shut down the listeners
    for node in listening_nodes {
        node.0.close().unwrap();
    }
    remove_dir_all(bootstrap_leader_ledger_path).unwrap();
}

fn send_tx_and_retry_get_balance(
    leader: &NodeInfo,
    alice: &Keypair,
    bob_pubkey: &Pubkey,
    transfer_amount: u64,
    expected: Option<u64>,
) -> Option<u64> {
    let mut client = mk_client(leader);
    trace!("getting leader last_id");
    let last_id = client.get_last_id();
    let mut tx = SystemTransaction::new_account(&alice, *bob_pubkey, transfer_amount, last_id, 0);
    info!(
        "executing transfer of {} from {} to {}",
        transfer_amount,
        alice.pubkey(),
        *bob_pubkey
    );
    if client.retry_transfer(&alice, &mut tx, 5).is_err() {
        None
    } else {
        retry_get_balance(&mut client, bob_pubkey, expected)
    }
}

fn retry_send_tx_and_retry_get_balance(
    leader: &NodeInfo,
    alice: &Keypair,
    bob_pubkey: &Pubkey,
    expected: Option<u64>,
) -> Option<u64> {
    let mut client = mk_client(leader);
    trace!("getting leader last_id");
    let last_id = client.get_last_id();
    info!("executing leader transfer");
    const LAST: usize = 30;
    for run in 0..(LAST + 1) {
        let _sig = client.transfer(500, &alice, *bob_pubkey, &last_id).unwrap();
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

fn new_fullnode() -> (Arc<Keypair>, Node, ContactInfo) {
    let keypair = Arc::new(Keypair::new());
    let node = Node::new_localhost_with_pubkey(keypair.pubkey());
    let node_info = node.info.clone();
    (keypair, node, node_info)
}

fn new_genesis_block(
    leader: Pubkey,
    ticks_per_slot: u64,
    slots_per_epoch: u64,
) -> (GenesisBlock, Keypair) {
    let (mut genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(1_000_000_000_000_000_000, leader, 100);
    genesis_block.ticks_per_slot = ticks_per_slot;
    genesis_block.slots_per_epoch = slots_per_epoch;

    (genesis_block, mint_keypair)
}

fn fund_fullnode(
    from: &Arc<Keypair>,
    to: Pubkey,
    tokens: u64,
    last_tick: &Hash,
    last_hash: &mut Hash,
    entries: &mut Vec<Entry>,
) {
    let transfer_tx = SystemTransaction::new_account(from, to, tokens, *last_tick, 0);
    let transfer_entry = next_entry_mut(last_hash, 1, vec![transfer_tx]);

    entries.extend(vec![transfer_entry]);
}

fn stake_fullnode(
    node: &Arc<Keypair>,
    stake: u64,
    last_tick: &Hash,
    last_hash: &mut Hash,
    entries: &mut Vec<Entry>,
) -> VotingKeypair {
    // Create and register a vote account for active_keypair
    let voting_keypair = VotingKeypair::new_local(node);
    let vote_account_id = voting_keypair.pubkey();

    let new_vote_account_tx =
        VoteTransaction::fund_staking_account(node, vote_account_id, *last_tick, stake, 0);
    let new_vote_account_entry = next_entry_mut(last_hash, 1, vec![new_vote_account_tx]);
    /*
    let vote_tx = VoteTransaction::new_vote(&voting_keypair, 1, *last_tick, 0);
    let vote_entry = next_entry_mut(last_hash, 1, vec![vote_tx]);

    entries.extend(vec![new_vote_account_entry, vote_entry]);
    */
    entries.extend(vec![new_vote_account_entry]);
    voting_keypair
}

fn add_tick(last_id: &mut Hash, entries: &mut Vec<Entry>) -> Hash {
    let tick = solana::entry::create_ticks(1, *last_id);
    *last_id = tick[0].id;
    entries.extend(tick);
    *last_id
}

fn start_fullnode(
    node: Node,
    kp: Arc<Keypair>,
    v_kp: VotingKeypair,
    ledger: &str,
    leader: Option<&NodeInfo>,
    config: &FullnodeConfig,
) -> (impl FnOnce(), Receiver<(FullnodeReturnType, u64)>) {
    let (rotation_sender, rotation_receiver) = channel();
    let fullnode = Fullnode::new(node, &kp, ledger, v_kp, leader, &config);
    (fullnode.run(Some(rotation_sender)), rotation_receiver)
}

fn new_non_bootstrap_leader_fullnode(
    mint: &Arc<Keypair>,
    last_tick: &mut Hash,
    mut last_id: &mut Hash,
    entries: &mut Vec<Entry>,
) -> (Node, Arc<Keypair>, VotingKeypair) {
    // Create the node information
    let (node_keypair, node, _) = new_fullnode();

    let voting = {
        fund_fullnode(
            mint,
            node_keypair.pubkey(),
            50,
            &last_tick,
            &mut last_id,
            entries,
        );
        let voting = stake_fullnode(&node_keypair, 10, &last_tick, &mut last_id, entries);

        *last_tick = add_tick(last_id, entries);
        voting
    };

    (node, node_keypair, voting)
}

fn new_bootstrap_leader_fullnode(
    ticks_per_slot: u64,
    slots_per_epoch: u64,
    entries: &mut Vec<Entry>,
) -> (
    Arc<Keypair>,
    String,
    Hash,
    Hash,
    (Node, Arc<Keypair>, VotingKeypair),
) {
    // Create the node information
    let (node_keypair, node, _) = new_fullnode();

    let (genesis_block, mint_keypair) =
        new_genesis_block(node_keypair.pubkey(), ticks_per_slot, slots_per_epoch);

    let (ledger_path, mut last_id) = create_new_tmp_ledger!(&genesis_block);

    let mut last_tick = add_tick(&mut last_id, entries);

    let voting = stake_fullnode(&node_keypair, 20, &mut last_tick, &mut last_id, entries);

    (
        Arc::new(mint_keypair),
        ledger_path,
        last_tick,
        last_id,
        (node, node_keypair, voting),
    )
}

#[test]
#[ignore]
fn test_fullnodes_bootup() {
    let ticks_per_slot = 1;
    let slots_per_epoch = 1;
    solana_logger::setup();

    // Create fullnode config, and set node scheduler policies
    let fullnode_config = FullnodeConfig::default();
    // let (tick_step_sender, tick_step_receiver) = sync_channel(1);
    // fullnode_config.tick_config = PohServiceConfig::Step(tick_step_sender);

    let mut entries = vec![];

    let (mint, ledger, mut last_tick, mut last_id, leader) =
        new_bootstrap_leader_fullnode(ticks_per_slot, slots_per_epoch, &mut entries);

    let leader_info = leader.0.info.clone();

    let validator =
        new_non_bootstrap_leader_fullnode(&mint, &mut last_tick, &mut last_id, &mut entries);

    {
        info!("Number of entries {}", entries.len());
        trace!("last_id: {:?}", last_id);
        trace!("last_tick: {:?}", last_tick);
        trace!("entries: {:?}", entries);

        let blocktree = Blocktree::open_config(&ledger, ticks_per_slot).unwrap();
        blocktree.write_entries(1, 0, 0, &entries).unwrap();
    }

    //    let validator_info = validator.0.info.clone();
    let validator_ledger = tmp_copy_blocktree!(&ledger);

    let mut exits = vec![];
    let (validator_exit, validator_rotation_receiver) = start_fullnode(
        validator.0,
        validator.1,
        validator.2,
        &validator_ledger,
        Some(&leader_info),
        &fullnode_config,
    );
    exits.push(validator_exit);

    let (leader_exit, leader_rotation_receiver) = start_fullnode(
        leader.0,
        leader.1,
        leader.2,
        &ledger,
        None,
        &fullnode_config,
    );
    exits.push(leader_exit);

    converge(&leader_info, exits.len());
    info!(
        "found leader: {:?}",
        poll_gossip_for_leader(leader_info.gossip, Some(5)).unwrap()
    );

    let mut leader_should_be_leader = true;
    let mut validator_should_be_leader = false;

    let mut leader_slot_height_of_next_rotation = 4;
    let mut validator_slot_height_of_next_rotation = 4;

    let max_slot_height = 8;
    /*
        let bob = Keypair::new().pubkey();
        let mut client_last_id = solana_sdk::hash::Hash::default();
    */
    while leader_slot_height_of_next_rotation < max_slot_height
        && validator_slot_height_of_next_rotation < max_slot_height
    {
        // Check for leader rotation
        {
            match leader_rotation_receiver.try_recv() {
                Ok((rotation_type, slot)) => {
                    if slot == 0 {
                        // Skip slot 0, as the nodes are not fully initialized in terms of leader scheduler
                        continue;
                    }
                    info!(
                        "leader rotation event {:?} at slot={} {}",
                        rotation_type, slot, leader_slot_height_of_next_rotation
                    );
                    info!("leader should be leader? {}", leader_should_be_leader);
                    assert_eq!(slot, leader_slot_height_of_next_rotation);
                    assert_eq!(
                        rotation_type,
                        if leader_should_be_leader {
                            FullnodeReturnType::LeaderToValidatorRotation
                        } else {
                            FullnodeReturnType::ValidatorToLeaderRotation
                        }
                    );
                    leader_should_be_leader = !leader_should_be_leader;
                    leader_slot_height_of_next_rotation += 1;
                }
                Err(TryRecvError::Empty) => {}
                err => panic!(err),
            }
        }

        // Check for validator rotation
        match validator_rotation_receiver.try_recv() {
            Ok((rotation_type, slot)) => {
                if slot == 0 {
                    // Skip slot 0, as the nodes are not fully initialized in terms of leader scheduler
                    continue;
                }
                info!(
                    "validator rotation event {:?} at slot={} {}",
                    rotation_type, slot, validator_slot_height_of_next_rotation
                );
                info!("validator should be leader? {}", validator_should_be_leader);
                assert_eq!(slot, validator_slot_height_of_next_rotation);
                assert_eq!(
                    rotation_type,
                    if validator_should_be_leader {
                        FullnodeReturnType::LeaderToValidatorRotation
                    } else {
                        FullnodeReturnType::ValidatorToLeaderRotation
                    }
                );
                validator_slot_height_of_next_rotation += 1;
                validator_should_be_leader = !validator_should_be_leader;
            }
            Err(TryRecvError::Empty) => {}
            err => panic!(err),
        }
    }
}

fn test_fullnode_rotate(
    ticks_per_slot: u64,
    slots_per_epoch: u64,
    include_validator: bool,
    transact: bool,
) {
    solana_logger::setup();

    let mut leader_should_be_leader = true;
    let mut leader_slot_height_of_next_rotation = 2;

    // Create fullnode config, and set node scheduler policies
    let mut fullnode_config = FullnodeConfig::default();
    let (tick_step_sender, tick_step_receiver) = sync_channel(1);
    fullnode_config.tick_config = PohServiceConfig::Step(tick_step_sender);

    let mut entries = vec![];

    let (mint, ledger, mut last_tick, mut last_id, leader) =
        new_bootstrap_leader_fullnode(ticks_per_slot, slots_per_epoch, &mut entries);

    let leader_info = leader.0.info.clone();

    let validator = if include_validator {
        leader_slot_height_of_next_rotation += 1;
        Some(new_non_bootstrap_leader_fullnode(
            &mint,
            &mut last_tick,
            &mut last_id,
            &mut entries,
        ))
    } else {
        None
    };

    {
        trace!("last_id: {:?}", last_id);
        trace!("last_tick: {:?}", last_tick);
        trace!("entries: {:?}", entries);

        let blocktree = Blocktree::open_config(&ledger, ticks_per_slot).unwrap();
        blocktree.write_entries(1, 0, 0, &entries).unwrap();
    }

    let mut ledger_paths = Vec::new();
    ledger_paths.push(ledger.clone());
    info!("ledger is {}", ledger);

    let validator_ledger = tmp_copy_blocktree!(&ledger);
    ledger_paths.push(validator_ledger.clone());

    let mut node_exits = vec![];

    let validator_rotation_receiver = if let Some(node) = validator {
        let (validator_exit, validator_rotation_receiver) = start_fullnode(
            node.0,
            node.1,
            node.2,
            &validator_ledger,
            Some(&leader_info),
            &fullnode_config,
        );
        node_exits.push(validator_exit);
        validator_rotation_receiver
    } else {
        channel().1
    };

    let (leader_exit, leader_rotation_receiver) = start_fullnode(
        leader.0,
        leader.1,
        leader.2,
        &ledger,
        None,
        &fullnode_config,
    );
    node_exits.push(leader_exit);

    converge(&leader_info, node_exits.len());
    info!(
        "found leader: {:?}",
        poll_gossip_for_leader(leader_info.gossip, Some(5)).unwrap()
    );

    let bob = Keypair::new().pubkey();
    let mut expected_bob_balance = 0;

    let mut client_last_id = solana_sdk::hash::Hash::default();

    let mut validator_should_be_leader = !leader_should_be_leader;
    let mut validator_slot_height_of_next_rotation = leader_slot_height_of_next_rotation;

    let mut log_spam = 0;
    let max_slot_height = 5;
    while leader_slot_height_of_next_rotation < max_slot_height
        && validator_slot_height_of_next_rotation < max_slot_height
    {
        // Check for leader rotation
        {
            match leader_rotation_receiver.try_recv() {
                Ok((rotation_type, slot)) => {
                    if slot < leader_slot_height_of_next_rotation {
                        // Skip slot 0, as the nodes are not fully initialized in terms of leader scheduler
                        continue;
                    }
                    info!(
                        "leader rotation event {:?} at slot={} {}",
                        rotation_type, slot, leader_slot_height_of_next_rotation
                    );
                    info!("leader should be leader? {}", leader_should_be_leader);
                    assert_eq!(slot, leader_slot_height_of_next_rotation);
                    if include_validator {
                        assert_eq!(
                            rotation_type,
                            if leader_should_be_leader {
                                FullnodeReturnType::LeaderToValidatorRotation
                            } else {
                                FullnodeReturnType::ValidatorToLeaderRotation
                            }
                        );
                        leader_should_be_leader = !leader_should_be_leader;
                    } else {
                        assert_eq!(rotation_type, FullnodeReturnType::LeaderToLeaderRotation);
                    }
                    leader_slot_height_of_next_rotation += 1;
                }
                Err(TryRecvError::Empty) => {}
                err => panic!(err),
            }
        }

        // Check for validator rotation
        if include_validator {
            match validator_rotation_receiver.try_recv() {
                Ok((rotation_type, slot)) => {
                    if slot < validator_slot_height_of_next_rotation {
                        // Skip slot 0, as the nodes are not fully initialized in terms of leader scheduler
                        continue;
                    }
                    info!(
                        "validator rotation event {:?} at slot={} {}",
                        rotation_type, slot, validator_slot_height_of_next_rotation
                    );
                    info!("validator should be leader? {}", validator_should_be_leader);
                    assert_eq!(slot, validator_slot_height_of_next_rotation);
                    assert_eq!(
                        rotation_type,
                        if validator_should_be_leader {
                            FullnodeReturnType::LeaderToValidatorRotation
                        } else {
                            FullnodeReturnType::ValidatorToLeaderRotation
                        }
                    );
                    validator_slot_height_of_next_rotation += 1;
                    validator_should_be_leader = !validator_should_be_leader;
                }
                Err(TryRecvError::Empty) => {}
                err => panic!(err),
            }
        }

        if transact {
            let mut client = mk_client(&leader_info);
            client_last_id = client.get_next_last_id_ext(&client_last_id, &|| {
                tick_step_receiver.recv().expect("tick step");
                sleep(Duration::from_millis(100));
            });
            info!("Transferring 500 tokens, last_id={:?}", client_last_id);
            expected_bob_balance += 500;

            let signature = client.transfer(500, &mint, bob, &client_last_id).unwrap();
            debug!("transfer send, signature is {:?}", signature);
            for _ in 0..30 {
                if client.poll_for_signature(&signature).is_err() {
                    tick_step_receiver.recv().expect("tick step");
                    info!("poll for signature tick step received");
                } else {
                    break;
                }
            }
            debug!("transfer signature confirmed");
            let actual_bob_balance =
                retry_get_balance(&mut client, &bob, Some(expected_bob_balance)).unwrap();
            assert_eq!(actual_bob_balance, expected_bob_balance);
            debug!("account balance confirmed: {}", actual_bob_balance);

            client_last_id = client.get_next_last_id_ext(&client_last_id, &|| {
                tick_step_receiver.recv().expect("tick step");
                sleep(Duration::from_millis(100));
            });
        } else {
            log_spam += 1;
            if log_spam % 25 == 0 {
                if include_validator {
                    trace!("waiting for leader and validator to reach max slot height...");
                } else {
                    trace!("waiting for leader to reach max slot height...");
                }
            }
        }
        tick_step_receiver.recv().expect("tick step");
    }

    if transact {
        // Make sure at least one transfer succeeded.
        assert!(expected_bob_balance > 0);
    }

    info!("Shutting down");
    drop(tick_step_receiver);
    for node_exit in node_exits {
        node_exit();
    }

    for path in ledger_paths {
        Blocktree::destroy(&path)
            .unwrap_or_else(|err| warn!("Expected successful database destruction: {:?}", err));
        remove_dir_all(path).unwrap();
    }

    trace!(
        "final validator_slot_height_of_next_rotation: {}",
        validator_slot_height_of_next_rotation
    );
    trace!(
        "final leader_slot_height_of_next_rotation: {}",
        leader_slot_height_of_next_rotation
    );
    trace!("final leader_should_be_leader: {}", leader_should_be_leader);
    trace!(
        "final validator_should_be_leader: {}",
        validator_should_be_leader
    );
}

#[test]
fn test_one_fullnode_rotate_every_tick_without_transactions() {
    test_fullnode_rotate(1, 1, false, false);
}

#[test]
fn test_one_fullnode_rotate_every_second_tick_without_transactions() {
    test_fullnode_rotate(2, 1, false, false);
}

#[test]
#[ignore]
fn test_two_fullnodes_rotate_every_tick_without_transactions() {
    test_fullnode_rotate(1, 1, true, false);
}

#[test]
#[ignore]
fn test_two_fullnodes_rotate_every_second_tick_without_transactions() {
    test_fullnode_rotate(2, 1, true, false);
}

#[test]
fn test_one_fullnode_rotate_every_tick_with_transactions() {
    test_fullnode_rotate(1, 1, false, true);
}

#[test]
#[ignore]
fn test_two_fullnodes_rotate_every_tick_with_transactions() {
    test_fullnode_rotate(1, 1, true, true);
}
