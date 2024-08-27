use {
    solana_cli::{
        check_balance,
        cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig},
        test_utils::check_ready,
    },
    solana_faucet::faucet::run_local_faucet,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        commitment_config::CommitmentConfig,
        fee::FeeStructure,
        native_token::sol_to_lamports,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
    std::time::Duration,
    test_case::test_case,
};

#[test_case(None; "base")]
#[test_case(Some(1_000_000); "with_compute_unit_price")]
fn test_ping(compute_unit_price: Option<u64>) {
    solana_logger::setup();
    let fee = FeeStructure::default().get_max_fee(1, 0);
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator = TestValidator::with_custom_fees(
        mint_pubkey,
        fee,
        Some(faucet_addr),
        SocketAddrSpace::Unspecified,
    );

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let default_signer = Keypair::new();
    let signer_pubkey = default_signer.pubkey();

    let mut config = CliConfig::recent_for_tests();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&default_signer];

    request_and_confirm_airdrop(&rpc_client, &config, &signer_pubkey, sol_to_lamports(1.0))
        .unwrap();
    check_balance!(sol_to_lamports(1.0), &rpc_client, &signer_pubkey);
    check_ready(&rpc_client);

    let count = 5;
    config.command = CliCommand::Ping {
        interval: Duration::from_secs(0),
        count: Some(count),
        timeout: Duration::from_secs(15),
        blockhash: None,
        print_timestamp: false,
        compute_unit_price,
    };
    process_command(&config).unwrap();
}
