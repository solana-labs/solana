use log::*;
use solana_bench_exchange::bench::{airdrop_lamports, do_bench_exchange, Config};
use solana_core::gossip_service::{discover_cluster, get_multi_client};
use solana_core::validator::ValidatorConfig;
use solana_exchange_program::exchange_processor::process_instruction;
use solana_exchange_program::id;
use solana_exchange_program::solana_exchange_program;
use solana_faucet::faucet::run_local_faucet;
use solana_local_cluster::local_cluster::{ClusterConfig, LocalCluster};
use solana_runtime::bank::Bank;
use solana_runtime::bank_client::BankClient;
use solana_sdk::genesis_config::create_genesis_config;
use solana_sdk::signature::{Keypair, Signer};
use std::process::exit;
use std::sync::mpsc::channel;
use std::time::Duration;

#[test]
#[ignore]
fn test_exchange_local_cluster() {
    solana_logger::setup();

    const NUM_NODES: usize = 1;

    let mut config = Config::default();
    config.identity = Keypair::new();
    config.duration = Duration::from_secs(1);
    config.fund_amount = 100_000;
    config.threads = 1;
    config.transfer_delay = 20; // 15
    config.batch_size = 100; // 1000;
    config.chunk_size = 10; // 200;
    config.account_groups = 1; // 10;
    let Config {
        fund_amount,
        batch_size,
        account_groups,
        ..
    } = config;
    let accounts_in_groups = batch_size * account_groups;

    let cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![100_000; NUM_NODES],
        cluster_lamports: 100_000_000_000_000,
        validator_configs: vec![ValidatorConfig::default(); NUM_NODES],
        native_instruction_processors: [solana_exchange_program!()].to_vec(),
        ..ClusterConfig::default()
    });

    let faucet_keypair = Keypair::new();
    cluster.transfer(
        &cluster.funding_keypair,
        &faucet_keypair.pubkey(),
        2_000_000_000_000,
    );

    let (addr_sender, addr_receiver) = channel();
    run_local_faucet(faucet_keypair, addr_sender, Some(1_000_000_000_000));
    let faucet_addr = addr_receiver.recv_timeout(Duration::from_secs(2)).unwrap();

    info!("Connecting to the cluster");
    let (nodes, _) =
        discover_cluster(&cluster.entry_point_info.gossip, NUM_NODES).unwrap_or_else(|err| {
            error!("Failed to discover {} nodes: {:?}", NUM_NODES, err);
            exit(1);
        });

    let (client, num_clients) = get_multi_client(&nodes);

    info!("clients: {}", num_clients);
    assert!(num_clients >= NUM_NODES);

    const NUM_SIGNERS: u64 = 2;
    airdrop_lamports(
        &client,
        &faucet_addr,
        &config.identity,
        fund_amount * (accounts_in_groups + 1) as u64 * NUM_SIGNERS,
    );

    do_bench_exchange(vec![client], config);
}

#[test]
fn test_exchange_bank_client() {
    solana_logger::setup();
    let (genesis_config, identity) = create_genesis_config(100_000_000_000_000);
    let mut bank = Bank::new(&genesis_config);
    bank.add_static_program("exchange_program", id(), process_instruction);
    let clients = vec![BankClient::new(bank)];

    let mut config = Config::default();
    config.identity = identity;
    config.duration = Duration::from_secs(1);
    config.fund_amount = 100_000;
    config.threads = 1;
    config.transfer_delay = 20; // 0;
    config.batch_size = 100; // 1500;
    config.chunk_size = 10; // 1500;
    config.account_groups = 1; // 50;

    do_bench_exchange(clients, config);
}
