use log::*;
use serial_test_derive::serial;
use solana_client::thin_client::create_client;
use solana_core::{
    archiver::Archiver,
    cluster_info::{ClusterInfo, Node, VALIDATOR_PORT_RANGE},
    contact_info::ContactInfo,
    gossip_service::discover_cluster,
    storage_stage::SLOTS_PER_TURN_TEST,
    validator::ValidatorConfig,
};
use solana_ledger::{blocktree::Blocktree, create_new_tmp_ledger, get_tmp_ledger_path};
use solana_local_cluster::local_cluster::{ClusterConfig, LocalCluster};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    genesis_config::create_genesis_config,
    signature::{Keypair, KeypairUtil},
};
use std::{
    fs::remove_dir_all,
    sync::{Arc, RwLock},
};

/// Start the cluster with the given configuration and wait till the archivers are discovered
/// Then download blobs from one of them.
fn run_archiver_startup_basic(num_nodes: usize, num_archivers: usize) {
    solana_logger::setup();
    info!("starting archiver test");

    let mut validator_config = ValidatorConfig::default();
    let slots_per_segment = 8;
    validator_config.storage_slots_per_turn = SLOTS_PER_TURN_TEST;
    let config = ClusterConfig {
        validator_configs: vec![validator_config; num_nodes],
        num_archivers,
        node_stakes: vec![100; num_nodes],
        cluster_lamports: 10_000,
        // keep a low slot/segment count to speed up the test
        slots_per_segment,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);

    let (cluster_nodes, cluster_archivers) =
        discover_cluster(&cluster.entry_point_info.gossip, num_nodes + num_archivers).unwrap();
    assert_eq!(
        cluster_nodes.len() + cluster_archivers.len(),
        num_nodes + num_archivers
    );
    let mut archiver_count = 0;
    let mut archiver_info = ContactInfo::default();
    for node in &cluster_archivers {
        info!("storage: {:?} rpc: {:?}", node.storage_addr, node.rpc);
        if ContactInfo::is_valid_address(&node.storage_addr) {
            archiver_count += 1;
            archiver_info = node.clone();
        }
    }
    assert_eq!(archiver_count, num_archivers);

    let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
        cluster_nodes[0].clone(),
    )));
    let path = get_tmp_ledger_path!();
    let blocktree = Arc::new(Blocktree::open(&path).unwrap());
    Archiver::download_from_archiver(&cluster_info, &archiver_info, &blocktree, slots_per_segment)
        .unwrap();
}

#[test]
#[serial]
fn test_archiver_startup_1_node() {
    run_archiver_startup_basic(1, 1);
}

#[test]
#[serial]
fn test_archiver_startup_2_nodes() {
    run_archiver_startup_basic(2, 1);
}

#[test]
#[serial]
fn test_archiver_startup_leader_hang() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    solana_logger::setup();
    info!("starting archiver test");

    let leader_ledger_path = std::path::PathBuf::from("archiver_test_leader_ledger");
    let (genesis_config, _mint_keypair) = create_genesis_config(10_000);
    let (archiver_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

    {
        let archiver_keypair = Arc::new(Keypair::new());
        let storage_keypair = Arc::new(Keypair::new());

        info!("starting archiver node");
        let archiver_node = Node::new_localhost_with_pubkey(&archiver_keypair.pubkey());

        let fake_gossip = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let leader_info = ContactInfo::new_gossip_entry_point(&fake_gossip);

        let archiver_res = Archiver::new(
            &archiver_ledger_path,
            archiver_node,
            leader_info,
            archiver_keypair,
            storage_keypair,
            CommitmentConfig::recent(),
        );

        assert!(archiver_res.is_err());
    }

    let _ignored = Blocktree::destroy(&leader_ledger_path);
    let _ignored = Blocktree::destroy(&archiver_ledger_path);
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&archiver_ledger_path);
}

#[test]
#[serial]
fn test_archiver_startup_ledger_hang() {
    solana_logger::setup();
    info!("starting archiver test");
    let mut validator_config = ValidatorConfig::default();
    validator_config.storage_slots_per_turn = SLOTS_PER_TURN_TEST;
    let cluster = LocalCluster::new_with_equal_stakes(2, 10_000, 100);

    info!("starting archiver node");
    let bad_keys = Arc::new(Keypair::new());
    let storage_keypair = Arc::new(Keypair::new());
    let mut archiver_node = Node::new_localhost_with_pubkey(&bad_keys.pubkey());

    // Pass bad TVU sockets to prevent successful ledger download
    archiver_node.sockets.tvu = vec![std::net::UdpSocket::bind("0.0.0.0:0").unwrap()];
    let (archiver_ledger_path, _blockhash) = create_new_tmp_ledger!(&cluster.genesis_config);

    let archiver_res = Archiver::new(
        &archiver_ledger_path,
        archiver_node,
        cluster.entry_point_info.clone(),
        bad_keys,
        storage_keypair,
        CommitmentConfig::recent(),
    );

    assert!(archiver_res.is_err());
}

#[test]
#[serial]
fn test_account_setup() {
    let num_nodes = 1;
    let num_archivers = 1;
    let mut validator_config = ValidatorConfig::default();
    validator_config.storage_slots_per_turn = SLOTS_PER_TURN_TEST;
    let config = ClusterConfig {
        validator_configs: vec![ValidatorConfig::default(); num_nodes],
        num_archivers,
        node_stakes: vec![100; num_nodes],
        cluster_lamports: 10_000,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);

    let _ = discover_cluster(
        &cluster.entry_point_info.gossip,
        num_nodes + num_archivers as usize,
    )
    .unwrap();
    // now check that the cluster actually has accounts for the archiver.
    let client = create_client(
        cluster.entry_point_info.client_facing_addr(),
        VALIDATOR_PORT_RANGE,
    );
    cluster.archiver_infos.iter().for_each(|(_, value)| {
        assert_eq!(
            client
                .poll_get_balance_with_commitment(
                    &value.archiver_storage_pubkey,
                    CommitmentConfig::recent()
                )
                .unwrap(),
            1
        );
    });
}
