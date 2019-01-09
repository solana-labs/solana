#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_json;

use bincode::deserialize;
use solana::client::mk_client;
use solana::cluster_info::{ClusterInfo, Node, NodeInfo};
use solana::create_vote_account::*;
use solana::db_ledger::DbLedger;
use solana::entry::Entry;
use solana::fullnode::Fullnode;
use solana::leader_scheduler::LeaderScheduler;
use solana::ledger::{create_tmp_genesis, get_tmp_ledger_path, tmp_copy_ledger};
use solana::replicator::Replicator;
use solana::rpc_request::{RpcClient, RpcRequest};
use solana::streamer::blob_receiver;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use std::fs::remove_dir_all;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_replicator_startup() {
    solana_logger::setup();
    info!("starting replicator test");
    let replicator_ledger_path = &get_tmp_ledger_path("replicator_test_replicator_ledger");

    info!("starting leader node");
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    let leader_ledger_path = "replicator_test_leader_ledger";
    let (mint, leader_ledger_path) = create_tmp_genesis(leader_ledger_path, 100, leader_info.id, 1);

    let validator_ledger_path =
        tmp_copy_ledger(&leader_ledger_path, "replicator_test_validator_ledger");

    {
        let (signer, t_signer, signer_exit) = local_vote_signer_service().unwrap();
        let rpc_client = RpcClient::new_from_socket(signer);

        let msg = "Registering a new node";
        let sig = Signature::new(&leader_keypair.sign(msg.as_bytes()).as_ref());

        let params = json!([leader_keypair.pubkey(), sig, msg.as_bytes()]);
        let resp = RpcRequest::RegisterNode
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap();
        let vote_account_id: Pubkey = serde_json::from_value(resp).unwrap();

        let leader = Fullnode::new(
            leader_node,
            &leader_ledger_path,
            leader_keypair,
            &vote_account_id,
            &signer,
            None,
            false,
            LeaderScheduler::from_bootstrap_leader(leader_info.id.clone()),
            None,
        );

        let validator_keypair = Arc::new(Keypair::new());
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

        let msg = "Registering a new node";
        let sig = Signature::new(&validator_keypair.sign(msg.as_bytes()).as_ref());

        let params = json!([validator_keypair.pubkey(), sig, msg.as_bytes()]);
        let resp = RpcRequest::RegisterNode
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap();
        let vote_account_id: Pubkey = serde_json::from_value(resp).unwrap();

        #[cfg(feature = "chacha")]
        let validator_node_info = validator_node.info.clone();

        let validator = Fullnode::new(
            validator_node,
            &validator_ledger_path,
            validator_keypair,
            &vote_account_id,
            &signer,
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
        let replicator_info = replicator_node.info.clone();

        let leader_info = NodeInfo::new_entry_point(&leader_info.gossip);

        let replicator = Replicator::new(
            Some(replicator_ledger_path),
            replicator_node,
            &leader_info,
            &replicator_keypair,
        )
        .unwrap();

        // Create a client which downloads from the replicator and see that it
        // can respond with blobs.
        let tn = Node::new_localhost();
        let cluster_info = ClusterInfo::new(tn.info.clone());
        let repair_index = 1;
        let req = cluster_info
            .window_index_request_bytes(repair_index)
            .unwrap();

        let exit = Arc::new(AtomicBool::new(false));
        let (s_reader, r_reader) = channel();
        let repair_socket = Arc::new(tn.sockets.repair);
        let t_receiver = blob_receiver(repair_socket.clone(), exit.clone(), s_reader);

        info!(
            "Sending repair requests from: {} to: {}",
            tn.info.id, replicator_info.gossip
        );

        let mut num_txs = 0;
        for _ in 0..5 {
            repair_socket.send_to(&req, replicator_info.gossip).unwrap();

            let x = r_reader.recv_timeout(Duration::new(1, 0));

            if let Ok(blobs) = x {
                for b in blobs {
                    let br = b.read().unwrap();
                    assert!(br.index().unwrap() == repair_index);
                    let entry: Entry = deserialize(&br.data()[..br.meta.size]).unwrap();
                    info!("entry: {:?}", entry);
                    num_txs = entry.transactions.len();
                }
                break;
            }
        }
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().unwrap();

        // The replicator will not submit storage proofs if
        // chacha is not enabled
        #[cfg(feature = "chacha")]
        {
            use solana::rpc_request::{RpcClient, RpcRequest};
            use std::thread::sleep;

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
        assert!(num_txs != 0);
        stop_local_vote_signer_service(t_signer, &signer_exit);

        replicator.close();
        validator.exit();
        leader.close().expect("Expected successful node closure");
    }

    DbLedger::destroy(&leader_ledger_path).expect("Expected successful database destruction");
    DbLedger::destroy(&replicator_ledger_path).expect("Expected successful database destruction");
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}
