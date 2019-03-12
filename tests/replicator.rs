#[macro_use]
extern crate log;

#[macro_use]
extern crate solana;

use solana::blocktree::{
    create_new_tmp_ledger, get_tmp_ledger_path, tmp_copy_blocktree, Blocktree,
};
use solana::cluster_info::{Node, FULLNODE_PORT_RANGE};
use solana::contact_info::ContactInfo;
use solana::fullnode::{Fullnode, FullnodeConfig};
use solana::replicator::Replicator;
use solana::storage_stage::STORAGE_ROTATE_TEST_COUNT;
use solana_client::client::create_client;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use std::fs::remove_dir_all;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_replicator_startup_basic() {
    solana_logger::setup();
    info!("starting replicator test");
    let replicator_ledger_path = &get_tmp_ledger_path!();

    info!("starting leader node");
    let leader_keypair = Arc::new(Keypair::new());
    let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    let (genesis_block, mint_keypair) =
        GenesisBlock::new_with_leader(1_000_000_000, &leader_info.id, 42);
    let (leader_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

    let validator_ledger_path = tmp_copy_blocktree!(&leader_ledger_path);

    {
        let voting_keypair = Keypair::new();

        let mut fullnode_config = FullnodeConfig::default();
        fullnode_config.storage_rotate_count = STORAGE_ROTATE_TEST_COUNT;
        let leader = Fullnode::new(
            leader_node,
            &leader_keypair,
            &leader_ledger_path,
            &voting_keypair.pubkey(),
            voting_keypair,
            None,
            &fullnode_config,
        );

        debug!("Looking for leader on gossip...");
        solana::gossip_service::discover(&leader_info.gossip, 1).unwrap();

        let validator_keypair = Arc::new(Keypair::new());
        let voting_keypair = Keypair::new();

        let mut leader_client =
            create_client(leader_info.client_facing_addr(), FULLNODE_PORT_RANGE);
        let blockhash = leader_client.get_recent_blockhash();
        debug!("blockhash: {:?}", blockhash);

        leader_client
            .transfer(10, &mint_keypair, &validator_keypair.pubkey(), &blockhash)
            .unwrap();

        let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());

        let validator = Fullnode::new(
            validator_node,
            &validator_keypair,
            &validator_ledger_path,
            &voting_keypair.pubkey(),
            voting_keypair,
            Some(&leader_info),
            &fullnode_config,
        );

        let bob = Keypair::new();

        info!("starting transfers..");
        for i in 0..64 {
            debug!("transfer {}", i);
            let blockhash = leader_client.get_recent_blockhash();
            let mut transaction =
                SystemTransaction::new_account(&mint_keypair, &bob.pubkey(), 1, blockhash, 0);
            leader_client
                .retry_transfer(&mint_keypair, &mut transaction, 5)
                .unwrap();
            debug!(
                "transfer {}: mint balance={:?}, bob balance={:?}",
                i,
                leader_client.get_balance(&mint_keypair.pubkey()),
                leader_client.get_balance(&bob.pubkey()),
            );
        }

        let replicator_keypair = Arc::new(Keypair::new());

        info!("giving replicator lamports..");

        let blockhash = leader_client.get_recent_blockhash();
        // Give the replicator some lamports
        let mut tx = SystemTransaction::new_account(
            &mint_keypair,
            &replicator_keypair.pubkey(),
            1,
            blockhash,
            0,
        );
        leader_client
            .retry_transfer(&mint_keypair, &mut tx, 5)
            .unwrap();

        info!("starting replicator node");
        let replicator_node = Node::new_localhost_with_pubkey(&replicator_keypair.pubkey());
        let _replicator_info = replicator_node.info.clone();

        let leader_info = ContactInfo::new_gossip_entry_point(&leader_info.gossip);

        let replicator = Replicator::new(
            replicator_ledger_path,
            replicator_node,
            leader_info,
            replicator_keypair,
            None,
        )
        .unwrap();

        info!("started replicator..");

        replicator.close();
        validator.close().unwrap();
        leader.close().unwrap();
    }

    info!("cleanup");
    Blocktree::destroy(&leader_ledger_path).expect("Expected successful database destruction");
    Blocktree::destroy(&replicator_ledger_path).expect("Expected successful database destruction");
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}

#[test]
fn test_replicator_startup_leader_hang() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    solana_logger::setup();
    info!("starting replicator test");

    let leader_ledger_path = "replicator_test_leader_ledger";
    let (genesis_block, _mint_keypair) = GenesisBlock::new(10_000);
    let (replicator_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

    {
        let replicator_keypair = Arc::new(Keypair::new());

        info!("starting replicator node");
        let replicator_node = Node::new_localhost_with_pubkey(&replicator_keypair.pubkey());

        let fake_gossip = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let leader_info = ContactInfo::new_gossip_entry_point(&fake_gossip);

        let replicator_res = Replicator::new(
            &replicator_ledger_path,
            replicator_node,
            leader_info,
            replicator_keypair,
            Some(Duration::from_secs(3)),
        );

        assert!(replicator_res.is_err());
    }

    let _ignored = Blocktree::destroy(&leader_ledger_path);
    let _ignored = Blocktree::destroy(&replicator_ledger_path);
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}

#[test]
#[ignore] //TODO: hangs, was passing because of bug in network code
fn test_replicator_startup_ledger_hang() {
    solana_logger::setup();
    info!("starting replicator test");
    let leader_keypair = Arc::new(Keypair::new());

    let (genesis_block, _mint_keypair) =
        GenesisBlock::new_with_leader(100, &leader_keypair.pubkey(), 42);
    let (replicator_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

    info!("starting leader node");
    let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());
    let leader_info = leader_node.info.clone();

    let (leader_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);
    let validator_ledger_path = tmp_copy_blocktree!(&leader_ledger_path);

    {
        let voting_keypair = Keypair::new();

        let fullnode_config = FullnodeConfig::default();
        let _ = Fullnode::new(
            leader_node,
            &leader_keypair,
            &leader_ledger_path,
            &voting_keypair.pubkey(),
            voting_keypair,
            None,
            &fullnode_config,
        );

        let validator_keypair = Arc::new(Keypair::new());
        let voting_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());

        let _ = Fullnode::new(
            validator_node,
            &validator_keypair,
            &validator_ledger_path,
            &voting_keypair.pubkey(),
            voting_keypair,
            Some(&leader_info),
            &FullnodeConfig::default(),
        );

        info!("starting replicator node");
        let bad_keys = Arc::new(Keypair::new());
        let mut replicator_node = Node::new_localhost_with_pubkey(&bad_keys.pubkey());

        // Pass bad TVU sockets to prevent successful ledger download
        replicator_node.sockets.tvu = vec![std::net::UdpSocket::bind("0.0.0.0:0").unwrap()];

        let leader_info = ContactInfo::new_gossip_entry_point(&leader_info.gossip);

        let replicator_res = Replicator::new(
            &replicator_ledger_path,
            replicator_node,
            leader_info,
            bad_keys,
            Some(Duration::from_secs(3)),
        );

        assert!(replicator_res.is_err());
    }

    let _ignored = Blocktree::destroy(&leader_ledger_path);
    let _ignored = Blocktree::destroy(&replicator_ledger_path);
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}
