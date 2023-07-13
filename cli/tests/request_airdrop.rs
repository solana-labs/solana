#![allow(clippy::arithmetic_side_effects)]
use {
    solana_cli::cli::{process_command, CliCommand, CliConfig},
    solana_faucet::faucet::run_local_faucet,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        commitment_config::CommitmentConfig,
        native_token::sol_to_lamports,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
};

#[test]
fn test_cli_request_airdrop() {
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let mut bob_config = CliConfig::recent_for_tests();
    bob_config.json_rpc_url = test_validator.rpc_url();
    bob_config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: sol_to_lamports(50.0),
    };
    let keypair = Keypair::new();
    bob_config.signers = vec![&keypair];

    let sig_response = process_command(&bob_config);
    sig_response.unwrap();

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let balance = rpc_client
        .get_balance(&bob_config.signers[0].pubkey())
        .unwrap();
    assert_eq!(balance, sol_to_lamports(50.0));
}
