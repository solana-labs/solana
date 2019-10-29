use crate::local_cluster::{ClusterConfig, LocalCluster};
use serial_test_derive::serial;
use solana_bench_tps::bench::{do_bench_tps, generate_and_fund_keypairs};
use solana_bench_tps::cli::Config;
use solana_client::thin_client::create_client;
use solana_core::cluster_info::VALIDATOR_PORT_RANGE;
use solana_core::validator::ValidatorConfig;
use solana_drone::drone::run_local_drone;
#[cfg(feature = "move")]
use solana_move_loader_program;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::sync::mpsc::channel;
use std::time::Duration;

fn test_bench_tps_local_cluster(config: Config) {
    #[cfg(feature = "move")]
    let native_instruction_processors = vec![solana_move_loader_program!()];

    #[cfg(not(feature = "move"))]
    let native_instruction_processors = vec![];

    solana_logger::setup();
    const NUM_NODES: usize = 1;
    let cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![999_990; NUM_NODES],
        cluster_lamports: 200_000_000,
        validator_configs: vec![ValidatorConfig::default(); NUM_NODES],
        native_instruction_processors,
        ..ClusterConfig::default()
    });

    let drone_keypair = Keypair::new();
    cluster.transfer(
        &cluster.funding_keypair,
        &drone_keypair.pubkey(),
        100_000_000,
    );

    let client = create_client(
        (cluster.entry_point_info.rpc, cluster.entry_point_info.tpu),
        VALIDATOR_PORT_RANGE,
    );

    let (addr_sender, addr_receiver) = channel();
    run_local_drone(drone_keypair, addr_sender, None);
    let drone_addr = addr_receiver.recv_timeout(Duration::from_secs(2)).unwrap();

    let lamports_per_account = 100;

    let (keypairs, move_keypairs, _keypair_balance) = generate_and_fund_keypairs(
        &client,
        Some(drone_addr),
        &config.id,
        config.tx_count,
        lamports_per_account,
        config.use_move,
    )
    .unwrap();

    let total = do_bench_tps(vec![client], config, keypairs, 0, move_keypairs);
    assert!(total > 100);
}

#[test]
#[serial]
fn test_bench_tps_local_cluster_solana() {
    let mut config = Config::default();
    config.tx_count = 100;
    config.duration = Duration::from_secs(10);

    test_bench_tps_local_cluster(config);
}

#[test]
#[serial]
fn test_bench_tps_local_cluster_move() {
    let mut config = Config::default();
    config.tx_count = 100;
    config.duration = Duration::from_secs(25);
    config.use_move = true;

    test_bench_tps_local_cluster(config);
}
