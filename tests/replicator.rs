#[macro_use]
extern crate log;
extern crate solana;
extern crate solana_sdk;

use solana::client::mk_client;
use solana::cluster_info::Node;
use solana::db_ledger::DbLedger;
use solana::fullnode::Fullnode;
use solana::leader_scheduler::LeaderScheduler;
use solana::ledger::{create_tmp_genesis, get_tmp_ledger_path, read_ledger};
use solana::logger;
use solana::replicator::Replicator;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::fs::remove_dir_all;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

#[test]
fn test_replicator_startup() {
    logger::setup();
    info!("starting replicator test");
    let entry_height = 0;
    let replicator_ledger_path = &get_tmp_ledger_path("replicator_test_replicator_ledger");

    let exit = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));

    info!("starting leader node");
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let network_addr = leader_node.sockets.gossip.local_addr().unwrap();
    let leader_info = leader_node.info.clone();
    let vote_account_keypair = Arc::new(Keypair::new());

    let leader_ledger_path = "replicator_test_leader_ledger";
    let (mint, leader_ledger_path) = create_tmp_genesis(leader_ledger_path, 100, leader_info.id, 1);

    {
        let leader = Fullnode::new(
            leader_node,
            &leader_ledger_path,
            leader_keypair,
            vote_account_keypair,
            None,
            false,
            LeaderScheduler::from_bootstrap_leader(leader_info.id),
            None,
        );

        let mut leader_client = mk_client(&leader_info);

        let bob = Keypair::new();

        let last_id = leader_client.get_last_id();
        leader_client
            .transfer(1, &mint.keypair(), bob.pubkey(), &last_id)
            .unwrap();

        let replicator_keypair = Keypair::new();

        info!("starting replicator node");
        let replicator_node = Node::new_localhost_with_pubkey(replicator_keypair.pubkey());
        let (replicator, _leader_info) = Replicator::new(
            entry_height,
            1,
            &exit,
            Some(replicator_ledger_path),
            replicator_node,
            Some(network_addr),
            done.clone(),
        );

        let mut num_entries = 0;
        for _ in 0..60 {
            match read_ledger(replicator_ledger_path, true) {
                Ok(entries) => {
                    for _ in entries {
                        num_entries += 1;
                    }
                    info!("{} entries", num_entries);
                    if num_entries > 0 {
                        break;
                    }
                }
                Err(e) => {
                    info!("error reading ledger: {:?}", e);
                }
            }
            sleep(Duration::from_millis(300));
            let last_id = leader_client.get_last_id();
            leader_client
                .transfer(1, &mint.keypair(), bob.pubkey(), &last_id)
                .unwrap();
        }
        assert_eq!(done.load(Ordering::Relaxed), true);
        assert!(num_entries > 0);
        exit.store(true, Ordering::Relaxed);
        replicator.join();
        leader.exit();
    }

    DbLedger::destroy(&leader_ledger_path).expect("Expected successful database destuction");
    DbLedger::destroy(&replicator_ledger_path).expect("Expected successful database destuction");
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}
