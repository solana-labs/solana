use solana_clap_utils::keypair::presigner_from_pubkey_sigs;
use solana_cli::{
    cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig},
    offline::{parse_sign_only_reply_string, BlockhashQuery},
};
use solana_client::rpc_client::RpcClient;
use solana_core::validator::{TestValidator, TestValidatorOptions};
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    account_utils::StateMut,
    fee_calculator::FeeCalculator,
    nonce,
    pubkey::Pubkey,
    signature::{keypair_from_seed, Keypair, Signer},
    system_instruction::create_address_with_seed,
};
use solana_stake_program::{
    stake_instruction::LockupArgs,
    stake_state::{Lockup, StakeAuthorize, StakeState},
};
use std::{fs::remove_dir_all, sync::mpsc::channel, thread::sleep, time::Duration};

fn check_balance(expected_balance: u64, client: &RpcClient, pubkey: &Pubkey) {
    (0..5).for_each(|tries| {
        let balance = client.retry_get_balance(pubkey, 1).unwrap().unwrap();
        if balance == expected_balance {
            return;
        }
        if tries == 4 {
            assert_eq!(balance, expected_balance);
        }
        sleep(Duration::from_millis(500));
    });
}

#[test]
fn test_stake_delegation_force() {
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

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config.signers = vec![&default_signer];

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config.signers[0].pubkey(),
        100_000,
    )
    .unwrap();

    // Create vote account
    let vote_keypair = Keypair::new();
    config.signers = vec![&default_signer, &vote_keypair];
    config.command = CliCommand::CreateVoteAccount {
        seed: None,
        node_pubkey: config.signers[0].pubkey(),
        authorized_voter: None,
        authorized_withdrawer: None,
        commission: 0,
    };
    process_command(&config).unwrap();

    // Create stake account
    let stake_keypair = Keypair::new();
    config.signers = vec![&default_signer, &stake_keypair];
    config.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config).unwrap();

    // Delegate stake fails (vote account had never voted)
    config.signers = vec![&default_signer];
    config.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        vote_account_pubkey: vote_keypair.pubkey(),
        stake_authority: 0,
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap_err();

    // But if we force it, it works anyway!
    config.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        vote_account_pubkey: vote_keypair.pubkey(),
        stake_authority: 0,
        force: true,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_seed_stake_delegation_and_deactivation() {
    solana_logger::setup();

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        vote_pubkey,
        ..
    } = TestValidator::run();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let validator_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let mut config_validator = CliConfig::default();
    config_validator.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config_validator.signers = vec![&validator_keypair];

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_validator.signers[0].pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_validator.signers[0].pubkey());

    let stake_address = create_address_with_seed(
        &config_validator.signers[0].pubkey(),
        "hi there",
        &solana_stake_program::id(),
    )
    .expect("bad seed");

    // Create stake account with a seed, uses the validator config as the base,
    //   which is nice ;)
    config_validator.command = CliCommand::CreateStakeAccount {
        stake_account: 0,
        seed: Some("hi there".to_string()),
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config_validator).unwrap();

    // Delegate stake
    config_validator.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_address,
        vote_account_pubkey: vote_pubkey,
        stake_authority: 0,
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config_validator).unwrap();

    // Deactivate stake
    config_validator.command = CliCommand::DeactivateStake {
        stake_account_pubkey: stake_address,
        stake_authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config_validator).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_stake_delegation_and_deactivation() {
    solana_logger::setup();

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        vote_pubkey,
        ..
    } = TestValidator::run();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);
    let validator_keypair = Keypair::new();

    let mut config_validator = CliConfig::default();
    config_validator.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config_validator.signers = vec![&validator_keypair];

    let stake_keypair = keypair_from_seed(&[0u8; 32]).unwrap();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_validator.signers[0].pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_validator.signers[0].pubkey());

    // Create stake account
    config_validator.signers.push(&stake_keypair);
    config_validator.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config_validator).unwrap();

    // Delegate stake
    config_validator.signers.pop();
    config_validator.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        vote_account_pubkey: vote_pubkey,
        stake_authority: 0,
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config_validator).unwrap();

    // Deactivate stake
    config_validator.command = CliCommand::DeactivateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        stake_authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config_validator).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_offline_stake_delegation_and_deactivation() {
    solana_logger::setup();

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        vote_pubkey,
        ..
    } = TestValidator::run();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_validator = CliConfig::default();
    config_validator.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let validator_keypair = Keypair::new();
    config_validator.signers = vec![&validator_keypair];

    let mut config_payer = CliConfig::default();
    config_payer.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let stake_keypair = keypair_from_seed(&[0u8; 32]).unwrap();

    let mut config_offline = CliConfig::default();
    config_offline.json_rpc_url = String::default();
    config_offline.command = CliCommand::ClusterVersion;
    let offline_keypair = Keypair::new();
    config_offline.signers = vec![&offline_keypair];
    // Verfiy that we cannot reach the cluster
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_validator.signers[0].pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_validator.signers[0].pubkey());

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_offline.signers[0].pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_offline.signers[0].pubkey());

    // Create stake account
    config_validator.signers.push(&stake_keypair);
    config_validator.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: Some(config_offline.signers[0].pubkey().into()),
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config_validator).unwrap();

    // Delegate stake offline
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    config_offline.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        vote_account_pubkey: vote_pubkey,
        stake_authority: 0,
        force: false,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner =
        presigner_from_pubkey_sigs(&config_offline.signers[0].pubkey(), &signers).unwrap();
    config_payer.signers = vec![&offline_presigner];
    config_payer.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        vote_account_pubkey: vote_pubkey,
        stake_authority: 0,
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config_payer).unwrap();

    // Deactivate stake offline
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    config_offline.command = CliCommand::DeactivateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        stake_authority: 0,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner =
        presigner_from_pubkey_sigs(&config_offline.signers[0].pubkey(), &signers).unwrap();
    config_payer.signers = vec![&offline_presigner];
    config_payer.command = CliCommand::DeactivateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        stake_authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config_payer).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_nonced_stake_delegation_and_deactivation() {
    solana_logger::setup();

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        vote_pubkey,
        ..
    } = TestValidator::run();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let config_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let mut config = CliConfig::default();
    config.signers = vec![&config_keypair];
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(nonce::State::size())
        .unwrap();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config.signers[0].pubkey(),
        100_000,
    )
    .unwrap();

    // Create stake account
    let stake_keypair = Keypair::new();
    config.signers.push(&stake_keypair);
    config.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config).unwrap();

    // Create nonce account
    let nonce_account = Keypair::new();
    config.signers[1] = &nonce_account;
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: 1,
        seed: None,
        nonce_authority: Some(config.signers[0].pubkey()),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state = StateMut::<nonce::state::Versions>::state(&account)
        .unwrap()
        .convert_to_current();
    let nonce_hash = match nonce_state {
        nonce::State::Initialized(ref data) => data.blockhash,
        _ => panic!("Nonce is not initialized"),
    };

    // Delegate stake
    config.signers = vec![&config_keypair];
    config.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        vote_account_pubkey: vote_pubkey,
        stake_authority: 0,
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state = StateMut::<nonce::state::Versions>::state(&account)
        .unwrap()
        .convert_to_current();
    let nonce_hash = match nonce_state {
        nonce::State::Initialized(ref data) => data.blockhash,
        _ => panic!("Nonce is not initialized"),
    };

    // Deactivate stake
    config.command = CliCommand::DeactivateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        stake_authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(nonce_hash),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_stake_authorize() {
    solana_logger::setup();

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

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config.signers = vec![&default_signer];

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config.signers[0].pubkey(),
        100_000,
    )
    .unwrap();

    let offline_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let mut config_offline = CliConfig::default();
    config_offline.signers = vec![&offline_keypair];
    config_offline.json_rpc_url = String::default();
    let offline_authority_pubkey = config_offline.signers[0].pubkey();
    config_offline.command = CliCommand::ClusterVersion;
    // Verfiy that we cannot reach the cluster
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_offline.signers[0].pubkey(),
        100_000,
    )
    .unwrap();

    // Create stake account, identity is authority
    let stake_keypair = Keypair::new();
    let stake_account_pubkey = stake_keypair.pubkey();
    config.signers.push(&stake_keypair);
    config.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config).unwrap();

    // Assign new online stake authority
    let online_authority = Keypair::new();
    let online_authority_pubkey = online_authority.pubkey();
    config.signers.pop();
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: online_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_authority = match stake_state {
        StakeState::Initialized(meta) => meta.authorized.staker,
        _ => panic!("Unexpected stake state!"),
    };
    assert_eq!(current_authority, online_authority_pubkey);

    // Assign new offline stake authority
    config.signers.push(&online_authority);
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: offline_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: 1,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_authority = match stake_state {
        StakeState::Initialized(meta) => meta.authorized.staker,
        _ => panic!("Unexpected stake state!"),
    };
    assert_eq!(current_authority, offline_authority_pubkey);

    // Offline assignment of new nonced stake authority
    let nonced_authority = Keypair::new();
    let nonced_authority_pubkey = nonced_authority.pubkey();
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    config_offline.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: nonced_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: 0,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sign_reply = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sign_reply);
    let offline_presigner =
        presigner_from_pubkey_sigs(&offline_authority_pubkey, &signers).unwrap();
    config.signers = vec![&offline_presigner];
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: nonced_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_authority = match stake_state {
        StakeState::Initialized(meta) => meta.authorized.staker,
        _ => panic!("Unexpected stake state!"),
    };
    assert_eq!(current_authority, nonced_authority_pubkey);

    // Create nonce account
    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(nonce::State::size())
        .unwrap();
    let nonce_account = Keypair::new();
    config.signers = vec![&default_signer, &nonce_account];
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: 1,
        seed: None,
        nonce_authority: Some(offline_authority_pubkey),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state = StateMut::<nonce::state::Versions>::state(&account)
        .unwrap()
        .convert_to_current();
    let nonce_hash = match nonce_state {
        nonce::State::Initialized(ref data) => data.blockhash,
        _ => panic!("Nonce is not initialized"),
    };

    // Nonced assignment of new online stake authority
    let online_authority = Keypair::new();
    let online_authority_pubkey = online_authority.pubkey();
    config_offline.signers.push(&nonced_authority);
    config_offline.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: online_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: 1,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sign_reply = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sign_reply);
    assert_eq!(blockhash, nonce_hash);
    let offline_presigner =
        presigner_from_pubkey_sigs(&offline_authority_pubkey, &signers).unwrap();
    let nonced_authority_presigner =
        presigner_from_pubkey_sigs(&nonced_authority_pubkey, &signers).unwrap();
    config.signers = vec![&offline_presigner, &nonced_authority_presigner];
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: online_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: 1,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_authority = match stake_state {
        StakeState::Initialized(meta) => meta.authorized.staker,
        _ => panic!("Unexpected stake state!"),
    };
    assert_eq!(current_authority, online_authority_pubkey);
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state = StateMut::<nonce::state::Versions>::state(&account)
        .unwrap()
        .convert_to_current();
    let new_nonce_hash = match nonce_state {
        nonce::State::Initialized(ref data) => data.blockhash,
        _ => panic!("Nonce is not initialized"),
    };
    assert_ne!(nonce_hash, new_nonce_hash);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_stake_authorize_with_fee_payer() {
    solana_logger::setup();
    const SIG_FEE: u64 = 42;

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run_with_options(TestValidatorOptions {
        fees: SIG_FEE,
        bootstrap_validator_lamports: 42_000,
    });
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);
    let default_signer = Keypair::new();
    let default_pubkey = default_signer.pubkey();

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config.signers = vec![&default_signer];

    let payer_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let mut config_payer = CliConfig::default();
    config_payer.signers = vec![&payer_keypair];
    config_payer.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let payer_pubkey = config_payer.signers[0].pubkey();

    let mut config_offline = CliConfig::default();
    let offline_signer = Keypair::new();
    config_offline.signers = vec![&offline_signer];
    let offline_pubkey = config_offline.signers[0].pubkey();
    // Verify we're offline
    config_offline.command = CliCommand::ClusterVersion;
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &default_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &config.signers[0].pubkey());

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &payer_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &payer_pubkey);

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &offline_pubkey);

    // Create stake account, identity is authority
    let stake_keypair = Keypair::new();
    let stake_account_pubkey = stake_keypair.pubkey();
    config.signers.push(&stake_keypair);
    config.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config).unwrap();
    // `config` balance should be 50,000 - 1 stake account sig - 1 fee sig
    check_balance(50_000 - SIG_FEE - SIG_FEE, &rpc_client, &default_pubkey);

    // Assign authority with separate fee payer
    config.signers = vec![&default_signer, &payer_keypair];
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: offline_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 1,
    };
    process_command(&config).unwrap();
    // `config` balance has not changed, despite submitting the TX
    check_balance(50_000 - SIG_FEE - SIG_FEE, &rpc_client, &default_pubkey);
    // `config_payer` however has paid `config`'s authority sig
    // and `config_payer`'s fee sig
    check_balance(100_000 - SIG_FEE - SIG_FEE, &rpc_client, &payer_pubkey);

    // Assign authority with offline fee payer
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    config_offline.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: payer_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: 0,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sign_reply = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sign_reply);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    config.signers = vec![&offline_presigner];
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: payer_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    // `config`'s balance again has not changed
    check_balance(50_000 - SIG_FEE - SIG_FEE, &rpc_client, &default_pubkey);
    // `config_offline` however has paid 1 sig due to being both authority
    // and fee payer
    check_balance(100_000 - SIG_FEE, &rpc_client, &offline_pubkey);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_stake_split() {
    solana_logger::setup();

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run_with_options(TestValidatorOptions {
        fees: 1,
        bootstrap_validator_lamports: 42_000,
    });
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);
    let default_signer = Keypair::new();
    let offline_signer = Keypair::new();

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config.signers = vec![&default_signer];

    let mut config_offline = CliConfig::default();
    config_offline.json_rpc_url = String::default();
    config_offline.signers = vec![&offline_signer];
    let offline_pubkey = config_offline.signers[0].pubkey();
    // Verify we're offline
    config_offline.command = CliCommand::ClusterVersion;
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config.signers[0].pubkey(),
        500_000,
    )
    .unwrap();
    check_balance(500_000, &rpc_client, &config.signers[0].pubkey());

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &offline_pubkey);

    // Create stake account, identity is authority
    let minimum_stake_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(std::mem::size_of::<StakeState>())
        .unwrap();
    let stake_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let stake_account_pubkey = stake_keypair.pubkey();
    config.signers.push(&stake_keypair);
    config.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: Some(offline_pubkey),
        withdrawer: Some(offline_pubkey),
        lockup: Lockup::default(),
        lamports: 10 * minimum_stake_balance,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config).unwrap();
    check_balance(
        10 * minimum_stake_balance,
        &rpc_client,
        &stake_account_pubkey,
    );

    // Create nonce account
    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(nonce::State::size())
        .unwrap();
    let nonce_account = keypair_from_seed(&[1u8; 32]).unwrap();
    config.signers = vec![&default_signer, &nonce_account];
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: 1,
        seed: None,
        nonce_authority: Some(offline_pubkey),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();
    check_balance(minimum_nonce_balance, &rpc_client, &nonce_account.pubkey());

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state = StateMut::<nonce::state::Versions>::state(&account)
        .unwrap()
        .convert_to_current();
    let nonce_hash = match nonce_state {
        nonce::State::Initialized(ref data) => data.blockhash,
        _ => panic!("Nonce is not initialized"),
    };

    // Nonced offline split
    let split_account = keypair_from_seed(&[2u8; 32]).unwrap();
    check_balance(0, &rpc_client, &split_account.pubkey());
    config_offline.signers.push(&split_account);
    config_offline.command = CliCommand::SplitStake {
        stake_account_pubkey: stake_account_pubkey,
        stake_authority: 0,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        split_stake_account: 1,
        seed: None,
        lamports: 2 * minimum_stake_balance,
        fee_payer: 0,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    config.signers = vec![&offline_presigner, &split_account];
    config.command = CliCommand::SplitStake {
        stake_account_pubkey: stake_account_pubkey,
        stake_authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        split_stake_account: 1,
        seed: None,
        lamports: 2 * minimum_stake_balance,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    check_balance(
        8 * minimum_stake_balance,
        &rpc_client,
        &stake_account_pubkey,
    );
    check_balance(
        2 * minimum_stake_balance,
        &rpc_client,
        &split_account.pubkey(),
    );

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_stake_set_lockup() {
    solana_logger::setup();

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run_with_options(TestValidatorOptions {
        fees: 1,
        bootstrap_validator_lamports: 42_000,
    });
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);
    let default_signer = Keypair::new();
    let offline_signer = Keypair::new();

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config.signers = vec![&default_signer];

    let mut config_offline = CliConfig::default();
    config_offline.json_rpc_url = String::default();
    config_offline.signers = vec![&offline_signer];
    let offline_pubkey = config_offline.signers[0].pubkey();
    // Verify we're offline
    config_offline.command = CliCommand::ClusterVersion;
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config.signers[0].pubkey(),
        500_000,
    )
    .unwrap();
    check_balance(500_000, &rpc_client, &config.signers[0].pubkey());

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &offline_pubkey);

    // Create stake account, identity is authority
    let minimum_stake_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(std::mem::size_of::<StakeState>())
        .unwrap();

    let stake_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let stake_account_pubkey = stake_keypair.pubkey();

    let mut lockup = Lockup::default();
    lockup.custodian = config.signers[0].pubkey();

    config.signers.push(&stake_keypair);
    config.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: Some(offline_pubkey),
        withdrawer: Some(offline_pubkey),
        lockup,
        lamports: 10 * minimum_stake_balance,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config).unwrap();
    check_balance(
        10 * minimum_stake_balance,
        &rpc_client,
        &stake_account_pubkey,
    );

    // Online set lockup
    let lockup = LockupArgs {
        unix_timestamp: Some(1581534570),
        epoch: Some(200),
        custodian: None,
    };
    config.signers.pop();
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_lockup = match stake_state {
        StakeState::Initialized(meta) => meta.lockup,
        _ => panic!("Unexpected stake state!"),
    };
    assert_eq!(
        current_lockup.unix_timestamp,
        lockup.unix_timestamp.unwrap()
    );
    assert_eq!(current_lockup.epoch, lockup.epoch.unwrap());
    assert_eq!(current_lockup.custodian, config.signers[0].pubkey());

    // Set custodian to another pubkey
    let online_custodian = Keypair::new();
    let online_custodian_pubkey = online_custodian.pubkey();

    let lockup = LockupArgs {
        unix_timestamp: Some(1581534571),
        epoch: Some(201),
        custodian: Some(online_custodian_pubkey),
    };
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();

    let lockup = LockupArgs {
        unix_timestamp: Some(1581534572),
        epoch: Some(202),
        custodian: None,
    };
    config.signers = vec![&default_signer, &online_custodian];
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: 1,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_lockup = match stake_state {
        StakeState::Initialized(meta) => meta.lockup,
        _ => panic!("Unexpected stake state!"),
    };
    assert_eq!(
        current_lockup.unix_timestamp,
        lockup.unix_timestamp.unwrap()
    );
    assert_eq!(current_lockup.epoch, lockup.epoch.unwrap());
    assert_eq!(current_lockup.custodian, online_custodian_pubkey);

    // Set custodian to offline pubkey
    let lockup = LockupArgs {
        unix_timestamp: Some(1581534573),
        epoch: Some(203),
        custodian: Some(offline_pubkey),
    };
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: 1,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();

    // Create nonce account
    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(nonce::State::size())
        .unwrap();
    let nonce_account = keypair_from_seed(&[1u8; 32]).unwrap();
    let nonce_account_pubkey = nonce_account.pubkey();
    config.signers = vec![&default_signer, &nonce_account];
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: 1,
        seed: None,
        nonce_authority: Some(offline_pubkey),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();
    check_balance(minimum_nonce_balance, &rpc_client, &nonce_account_pubkey);

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account_pubkey).unwrap();
    let nonce_state = StateMut::<nonce::state::Versions>::state(&account)
        .unwrap()
        .convert_to_current();
    let nonce_hash = match nonce_state {
        nonce::State::Initialized(ref data) => data.blockhash,
        _ => panic!("Nonce is not initialized"),
    };

    // Nonced offline set lockup
    let lockup = LockupArgs {
        unix_timestamp: Some(1581534576),
        epoch: Some(222),
        custodian: None,
    };
    config_offline.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: 0,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_account_pubkey),
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    config.signers = vec![&offline_presigner];
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_account_pubkey),
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_lockup = match stake_state {
        StakeState::Initialized(meta) => meta.lockup,
        _ => panic!("Unexpected stake state!"),
    };
    assert_eq!(
        current_lockup.unix_timestamp,
        lockup.unix_timestamp.unwrap()
    );
    assert_eq!(current_lockup.epoch, lockup.epoch.unwrap());
    assert_eq!(current_lockup.custodian, offline_pubkey);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_offline_nonced_create_stake_account_and_withdraw() {
    solana_logger::setup();

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

    let mut config = CliConfig::default();
    let default_signer = keypair_from_seed(&[1u8; 32]).unwrap();
    config.signers = vec![&default_signer];
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_offline = CliConfig::default();
    let offline_signer = keypair_from_seed(&[2u8; 32]).unwrap();
    config_offline.signers = vec![&offline_signer];
    let offline_pubkey = config_offline.signers[0].pubkey();
    config_offline.json_rpc_url = String::default();
    config_offline.command = CliCommand::ClusterVersion;
    // Verfiy that we cannot reach the cluster
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config.signers[0].pubkey(),
        200_000,
    )
    .unwrap();
    check_balance(200_000, &rpc_client, &config.signers[0].pubkey());

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &offline_pubkey);

    // Create nonce account
    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(nonce::State::size())
        .unwrap();
    let nonce_account = keypair_from_seed(&[3u8; 32]).unwrap();
    let nonce_pubkey = nonce_account.pubkey();
    config.signers.push(&nonce_account);
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: 1,
        seed: None,
        nonce_authority: Some(offline_pubkey),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state = StateMut::<nonce::state::Versions>::state(&account)
        .unwrap()
        .convert_to_current();
    let nonce_hash = match nonce_state {
        nonce::State::Initialized(ref data) => data.blockhash,
        _ => panic!("Nonce is not initialized"),
    };

    // Create stake account offline
    let stake_keypair = keypair_from_seed(&[4u8; 32]).unwrap();
    let stake_pubkey = stake_keypair.pubkey();
    config_offline.signers.push(&stake_keypair);
    config_offline.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    let stake_presigner = presigner_from_pubkey_sigs(&stake_pubkey, &signers).unwrap();
    config.signers = vec![&offline_presigner, &stake_presigner];
    config.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: None,
        staker: Some(offline_pubkey),
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config).unwrap();
    check_balance(50_000, &rpc_client, &stake_pubkey);

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state = StateMut::<nonce::state::Versions>::state(&account)
        .unwrap()
        .convert_to_current();
    let nonce_hash = match nonce_state {
        nonce::State::Initialized(ref data) => data.blockhash,
        _ => panic!("Nonce is not initialized"),
    };

    // Offline, nonced stake-withdraw
    let recipient = keypair_from_seed(&[5u8; 32]).unwrap();
    let recipient_pubkey = recipient.pubkey();
    config_offline.signers.pop();
    config_offline.command = CliCommand::WithdrawStake {
        stake_account_pubkey: stake_pubkey,
        destination_account_pubkey: recipient_pubkey,
        lamports: 42,
        withdraw_authority: 0,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    config.signers = vec![&offline_presigner];
    config.command = CliCommand::WithdrawStake {
        stake_account_pubkey: stake_pubkey,
        destination_account_pubkey: recipient_pubkey,
        lamports: 42,
        withdraw_authority: 0,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    check_balance(42, &rpc_client, &recipient_pubkey);

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state = StateMut::<nonce::state::Versions>::state(&account)
        .unwrap()
        .convert_to_current();
    let nonce_hash = match nonce_state {
        nonce::State::Initialized(ref data) => data.blockhash,
        _ => panic!("Nonce is not initialized"),
    };

    // Create another stake account. This time with seed
    let seed = "seedy";
    config_offline.signers = vec![&offline_signer, &stake_keypair];
    config_offline.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: Some(seed.to_string()),
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    let stake_presigner = presigner_from_pubkey_sigs(&stake_pubkey, &signers).unwrap();
    config.signers = vec![&offline_presigner, &stake_presigner];
    config.command = CliCommand::CreateStakeAccount {
        stake_account: 1,
        seed: Some(seed.to_string()),
        staker: Some(offline_pubkey.into()),
        withdrawer: Some(offline_pubkey.into()),
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: 0,
        fee_payer: 0,
        from: 0,
    };
    process_command(&config).unwrap();
    let seed_address =
        create_address_with_seed(&stake_pubkey, seed, &solana_stake_program::id()).unwrap();
    check_balance(50_000, &rpc_client, &seed_address);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
