#![allow(clippy::arithmetic_side_effects)]

use {
    serial_test::serial,
    solana_bench_tps::{
        bench::{do_bench_tps, generate_and_fund_keypairs},
        cli::{Config, InstructionPaddingConfig},
        send_batch::generate_durable_nonce_accounts,
    },
    solana_client::{
        thin_client::ThinClient,
        tpu_client::{TpuClient, TpuClientConfig},
    },
    solana_core::validator::ValidatorConfig,
    solana_faucet::faucet::run_local_faucet,
    solana_local_cluster::{
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::make_identical_validator_configs,
    },
    solana_rpc::rpc::JsonRpcConfig,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        account::{Account, AccountSharedData},
        commitment_config::CommitmentConfig,
        fee_calculator::FeeRateGovernor,
        rent::Rent,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidatorGenesis,
    std::{sync::Arc, time::Duration},
};

fn program_account(program_data: &[u8]) -> AccountSharedData {
    AccountSharedData::from(Account {
        lamports: Rent::default().minimum_balance(program_data.len()).min(1),
        data: program_data.to_vec(),
        owner: solana_sdk::bpf_loader::id(),
        executable: true,
        rent_epoch: 0,
    })
}

fn test_bench_tps_local_cluster(config: Config) {
    let native_instruction_processors = vec![];
    let additional_accounts = vec![(
        spl_instruction_padding::ID,
        program_account(include_bytes!("fixtures/spl_instruction_padding.so")),
    )];

    solana_logger::setup();

    let faucet_keypair = Keypair::new();
    let faucet_pubkey = faucet_keypair.pubkey();
    let faucet_addr = run_local_faucet(faucet_keypair, None);

    const NUM_NODES: usize = 1;
    let cluster = LocalCluster::new(
        &mut ClusterConfig {
            node_stakes: vec![999_990; NUM_NODES],
            cluster_lamports: 200_000_000,
            validator_configs: make_identical_validator_configs(
                &ValidatorConfig {
                    rpc_config: JsonRpcConfig {
                        faucet_addr: Some(faucet_addr),
                        ..JsonRpcConfig::default_for_test()
                    },
                    ..ValidatorConfig::default_for_test()
                },
                NUM_NODES,
            ),
            native_instruction_processors,
            additional_accounts,
            ..ClusterConfig::default()
        },
        SocketAddrSpace::Unspecified,
    );

    cluster.transfer(&cluster.funding_keypair, &faucet_pubkey, 100_000_000);

    let client = Arc::new(ThinClient::new(
        cluster.entry_point_info.rpc().unwrap(),
        cluster
            .entry_point_info
            .tpu(cluster.connection_cache.protocol())
            .unwrap(),
        cluster.connection_cache.clone(),
    ));

    let lamports_per_account = 100;

    let keypair_count = config.tx_count * config.keypair_multiplier;
    let keypairs = generate_and_fund_keypairs(
        client.clone(),
        &config.id,
        keypair_count,
        lamports_per_account,
        false,
    )
    .unwrap();

    let _total = do_bench_tps(client, config, keypairs, None);

    #[cfg(not(debug_assertions))]
    assert!(_total > 100);
}

fn test_bench_tps_test_validator(config: Config) {
    solana_logger::setup();

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();

    let faucet_addr = run_local_faucet(mint_keypair, None);

    let test_validator = TestValidatorGenesis::default()
        .fee_rate_governor(FeeRateGovernor::new(0, 0))
        .rent(Rent {
            lamports_per_byte_year: 1,
            exemption_threshold: 1.0,
            ..Rent::default()
        })
        .faucet_addr(Some(faucet_addr))
        .add_program("spl_instruction_padding", spl_instruction_padding::ID)
        .start_with_mint_address(mint_pubkey, SocketAddrSpace::Unspecified)
        .expect("validator start failed");

    let rpc_client = Arc::new(RpcClient::new_with_commitment(
        test_validator.rpc_url(),
        CommitmentConfig::processed(),
    ));
    let websocket_url = test_validator.rpc_pubsub_url();
    let client =
        Arc::new(TpuClient::new(rpc_client, &websocket_url, TpuClientConfig::default()).unwrap());

    let lamports_per_account = 1000;

    let keypair_count = config.tx_count * config.keypair_multiplier;
    let keypairs = generate_and_fund_keypairs(
        client.clone(),
        &config.id,
        keypair_count,
        lamports_per_account,
        false,
    )
    .unwrap();
    let nonce_keypairs = if config.use_durable_nonce {
        Some(generate_durable_nonce_accounts(client.clone(), &keypairs))
    } else {
        None
    };

    let _total = do_bench_tps(client, config, keypairs, nonce_keypairs);

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

#[test]
#[serial]
fn test_bench_tps_tpu_client() {
    test_bench_tps_test_validator(Config {
        tx_count: 100,
        duration: Duration::from_secs(10),
        ..Config::default()
    });
}

#[test]
#[serial]
fn test_bench_tps_tpu_client_nonce() {
    test_bench_tps_test_validator(Config {
        tx_count: 100,
        duration: Duration::from_secs(10),
        use_durable_nonce: true,
        ..Config::default()
    });
}

#[test]
#[serial]
fn test_bench_tps_local_cluster_with_padding() {
    test_bench_tps_local_cluster(Config {
        tx_count: 100,
        duration: Duration::from_secs(10),
        instruction_padding_config: Some(InstructionPaddingConfig {
            program_id: spl_instruction_padding::ID,
            data_size: 0,
        }),
        ..Config::default()
    });
}

#[test]
#[serial]
fn test_bench_tps_tpu_client_with_padding() {
    test_bench_tps_test_validator(Config {
        tx_count: 100,
        duration: Duration::from_secs(10),
        instruction_padding_config: Some(InstructionPaddingConfig {
            program_id: spl_instruction_padding::ID,
            data_size: 0,
        }),
        ..Config::default()
    });
}
