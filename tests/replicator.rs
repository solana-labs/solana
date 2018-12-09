#[macro_use]
extern crate log;

use solana::client::mk_client;
use solana::cluster_info::{Node, NodeInfo};
use solana::db_ledger::DbLedger;
use solana::fullnode::Fullnode;
use solana::leader_scheduler::LeaderScheduler;
use solana::ledger::{create_tmp_genesis, get_tmp_ledger_path, read_ledger};
use solana::logger;
use solana::replicator::Replicator;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::fs::remove_dir_all;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

#[test]
fn test_replicator_startup() {
    logger::setup();
    info!("starting replicator test");
    let replicator_ledger_path = &get_tmp_ledger_path("replicator_test_replicator_ledger");

    info!("starting leader node");
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
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

        leader_client
            .transfer(1, &mint.keypair(), replicator_keypair.pubkey(), &last_id)
            .unwrap();

        info!("starting replicator node");
        let replicator_node = Node::new_localhost_with_pubkey(replicator_keypair.pubkey());

        let leader_info = NodeInfo::new_entry_point(&leader_info.gossip);

        let replicator = Replicator::new(
            Some(replicator_ledger_path),
            replicator_node,
            &leader_info,
            &replicator_keypair,
        )
        .unwrap();

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
        assert!(num_entries > 0);
        replicator.close();
        leader.exit();
    }

    DbLedger::destroy(&leader_ledger_path).expect("Expected successful database destuction");
    DbLedger::destroy(&replicator_ledger_path).expect("Expected successful database destuction");
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}
