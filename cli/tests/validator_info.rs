use {
    serde_json::json,
    solana_cli::{
        check_balance,
        cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig},
    },
    solana_faucet::faucet::run_local_faucet,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        commitment_config::CommitmentConfig,
        signature::{keypair_from_seed, Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
    test_case::test_case,
};

#[test_case(None; "base")]
#[test_case(Some(1_000_000); "with_compute_unit_price")]
fn test_publish(compute_unit_price: Option<u64>) {
    solana_logger::setup();

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let validator_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let mut config_validator = CliConfig::recent_for_tests();
    config_validator.json_rpc_url = test_validator.rpc_url();
    config_validator.signers = vec![&validator_keypair];

    request_and_confirm_airdrop(
        &rpc_client,
        &config_validator,
        &config_validator.signers[0].pubkey(),
        100_000_000_000,
    )
    .unwrap();
    check_balance!(
        100_000_000_000,
        &rpc_client,
        &config_validator.signers[0].pubkey()
    );

    config_validator.command = CliCommand::SetValidatorInfo {
        validator_info: json!({ "name": "test" }),
        force_keybase: true,
        info_pubkey: None,
        compute_unit_price,
    };
    process_command(&config_validator).unwrap();
}
