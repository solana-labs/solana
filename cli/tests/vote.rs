#![allow(clippy::arithmetic_side_effects)]
use {
    solana_cli::{
        check_balance,
        cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig},
        spend_utils::SpendAmount,
    },
    solana_cli_output::{parse_sign_only_reply_string, OutputFormat},
    solana_faucet::faucet::run_local_faucet,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_nonce_utils::blockhash_query::{self, BlockhashQuery},
    solana_sdk::{
        account_utils::StateMut,
        commitment_config::CommitmentConfig,
        signature::{Keypair, NullSigner, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
    solana_vote_program::vote_state::{VoteAuthorize, VoteState, VoteStateVersions},
};

#[test]
fn test_vote_authorize_and_withdraw() {
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());
    let default_signer = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&default_signer];

    request_and_confirm_airdrop(&rpc_client, &config, &config.signers[0].pubkey(), 100_000)
        .unwrap();

    // Create vote account
    let vote_account_keypair = Keypair::new();
    let vote_account_pubkey = vote_account_keypair.pubkey();
    config.signers = vec![&default_signer, &vote_account_keypair];
    config.command = CliCommand::CreateVoteAccount {
        vote_account: 1,
        seed: None,
        identity_account: 0,
        authorized_voter: None,
        authorized_withdrawer: config.signers[0].pubkey(),
        commission: 0,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    let vote_account = rpc_client
        .get_account(&vote_account_keypair.pubkey())
        .unwrap();
    let vote_state: VoteStateVersions = vote_account.state().unwrap();
    let authorized_withdrawer = vote_state.convert_to_current().authorized_withdrawer;
    assert_eq!(authorized_withdrawer, config.signers[0].pubkey());
    let expected_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(VoteState::size_of())
        .unwrap()
        .max(1);
    check_balance!(expected_balance, &rpc_client, &vote_account_pubkey);

    // Transfer in some more SOL
    config.signers = vec![&default_signer];
    config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(10_000),
        to: vote_account_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    let expected_balance = expected_balance + 10_000;
    check_balance!(expected_balance, &rpc_client, &vote_account_pubkey);

    // Authorize vote account withdrawal to another signer
    let first_withdraw_authority = Keypair::new();
    config.signers = vec![&default_signer];
    config.command = CliCommand::VoteAuthorize {
        vote_account_pubkey,
        new_authorized_pubkey: first_withdraw_authority.pubkey(),
        vote_authorize: VoteAuthorize::Withdrawer,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        authorized: 0,
        new_authorized: None,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    let vote_account = rpc_client
        .get_account(&vote_account_keypair.pubkey())
        .unwrap();
    let vote_state: VoteStateVersions = vote_account.state().unwrap();
    let authorized_withdrawer = vote_state.convert_to_current().authorized_withdrawer;
    assert_eq!(authorized_withdrawer, first_withdraw_authority.pubkey());

    // Authorize vote account withdrawal to another signer with checked instruction
    let withdraw_authority = Keypair::new();
    config.signers = vec![&default_signer, &first_withdraw_authority];
    config.command = CliCommand::VoteAuthorize {
        vote_account_pubkey,
        new_authorized_pubkey: withdraw_authority.pubkey(),
        vote_authorize: VoteAuthorize::Withdrawer,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        authorized: 1,
        new_authorized: Some(1),
        compute_unit_price: None,
    };
    process_command(&config).unwrap_err(); // unsigned by new authority should fail
    config.signers = vec![
        &default_signer,
        &first_withdraw_authority,
        &withdraw_authority,
    ];
    config.command = CliCommand::VoteAuthorize {
        vote_account_pubkey,
        new_authorized_pubkey: withdraw_authority.pubkey(),
        vote_authorize: VoteAuthorize::Withdrawer,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        authorized: 1,
        new_authorized: Some(2),
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    let vote_account = rpc_client
        .get_account(&vote_account_keypair.pubkey())
        .unwrap();
    let vote_state: VoteStateVersions = vote_account.state().unwrap();
    let authorized_withdrawer = vote_state.convert_to_current().authorized_withdrawer;
    assert_eq!(authorized_withdrawer, withdraw_authority.pubkey());

    // Withdraw from vote account
    let destination_account = solana_sdk::pubkey::new_rand(); // Send withdrawal to new account to make balance check easy
    config.signers = vec![&default_signer, &withdraw_authority];
    config.command = CliCommand::WithdrawFromVoteAccount {
        vote_account_pubkey,
        withdraw_authority: 1,
        withdraw_amount: SpendAmount::Some(1_000),
        destination_account_pubkey: destination_account,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    let expected_balance = expected_balance - 1_000;
    check_balance!(expected_balance, &rpc_client, &vote_account_pubkey);
    check_balance!(1_000, &rpc_client, &destination_account);

    // Re-assign validator identity
    let new_identity_keypair = Keypair::new();
    config.signers.push(&new_identity_keypair);
    config.command = CliCommand::VoteUpdateValidator {
        vote_account_pubkey,
        new_identity_account: 2,
        withdraw_authority: 1,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();

    // Close vote account
    let destination_account = solana_sdk::pubkey::new_rand(); // Send withdrawal to new account to make balance check easy
    config.signers = vec![&default_signer, &withdraw_authority];
    config.command = CliCommand::CloseVoteAccount {
        vote_account_pubkey,
        withdraw_authority: 1,
        destination_account_pubkey: destination_account,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    check_balance!(0, &rpc_client, &vote_account_pubkey);
    check_balance!(expected_balance, &rpc_client, &destination_account);
}

#[test]
fn test_offline_vote_authorize_and_withdraw() {
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());
    let default_signer = Keypair::new();

    let mut config_payer = CliConfig::recent_for_tests();
    config_payer.json_rpc_url = test_validator.rpc_url();
    config_payer.signers = vec![&default_signer];

    let mut config_offline = CliConfig::recent_for_tests();
    config_offline.json_rpc_url = String::default();
    config_offline.command = CliCommand::ClusterVersion;
    let offline_keypair = Keypair::new();
    config_offline.signers = vec![&offline_keypair];
    // Verify that we cannot reach the cluster
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(
        &rpc_client,
        &config_payer,
        &config_payer.signers[0].pubkey(),
        100_000,
    )
    .unwrap();
    check_balance!(100_000, &rpc_client, &config_payer.signers[0].pubkey());

    request_and_confirm_airdrop(
        &rpc_client,
        &config_offline,
        &config_offline.signers[0].pubkey(),
        100_000,
    )
    .unwrap();
    check_balance!(100_000, &rpc_client, &config_offline.signers[0].pubkey());

    // Create vote account with specific withdrawer
    let vote_account_keypair = Keypair::new();
    let vote_account_pubkey = vote_account_keypair.pubkey();
    config_payer.signers = vec![&default_signer, &vote_account_keypair];
    config_payer.command = CliCommand::CreateVoteAccount {
        vote_account: 1,
        seed: None,
        identity_account: 0,
        authorized_voter: None,
        authorized_withdrawer: offline_keypair.pubkey(),
        commission: 0,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config_payer).unwrap();
    let vote_account = rpc_client
        .get_account(&vote_account_keypair.pubkey())
        .unwrap();
    let vote_state: VoteStateVersions = vote_account.state().unwrap();
    let authorized_withdrawer = vote_state.convert_to_current().authorized_withdrawer;
    assert_eq!(authorized_withdrawer, offline_keypair.pubkey());
    let expected_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(VoteState::size_of())
        .unwrap()
        .max(1);
    check_balance!(expected_balance, &rpc_client, &vote_account_pubkey);

    // Transfer in some more SOL
    config_payer.signers = vec![&default_signer];
    config_payer.command = CliCommand::Transfer {
        amount: SpendAmount::Some(10_000),
        to: vote_account_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    process_command(&config_payer).unwrap();
    let expected_balance = expected_balance + 10_000;
    check_balance!(expected_balance, &rpc_client, &vote_account_pubkey);

    // Authorize vote account withdrawal to another signer, offline
    let withdraw_authority = Keypair::new();
    let blockhash = rpc_client.get_latest_blockhash().unwrap();
    config_offline.command = CliCommand::VoteAuthorize {
        vote_account_pubkey,
        new_authorized_pubkey: withdraw_authority.pubkey(),
        vote_authorize: VoteAuthorize::Withdrawer,
        sign_only: true,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        authorized: 0,
        new_authorized: None,
        compute_unit_price: None,
    };
    config_offline.output_format = OutputFormat::JsonCompact;
    let sig_response = process_command(&config_offline).unwrap();
    let sign_only = parse_sign_only_reply_string(&sig_response);
    assert!(sign_only.has_all_signers());
    let offline_presigner = sign_only
        .presigner_of(&config_offline.signers[0].pubkey())
        .unwrap();
    config_payer.signers = vec![&offline_presigner];
    config_payer.command = CliCommand::VoteAuthorize {
        vote_account_pubkey,
        new_authorized_pubkey: withdraw_authority.pubkey(),
        vote_authorize: VoteAuthorize::Withdrawer,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        authorized: 0,
        new_authorized: None,
        compute_unit_price: None,
    };
    process_command(&config_payer).unwrap();
    let vote_account = rpc_client
        .get_account(&vote_account_keypair.pubkey())
        .unwrap();
    let vote_state: VoteStateVersions = vote_account.state().unwrap();
    let authorized_withdrawer = vote_state.convert_to_current().authorized_withdrawer;
    assert_eq!(authorized_withdrawer, withdraw_authority.pubkey());

    // Withdraw from vote account offline
    let destination_account = solana_sdk::pubkey::new_rand(); // Send withdrawal to new account to make balance check easy
    let blockhash = rpc_client.get_latest_blockhash().unwrap();
    let fee_payer_null_signer = NullSigner::new(&default_signer.pubkey());
    config_offline.signers = vec![&fee_payer_null_signer, &withdraw_authority];
    config_offline.command = CliCommand::WithdrawFromVoteAccount {
        vote_account_pubkey,
        withdraw_authority: 1,
        withdraw_amount: SpendAmount::Some(1_000),
        destination_account_pubkey: destination_account,
        sign_only: true,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    config_offline.output_format = OutputFormat::JsonCompact;
    let sig_response = process_command(&config_offline).unwrap();
    let sign_only = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = sign_only
        .presigner_of(&config_offline.signers[1].pubkey())
        .unwrap();
    config_payer.signers = vec![&default_signer, &offline_presigner];
    config_payer.command = CliCommand::WithdrawFromVoteAccount {
        vote_account_pubkey,
        withdraw_authority: 1,
        withdraw_amount: SpendAmount::Some(1_000),
        destination_account_pubkey: destination_account,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config_payer).unwrap();
    let expected_balance = expected_balance - 1_000;
    check_balance!(expected_balance, &rpc_client, &vote_account_pubkey);
    check_balance!(1_000, &rpc_client, &destination_account);

    // Re-assign validator identity offline
    let blockhash = rpc_client.get_latest_blockhash().unwrap();
    let new_identity_keypair = Keypair::new();
    let new_identity_null_signer = NullSigner::new(&new_identity_keypair.pubkey());
    config_offline.signers = vec![
        &fee_payer_null_signer,
        &withdraw_authority,
        &new_identity_null_signer,
    ];
    config_offline.command = CliCommand::VoteUpdateValidator {
        vote_account_pubkey,
        new_identity_account: 2,
        withdraw_authority: 1,
        sign_only: true,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config_offline).unwrap();
    config_offline.output_format = OutputFormat::JsonCompact;
    let sig_response = process_command(&config_offline).unwrap();
    let sign_only = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = sign_only
        .presigner_of(&config_offline.signers[1].pubkey())
        .unwrap();
    config_payer.signers = vec![&default_signer, &offline_presigner, &new_identity_keypair];
    config_payer.command = CliCommand::VoteUpdateValidator {
        vote_account_pubkey,
        new_identity_account: 2,
        withdraw_authority: 1,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config_payer).unwrap();

    // Close vote account offline. Must use WithdrawFromVoteAccount and specify amount, since
    // CloseVoteAccount requires RpcClient
    let destination_account = solana_sdk::pubkey::new_rand(); // Send withdrawal to new account to make balance check easy
    config_offline.signers = vec![&fee_payer_null_signer, &withdraw_authority];
    config_offline.command = CliCommand::WithdrawFromVoteAccount {
        vote_account_pubkey,
        withdraw_authority: 1,
        withdraw_amount: SpendAmount::Some(expected_balance),
        destination_account_pubkey: destination_account,
        sign_only: true,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config_offline).unwrap();
    config_offline.output_format = OutputFormat::JsonCompact;
    let sig_response = process_command(&config_offline).unwrap();
    let sign_only = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = sign_only
        .presigner_of(&config_offline.signers[1].pubkey())
        .unwrap();
    config_payer.signers = vec![&default_signer, &offline_presigner];
    config_payer.command = CliCommand::WithdrawFromVoteAccount {
        vote_account_pubkey,
        withdraw_authority: 1,
        withdraw_amount: SpendAmount::Some(expected_balance),
        destination_account_pubkey: destination_account,
        sign_only: false,
        dump_transaction_message: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        compute_unit_price: None,
    };
    process_command(&config_payer).unwrap();
    check_balance!(0, &rpc_client, &vote_account_pubkey);
    check_balance!(expected_balance, &rpc_client, &destination_account);
}
