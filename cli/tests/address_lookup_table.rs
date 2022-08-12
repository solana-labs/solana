use {
    serde_json::Value,
    solana_cli::{
        address_lookup_table::AddressLookupTableCliCommand,
        cli::{process_command, CliCommand, CliConfig},
    },
    solana_cli_output::OutputFormat,
    solana_client::rpc_client::RpcClient,
    solana_faucet::faucet::run_local_faucet,
    solana_sdk::{
        commitment_config::CommitmentConfig,
        native_token::LAMPORTS_PER_SOL,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
    std::str::FromStr,
};

#[test]
fn test_cli_create_lookup_table() {
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 10 * LAMPORTS_PER_SOL,
    };
    process_command(&config).unwrap();

    config.command =
        CliCommand::AddressLookupTable(AddressLookupTableCliCommand::CreateLookupTable {
            authority_signer_index: 0,
            payer_signer_index: 0,
        });
    config.output_format = OutputFormat::JsonCompact;
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let lookup_table_address = json
        .as_object()
        .unwrap()
        .get("lookupTableAddress")
        .unwrap()
        .as_str()
        .unwrap();
    let lookup_table_key = Pubkey::from_str(lookup_table_address).unwrap();
    let lookup_table_account = rpc_client.get_account(&lookup_table_key).unwrap();
    assert_eq!(
        lookup_table_account.owner,
        solana_address_lookup_table_program::id()
    );
}
