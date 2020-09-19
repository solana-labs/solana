use solana_cli::{
    cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig},
    offline::{blockhash_query::BlockhashQuery, *},
    spend_utils::SpendAmount,
    test_utils::check_recent_balance,
};
use solana_client::rpc_client::RpcClient;
use solana_core::test_validator::TestValidator;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    account_utils::StateMut,
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use solana_vote_program::vote_state::{VoteAuthorize, VoteState, VoteStateVersions};
use std::{fs::remove_dir_all, sync::mpsc::channel};

#[test]
fn test_vote_authorize_and_withdraw() {
    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);
    let default_signer = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config.signers = vec![&default_signer];

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config.signers[0].pubkey(),
        100_000,
        &config,
    )
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
        authorized_withdrawer: Some(config.signers[0].pubkey()),
        commission: 0,
    };
    process_command(&config).unwrap();
    let vote_account = rpc_client
        .get_account_with_commitment(&vote_account_keypair.pubkey(), CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    let vote_state: VoteStateVersions = vote_account.state().unwrap();
    let authorized_withdrawer = vote_state.convert_to_current().authorized_withdrawer;
    assert_eq!(authorized_withdrawer, config.signers[0].pubkey());
    let expected_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(VoteState::size_of())
        .unwrap()
        .max(1);
    check_recent_balance(expected_balance, &rpc_client, &vote_account_pubkey);

    // Transfer in some more SOL
    config.signers = vec![&default_signer];
    config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(1_000),
        to: vote_account_pubkey,
        from: 0,
        sign_only: false,
        no_wait: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    let expected_balance = expected_balance + 1_000;
    check_recent_balance(expected_balance, &rpc_client, &vote_account_pubkey);

    // Authorize vote account withdrawal to another signer
    let withdraw_authority = Keypair::new();
    config.signers = vec![&default_signer];
    config.command = CliCommand::VoteAuthorize {
        vote_account_pubkey,
        new_authorized_pubkey: withdraw_authority.pubkey(),
        vote_authorize: VoteAuthorize::Withdrawer,
    };
    process_command(&config).unwrap();
    let vote_account = rpc_client
        .get_account_with_commitment(&vote_account_keypair.pubkey(), CommitmentConfig::recent())
        .unwrap()
        .value
        .unwrap();
    let vote_state: VoteStateVersions = vote_account.state().unwrap();
    let authorized_withdrawer = vote_state.convert_to_current().authorized_withdrawer;
    assert_eq!(authorized_withdrawer, withdraw_authority.pubkey());

    // Withdraw from vote account
    let destination_account = Pubkey::new_rand(); // Send withdrawal to new account to make balance check easy
    config.signers = vec![&default_signer, &withdraw_authority];
    config.command = CliCommand::WithdrawFromVoteAccount {
        vote_account_pubkey,
        withdraw_authority: 1,
        withdraw_amount: SpendAmount::Some(100),
        destination_account_pubkey: destination_account,
    };
    process_command(&config).unwrap();
    check_recent_balance(expected_balance - 100, &rpc_client, &vote_account_pubkey);
    check_recent_balance(100, &rpc_client, &destination_account);

    // Re-assign validator identity
    let new_identity_keypair = Keypair::new();
    config.signers.push(&new_identity_keypair);
    config.command = CliCommand::VoteUpdateValidator {
        vote_account_pubkey,
        new_identity_account: 2,
        withdraw_authority: 1,
    };
    process_command(&config).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
