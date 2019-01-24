#[macro_use]
extern crate log;

#[cfg(feature = "chacha")]
#[macro_use]
extern crate serde_json;

use bincode::deserialize;
use solana::client::mk_client;
use solana::cluster_info::{ClusterInfo, Node, NodeInfo};
use solana::db_ledger::DbLedger;
use solana::db_ledger::{create_tmp_genesis, get_tmp_ledger_path, tmp_copy_ledger};
use solana::entry::Entry;
use solana::fullnode::Fullnode;
use solana::leader_scheduler::LeaderScheduler;
use solana::replicator::Replicator;
use solana::storage_stage::STORAGE_ROTATE_TEST_COUNT;
use solana::streamer::blob_receiver;
use solana::vote_signer_proxy::VoteSignerProxy;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use solana_vote_signer::rpc::LocalVoteSigner;
use std::fs::remove_dir_all;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::sleep;
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
    let (mint, leader_ledger_path) =
        create_tmp_genesis(leader_ledger_path, 1_000_000_000, leader_info.id, 1);

    let validator_ledger_path =
        tmp_copy_ledger(&leader_ledger_path, "replicator_test_validator_ledger");

    {
        let signer_proxy =
            VoteSignerProxy::new(&leader_keypair, Box::new(LocalVoteSigner::default()));

        let leader = Fullnode::new_with_storage_rotate(
            leader_node,
            &leader_ledger_path,
            leader_keypair,
            Some(Arc::new(signer_proxy)),
            None,
            false,
            LeaderScheduler::from_bootstrap_leader(leader_info.id.clone()),
            None,
            STORAGE_ROTATE_TEST_COUNT,
        );

        let validator_keypair = Arc::new(Keypair::new());
        let signer_proxy =
            VoteSignerProxy::new(&validator_keypair, Box::new(LocalVoteSigner::default()));

        let mut leader_client = mk_client(&leader_info);

        let last_id = leader_client.get_last_id();
        let mut leader_client = mk_client(&leader_info);

        leader_client
            .transfer(10, &mint.keypair(), validator_keypair.pubkey(), &last_id)
            .unwrap();

        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
        #[cfg(feature = "chacha")]
        let validator_node_info = validator_node.info.clone();

        let validator = Fullnode::new_with_storage_rotate(
            validator_node,
            &validator_ledger_path,
            validator_keypair,
            Some(Arc::new(signer_proxy)),
            Some(leader_info.gossip),
            false,
            LeaderScheduler::from_bootstrap_leader(leader_info.id),
            None,
            STORAGE_ROTATE_TEST_COUNT,
        );

        let bob = Keypair::new();

        info!("starting transfers..");

        for _ in 0..64 {
            let last_id = leader_client.get_last_id();
            leader_client
                .transfer(1, &mint.keypair(), bob.pubkey(), &last_id)
                .unwrap();
            sleep(Duration::from_millis(200));
        }

        let replicator_keypair = Keypair::new();

        info!("giving replicator tokens..");

        let last_id = leader_client.get_last_id();
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
            None,
        )
        .unwrap();

        info!("started replicator..");

        // Create a client which downloads from the replicator and see that it
        // can respond with blobs.
        let tn = Node::new_localhost();
        let cluster_info = ClusterInfo::new(tn.info.clone());
        let repair_index = replicator.entry_height();
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

        let mut received_blob = false;
        for _ in 0..5 {
            repair_socket.send_to(&req, replicator_info.gossip).unwrap();

            let x = r_reader.recv_timeout(Duration::new(1, 0));

            if let Ok(blobs) = x {
                for b in blobs {
                    let br = b.read().unwrap();
                    assert!(br.index().unwrap() == repair_index);
                    let entry: Entry = deserialize(&br.data()[..br.meta.size]).unwrap();
                    info!("entry: {:?}", entry);
                    assert_ne!(entry.id, Hash::default());
                    received_blob = true;
                }
                break;
            }
        }
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().unwrap();

        assert!(received_blob);

        // The replicator will not submit storage proofs if
        // chacha is not enabled
        #[cfg(feature = "chacha")]
        {
            use solana::rpc_request::{RpcClient, RpcRequest, RpcRequestHandler};
            use std::thread::sleep;

            info!(
                "looking for pubkeys for entry: {}",
                replicator.entry_height()
            );
            let rpc_client = RpcClient::new_from_socket(validator_node_info.rpc);
            let mut non_zero_pubkeys = false;
            for _ in 0..60 {
                let params = json!([replicator.entry_height()]);
                let pubkeys = rpc_client
                    .make_rpc_request(1, RpcRequest::GetStoragePubkeysForEntryHeight, Some(params))
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

        replicator.close();
        validator.exit();
        leader.close().expect("Expected successful node closure");
    }

    DbLedger::destroy(&leader_ledger_path).expect("Expected successful database destruction");
    DbLedger::destroy(&replicator_ledger_path).expect("Expected successful database destruction");
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}

