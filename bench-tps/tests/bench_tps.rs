#![allow(clippy::integer_arithmetic)]
use {
    crossbeam_channel::unbounded,
    serial_test::serial,
    solana_bench_tps::{
        bench::{do_bench_tps, generate_and_fund_keypairs},
        cli::Config,
    },
    solana_client::thin_client::create_client,
    solana_core::validator::ValidatorConfig,
    solana_faucet::faucet::run_local_faucet_with_port,
    solana_gossip::cluster_info::VALIDATOR_PORT_RANGE,
    solana_local_cluster::{
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::make_identical_validator_configs,
    },
    solana_sdk::signature::{Keypair, Signer},
    solana_streamer::socket::SocketAddrSpace,
    std::{sync::Arc, time::Duration},
};

fn test_bench_tps_local_cluster(config: Config) {
    let native_instruction_processors = vec![];

    solana_logger::setup();
    const NUM_NODES: usize = 1;
    let cluster = LocalCluster::new(
        &mut ClusterConfig {
            node_stakes: vec![999_990; NUM_NODES],
            cluster_lamports: 200_000_000,
            validator_configs: make_identical_validator_configs(
                &ValidatorConfig::default_for_test(),
                NUM_NODES,
            ),
            native_instruction_processors,
            ..ClusterConfig::default()
        },
        SocketAddrSpace::Unspecified,
    );

    let faucet_keypair = Keypair::new();
    cluster.transfer(
        &cluster.funding_keypair,
        &faucet_keypair.pubkey(),
        100_000_000,
    );

    let client = Arc::new(create_client(
        (cluster.entry_point_info.rpc, cluster.entry_point_info.tpu),
        VALIDATOR_PORT_RANGE,
    ));

    let (addr_sender, addr_receiver) = unbounded();
    run_local_faucet_with_port(faucet_keypair, addr_sender, None, 0);
    let faucet_addr = addr_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("run_local_faucet")
        .expect("faucet_addr");

    let lamports_per_account = 100;

    let keypair_count = config.tx_count * config.keypair_multiplier;
    let keypairs = generate_and_fund_keypairs(
        client.clone(),
        Some(faucet_addr),
        &config.id,
        keypair_count,
        lamports_per_account,
    )
    .unwrap();

    let _total = do_bench_tps(client, config, keypairs);

    #[cfg(not(debug_assertions))]
    assert!(_total > 100);
}

#[test]
#[serial]
fn test_bench_tps_local_cluster_solana() {
    test_bench_tps_local_cluster(Config {
        tx_count: 100,
        duration: Duration::from_secs(10),
        ..Config::default()
    });
}
