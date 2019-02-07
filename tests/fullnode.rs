use log::*;
use solana::client::mk_client;
use solana::cluster_info::Node;
use solana::db_ledger::create_tmp_sample_ledger;
use solana::fullnode::{Fullnode, FullnodeConfig};
use solana::thin_client::{poll_gossip_for_leader, retry_get_balance};
use solana::voting_keypair::VotingKeypair;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::sync::mpsc::{channel, TryRecvError};
use std::sync::Arc;
use std::thread::spawn;

#[test]
fn test_fullnode_transact_while_rotating_fast() {
    solana_logger::setup();

    let leader_keypair = Arc::new(Keypair::new());
    let leader_pubkey = leader_keypair.pubkey().clone();
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader.info.clone();
    let (mint, leader_ledger_path, _last_entry_height, _last_entry_id) = create_tmp_sample_ledger(
        "fullnode_transact_while_rotating_fast",
        1_000_000_000_000_000_000,
        0,
        leader_pubkey,
        123,
    );

    // Setup the cluster with a single node that attempts to leader rotate after every tick
    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.leader_scheduler_config.ticks_per_slot = 1;
    let leader_fullnode = Fullnode::new(
        leader,
        &leader_keypair,
        &leader_ledger_path,
        VotingKeypair::new_local(&leader_keypair),
        None,
        &fullnode_config,
    );
    let (leader_rotation_sender, leader_rotation_receiver) = channel();
    let leader_fullnode_exit = leader_fullnode.run(Some(leader_rotation_sender));
    info!(
        "found leader: {:?}",
        poll_gossip_for_leader(leader_info.gossip, Some(5)).unwrap()
    );

    let _transition_monitor = spawn(move || loop {
        match leader_rotation_receiver.try_recv() {
            Ok(transition) => {
                info!("leader transition event: {:?}", transition);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                break;
            }
        };
    });

    let bob = Keypair::new().pubkey();
    let mut expected_bob_balance = 0;

    // Send a bunch of simple transactions at the node, ensure they are all processed correctly
    let mut client = mk_client(&leader_info);
    let mut last_id = client.get_last_id();
    for i in 0..100 {
        info!("Iteration #{}, last_id={:?}", i, last_id);
        expected_bob_balance += 500;

        let signature = client.transfer(500, &mint, bob, &last_id).unwrap();
        debug!("sent transfer, signature is {:?}", signature);
        std::thread::sleep(std::time::Duration::from_millis(100));
        client.poll_for_signature(&signature).unwrap();
        assert!(client.check_signature(&signature)); // TODO: use ^^^
        let actual_bob_balance =
            retry_get_balance(&mut client, &bob, Some(expected_bob_balance)).unwrap();
        assert_eq!(actual_bob_balance, expected_bob_balance);
        debug!("account balance confirmed");

        last_id = client.get_next_last_id(&last_id);
    }

    info!("Shutting down");
    leader_fullnode_exit();
}