#[test]
fn test_replicator_startup_leader_hang() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    solana_logger::setup();
    info!("starting replicator test");

    let replicator_ledger_path = &get_tmp_ledger_path("replicator_test_replicator_ledger");
    let leader_ledger_path = "replicator_test_leader_ledger";

    {
        let replicator_keypair = Keypair::new();

        info!("starting replicator node");
        let replicator_node = Node::new_localhost_with_pubkey(replicator_keypair.pubkey());

        let fake_gossip = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let leader_info = NodeInfo::new_entry_point(&fake_gossip);

        let replicator_res = Replicator::new(
            Some(replicator_ledger_path),
            replicator_node,
            &leader_info,
            &replicator_keypair,
            Some(Duration::from_secs(3)),
        );

        assert!(replicator_res.is_err());
    }

    let _ignored = DbLedger::destroy(&leader_ledger_path);
    let _ignored = DbLedger::destroy(&replicator_ledger_path);
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}

#[test]
fn test_replicator_startup_ledger_hang() {
    use std::net::UdpSocket;

    solana_logger::setup();
    info!("starting replicator test");
    let replicator_ledger_path = &get_tmp_ledger_path("replicator_test_replicator_ledger");

    info!("starting leader node");
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    let leader_ledger_path = "replicator_test_leader_ledger";
    let (_, leader_ledger_path) = create_tmp_genesis(leader_ledger_path, 100, leader_info.id, 1);

    let validator_ledger_path =
        tmp_copy_ledger(&leader_ledger_path, "replicator_test_validator_ledger");

    {
        let signer_proxy =
            VoteSignerProxy::new(&leader_keypair, Box::new(LocalVoteSigner::default()));

        let _ = Fullnode::new(
            leader_node,
            &leader_ledger_path,
            leader_keypair,
            Some(Arc::new(signer_proxy)),
            None,
            false,
            LeaderScheduler::from_bootstrap_leader(leader_info.id.clone()),
            None,
        );

        let validator_keypair = Arc::new(Keypair::new());
        let signer_proxy =
            VoteSignerProxy::new(&validator_keypair, Box::new(LocalVoteSigner::default()));
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());

        let _ = Fullnode::new(
            validator_node,
            &validator_ledger_path,
            validator_keypair,
            Some(Arc::new(signer_proxy)),
            Some(leader_info.gossip),
            false,
            LeaderScheduler::from_bootstrap_leader(leader_info.id),
            None,
        );

        info!("starting replicator node");
        let bad_keys = Keypair::new();
        let mut replicator_node = Node::new_localhost_with_pubkey(bad_keys.pubkey());

        // Pass bad TVU sockets to prevent successful ledger download
        replicator_node.sockets.tvu = vec![UdpSocket::bind("0.0.0.0:0").unwrap()];

        let leader_info = NodeInfo::new_entry_point(&leader_info.gossip);

        let replicator_res = Replicator::new(
            Some(replicator_ledger_path),
            replicator_node,
            &leader_info,
            &bad_keys,
            Some(Duration::from_secs(3)),
        );

        assert!(replicator_res.is_err());
    }

    let _ignored = DbLedger::destroy(&leader_ledger_path);
    let _ignored = DbLedger::destroy(&replicator_ledger_path);
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}
