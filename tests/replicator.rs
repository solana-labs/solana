#[macro_use]
extern crate log;

#[cfg(feature = "chacha")]
#[macro_use]
extern crate serde_json;

use solana::client::mk_client;
use solana::cluster_info::{Node, NodeInfo};
use solana::db_ledger::DbLedger;
use solana::fullnode::Fullnode;
use solana::leader_scheduler::LeaderScheduler;
use solana::ledger::{create_tmp_genesis, get_tmp_ledger_path, read_ledger, tmp_copy_ledger};
use solana::replicator::Replicator;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use std::fs::remove_dir_all;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

#[test]
fn test_replicator_startup() {
    info!("starting replicator test");
    let replicator_ledger_path = &get_tmp_ledger_path("replicator_test_replicator_ledger");

    info!("starting leader node");
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();
    let vote_account_keypair = Arc::new(Keypair::new());

    let leader_ledger_path = "replicator_test_leader_ledger";
    let (mint, leader_ledger_path) = create_tmp_genesis(leader_ledger_path, 100, leader_info.id, 1);

    let validator_ledger_path =
        tmp_copy_ledger(&leader_ledger_path, "replicator_test_validator_ledger");

    {
        let leader = Fullnode::new(
            leader_node,
            &leader_ledger_path,
            leader_keypair,
            &vote_account_keypair.pubkey(),
            &SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            None,
            false,
            LeaderScheduler::from_bootstrap_leader(leader_info.id.clone()),
            None,
        );

        let validator_keypair = Arc::new(Keypair::new());
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

        #[cfg(feature = "chacha")]
        let validator_node_info = validator_node.info.clone();

        let validator = Fullnode::new(
            validator_node,
            &validator_ledger_path,
            validator_keypair,
            vote_account_keypair,
            Some(leader_info.gossip),
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

        // Give the replicator some tokens
        let amount = 1;
        let mut tx = Transaction::system_new(
            &mint.keypair(),
            replicator_keypair.pubkey(),
            amount,
            last_id,
        );
        leader_client
            .retry_transfer(&mint.keypair(), &mut tx, 5)
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

        // Poll the ledger dir to see that some is downloaded
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

            // Do a transfer to make sure new entries are created which
            // stimulates the repair process
            let last_id = leader_client.get_last_id();
            leader_client
                .transfer(1, &mint.keypair(), bob.pubkey(), &last_id)
                .unwrap();
        }

        // The replicator will not submit storage proofs if
        // chacha is not enabled
        #[cfg(feature = "chacha")]
        {
            use solana::rpc_request::{RpcClient, RpcRequest};

            let rpc_client = RpcClient::new_from_socket(validator_node_info.rpc);
            let mut non_zero_pubkeys = false;
            for _ in 0..30 {
                let params = json!([0]);
                let pubkeys = RpcRequest::GetStoragePubkeysForEntryHeight
                    .make_rpc_request(&rpc_client, 1, Some(params))
                    .unwrap();
                info!("pubkeys: {:?}", pubkeys);
                if pubkeys.as_array().unwrap().len() != 0 {
                    non_zero_pubkeys = true;
                    break;
                }
                sleep(Duration::from_secs(1));
            }
            assert!(non_zero_pubkeys);
        }

        // Check that some ledger was downloaded
        assert!(num_entries > 0);

        replicator.close();
        validator.exit();
        leader.close().expect("Expected successful node closure");
    }

    DbLedger::destroy(&leader_ledger_path).expect("Expected successful database destruction");
    DbLedger::destroy(&replicator_ledger_path).expect("Expected successful database destruction");
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}
