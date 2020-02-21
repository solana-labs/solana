use solana_clap_utils::keypair::presigner_from_pubkey_sigs;
use solana_cli::{
    cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig},
    offline::{parse_sign_only_reply_string, BlockhashQuery},
};
use solana_client::rpc_client::RpcClient;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    account_utils::StateMut,
    fee_calculator::FeeCalculator,
    nonce_state::NonceState,
    pubkey::Pubkey,
    signature::{keypair_from_seed, read_keypair_file, write_keypair, Keypair, Signer},
    system_instruction::create_address_with_seed,
};
use solana_stake_program::stake_state::{Lockup, StakeAuthorize, StakeState};
use std::fs::remove_dir_all;
use std::sync::mpsc::channel;

#[cfg(test)]
use solana_core::validator::{
    new_validator_for_tests, new_validator_for_tests_ex, new_validator_for_tests_with_vote_pubkey,
};
use std::rc::Rc;
use std::thread::sleep;
use std::time::Duration;

use tempfile::NamedTempFile;

fn make_tmp_file() -> (String, NamedTempFile) {
    let tmp_file = NamedTempFile::new().unwrap();
    (String::from(tmp_file.path().to_str().unwrap()), tmp_file)
}

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
    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &config.keypair.pubkey(), 100_000)
        .unwrap();

    // Create vote account
    let vote_keypair = Keypair::new();
    let (vote_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&vote_keypair, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateVoteAccount {
        vote_account: read_keypair_file(&vote_keypair_file).unwrap().into(),
        seed: None,
        node_pubkey: config.keypair.pubkey(),
        authorized_voter: None,
        authorized_withdrawer: None,
        commission: 0,
    };
    process_command(&config).unwrap();

    // Create stake account
    let stake_keypair = Keypair::new();
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&stake_keypair, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&stake_keypair_file).unwrap().into()),
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config).unwrap();

    // Delegate stake fails (vote account had never voted)
    config.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        vote_account_pubkey: vote_keypair.pubkey(),
        stake_authority: None,
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config).unwrap_err();

    // But if we force it, it works anyway!
    config.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        vote_account_pubkey: vote_keypair.pubkey(),
        stake_authority: None,
        force: true,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_seed_stake_delegation_and_deactivation() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path, vote_pubkey) =
        new_validator_for_tests_with_vote_pubkey();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let validator_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let (validator_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&validator_keypair, tmp_file.as_file_mut()).unwrap();
    let mut config_validator = CliConfig::default();
    config_validator.keypair = validator_keypair.into();
    config_validator.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_stake = CliConfig::default();
    config_stake.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_validator.keypair.pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_validator.keypair.pubkey());

    let stake_address = create_address_with_seed(
        &config_validator.keypair.pubkey(),
        "hi there",
        &solana_stake_program::id(),
    )
    .expect("bad seed");

    // Create stake account with a seed, uses the validator config as the base,
    //   which is nice ;)
    config_validator.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&validator_keypair_file).unwrap().into()),
        seed: Some("hi there".to_string()),
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config_validator).unwrap();

    // Delegate stake
    config_validator.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_address,
        vote_account_pubkey: vote_pubkey,
        stake_authority: None,
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config_validator).unwrap();

    // Deactivate stake
    config_validator.command = CliCommand::DeactivateStake {
        stake_account_pubkey: stake_address,
        stake_authority: None,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config_validator).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_stake_delegation_and_deactivation() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path, vote_pubkey) =
        new_validator_for_tests_with_vote_pubkey();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_validator = CliConfig::default();
    config_validator.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let stake_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&stake_keypair, tmp_file.as_file_mut()).unwrap();
    let mut config_stake = CliConfig::default();
    config_stake.keypair = stake_keypair.into();
    config_stake.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_validator.keypair.pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_validator.keypair.pubkey());

    // Create stake account
    config_validator.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&stake_keypair_file).unwrap().into()),
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config_validator).unwrap();

    // Delegate stake
    config_validator.command = CliCommand::DelegateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        vote_account_pubkey: vote_pubkey,
        stake_authority: None,
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config_validator).unwrap();

    // Deactivate stake
    config_validator.command = CliCommand::DeactivateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        stake_authority: None,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config_validator).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_offline_stake_delegation_and_deactivation() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path, vote_pubkey) =
        new_validator_for_tests_with_vote_pubkey();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_validator = CliConfig::default();
    config_validator.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_payer = CliConfig::default();
    config_payer.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let stake_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&stake_keypair, tmp_file.as_file_mut()).unwrap();
    let mut config_stake = CliConfig::default();
    config_stake.keypair = stake_keypair.into();
    config_stake.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_offline = CliConfig::default();
    config_offline.json_rpc_url = String::default();
    config_offline.command = CliCommand::ClusterVersion;
    // Verfiy that we cannot reach the cluster
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_validator.keypair.pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_validator.keypair.pubkey());

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_offline.keypair.pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_offline.keypair.pubkey());

    // Create stake account
    config_validator.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&stake_keypair_file).unwrap().into()),
        seed: None,
        staker: Some(config_offline.keypair.pubkey().into()),
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config_validator).unwrap();

    // Delegate stake offline
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    config_offline.command = CliCommand::DelegateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        vote_account_pubkey: vote_pubkey,
        stake_authority: None,
        force: false,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner =
        presigner_from_pubkey_sigs(&config_offline.keypair.pubkey(), &signers).unwrap();
    config_payer.command = CliCommand::DelegateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        vote_account_pubkey: vote_pubkey,
        stake_authority: Some(offline_presigner.clone().into()),
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: Some(offline_presigner.clone().into()),
    };
    process_command(&config_payer).unwrap();

    // Deactivate stake offline
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    config_offline.command = CliCommand::DeactivateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        stake_authority: None,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner =
        presigner_from_pubkey_sigs(&config_offline.keypair.pubkey(), &signers).unwrap();
    config_payer.command = CliCommand::DeactivateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        stake_authority: Some(offline_presigner.clone().into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: Some(offline_presigner.clone().into()),
    };
    process_command(&config_payer).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_nonced_stake_delegation_and_deactivation() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path, vote_pubkey) =
        new_validator_for_tests_with_vote_pubkey();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let config_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let (config_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&config_keypair, tmp_file.as_file_mut()).unwrap();
    let mut config = CliConfig::default();
    config.keypair = config_keypair.into();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(NonceState::size())
        .unwrap();

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &config.keypair.pubkey(), 100_000)
        .unwrap();

    // Create stake account
    let stake_keypair = Keypair::new();
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&stake_keypair, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&stake_keypair_file).unwrap().into()),
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config).unwrap();

    // Create nonce account
    let nonce_account = Keypair::new();
    let (nonce_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&nonce_account, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: Rc::new(read_keypair_file(&nonce_keypair_file).unwrap().into()),
        seed: None,
        nonce_authority: Some(config.keypair.pubkey()),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };

    // Delegate stake
    config.command = CliCommand::DelegateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        vote_account_pubkey: vote_pubkey,
        stake_authority: None,
        force: false,
        sign_only: false,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config).unwrap();

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };

    // Deactivate stake
    config.command = CliCommand::DeactivateStake {
        stake_account_pubkey: stake_keypair.pubkey(),
        stake_authority: None,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(nonce_hash),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: Some(read_keypair_file(&config_keypair_file).unwrap().into()),
        fee_payer: None,
    };
    process_command(&config).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_stake_authorize() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &config.keypair.pubkey(), 100_000)
        .unwrap();

    let offline_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let (offline_authority_file, mut tmp_file) = make_tmp_file();
    write_keypair(&offline_keypair, tmp_file.as_file_mut()).unwrap();
    let mut config_offline = CliConfig::default();
    config_offline.keypair = offline_keypair.into();
    config_offline.json_rpc_url = String::default();
    let offline_authority_pubkey = config_offline.keypair.pubkey();
    config_offline.command = CliCommand::ClusterVersion;
    // Verfiy that we cannot reach the cluster
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_offline.keypair.pubkey(),
        100_000,
    )
    .unwrap();

    // Create stake account, identity is authority
    let stake_keypair = Keypair::new();
    let stake_account_pubkey = stake_keypair.pubkey();
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&stake_keypair, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&stake_keypair_file).unwrap().into()),
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config).unwrap();

    // Assign new online stake authority
    let online_authority = Keypair::new();
    let online_authority_pubkey = online_authority.pubkey();
    let (online_authority_file, mut tmp_file) = make_tmp_file();
    write_keypair(&online_authority, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: online_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: None,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
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
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: offline_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: Some(read_keypair_file(&online_authority_file).unwrap().into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
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
    let (nonced_authority_file, mut tmp_file) = make_tmp_file();
    write_keypair(&nonced_authority, tmp_file.as_file_mut()).unwrap();
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    config_offline.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: nonced_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: Some(read_keypair_file(&offline_authority_file).unwrap().into()),
        sign_only: true,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    let sign_reply = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sign_reply);
    let offline_presigner =
        presigner_from_pubkey_sigs(&offline_authority_pubkey, &signers).unwrap();
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: nonced_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: Some(offline_presigner.clone().into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: Some(offline_presigner.clone().into()),
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
        .get_minimum_balance_for_rent_exemption(NonceState::size())
        .unwrap();
    let nonce_account = Keypair::new();
    let (nonce_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&nonce_account, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: Rc::new(read_keypair_file(&nonce_keypair_file).unwrap().into()),
        seed: None,
        nonce_authority: Some(config_offline.keypair.pubkey()),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };

    // Nonced assignment of new online stake authority
    let online_authority = Keypair::new();
    let online_authority_pubkey = online_authority.pubkey();
    let (_online_authority_file, mut tmp_file) = make_tmp_file();
    write_keypair(&online_authority, tmp_file.as_file_mut()).unwrap();
    config_offline.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: online_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: Some(read_keypair_file(&nonced_authority_file).unwrap().into()),
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: None,
        fee_payer: None,
    };
    let sign_reply = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sign_reply);
    assert_eq!(blockhash, nonce_hash);
    let offline_presigner =
        presigner_from_pubkey_sigs(&offline_authority_pubkey, &signers).unwrap();
    let nonced_authority_presigner =
        presigner_from_pubkey_sigs(&nonced_authority_pubkey, &signers).unwrap();
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: online_authority_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: Some(nonced_authority_presigner.clone().into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: Some(offline_presigner.clone().into()),
        fee_payer: Some(offline_presigner.clone().into()),
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
    let nonce_state: NonceState = account.state().unwrap();
    let new_nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
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

    let (server, leader_data, alice, ledger_path, _voter) =
        new_validator_for_tests_ex(SIG_FEE, 42_000);
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let payer_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let (payer_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&payer_keypair, tmp_file.as_file_mut()).unwrap();
    let mut config_payer = CliConfig::default();
    config_payer.keypair = payer_keypair.into();
    config_payer.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let payer_pubkey = config_payer.keypair.pubkey();

    let mut config_offline = CliConfig::default();
    let offline_pubkey = config_offline.keypair.pubkey();
    // Verify we're offline
    config_offline.command = CliCommand::ClusterVersion;
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &config.keypair.pubkey(), 100_000)
        .unwrap();
    check_balance(100_000, &rpc_client, &config.keypair.pubkey());

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &payer_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &payer_pubkey);

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &offline_pubkey);

    // Create stake account, identity is authority
    let stake_keypair = Keypair::new();
    let stake_account_pubkey = stake_keypair.pubkey();
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&stake_keypair, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&stake_keypair_file).unwrap().into()),
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config).unwrap();
    // `config` balance should be 50,000 - 1 stake account sig - 1 fee sig
    check_balance(
        50_000 - SIG_FEE - SIG_FEE,
        &rpc_client,
        &config.keypair.pubkey(),
    );

    // Assign authority with separate fee payer
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: offline_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: None,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: Some(read_keypair_file(&payer_keypair_file).unwrap().into()),
    };
    process_command(&config).unwrap();
    // `config` balance has not changed, despite submitting the TX
    check_balance(
        50_000 - SIG_FEE - SIG_FEE,
        &rpc_client,
        &config.keypair.pubkey(),
    );
    // `config_payer` however has paid `config`'s authority sig
    // and `config_payer`'s fee sig
    check_balance(
        100_000 - SIG_FEE - SIG_FEE,
        &rpc_client,
        &config_payer.keypair.pubkey(),
    );

    // Assign authority with offline fee payer
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    config_offline.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: payer_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: None,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    let sign_reply = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sign_reply);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    config.command = CliCommand::StakeAuthorize {
        stake_account_pubkey,
        new_authorized_pubkey: payer_pubkey,
        stake_authorize: StakeAuthorize::Staker,
        authority: Some(offline_presigner.clone().into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: Some(offline_presigner.clone().into()),
    };
    process_command(&config).unwrap();
    // `config`'s balance again has not changed
    check_balance(
        50_000 - SIG_FEE - SIG_FEE,
        &rpc_client,
        &config.keypair.pubkey(),
    );
    // `config_offline` however has paid 1 sig due to being both authority
    // and fee payer
    check_balance(
        100_000 - SIG_FEE,
        &rpc_client,
        &config_offline.keypair.pubkey(),
    );

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_stake_split() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path, _voter) = new_validator_for_tests_ex(1, 42_000);
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_offline = CliConfig::default();
    config_offline.json_rpc_url = String::default();
    let offline_pubkey = config_offline.keypair.pubkey();
    // Verify we're offline
    config_offline.command = CliCommand::ClusterVersion;
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &config.keypair.pubkey(), 500_000)
        .unwrap();
    check_balance(500_000, &rpc_client, &config.keypair.pubkey());

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &offline_pubkey);

    // Create stake account, identity is authority
    let minimum_stake_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(std::mem::size_of::<StakeState>())
        .unwrap();
    let stake_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let stake_account_pubkey = stake_keypair.pubkey();
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&stake_keypair, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&stake_keypair_file).unwrap().into()),
        seed: None,
        staker: Some(offline_pubkey),
        withdrawer: Some(offline_pubkey),
        lockup: Lockup::default(),
        lamports: 10 * minimum_stake_balance,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config).unwrap();
    check_balance(
        10 * minimum_stake_balance,
        &rpc_client,
        &stake_account_pubkey,
    );

    // Create nonce account
    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(NonceState::size())
        .unwrap();
    let nonce_account = keypair_from_seed(&[1u8; 32]).unwrap();
    let nonce_account_pubkey = nonce_account.pubkey();
    let (nonce_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&nonce_account, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: Rc::new(read_keypair_file(&nonce_keypair_file).unwrap().into()),
        seed: None,
        nonce_authority: Some(offline_pubkey),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();
    check_balance(minimum_nonce_balance, &rpc_client, &nonce_account_pubkey);

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account_pubkey).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };

    // Nonced offline split
    let split_account = keypair_from_seed(&[2u8; 32]).unwrap();
    let (split_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&split_account, tmp_file.as_file_mut()).unwrap();
    check_balance(0, &rpc_client, &split_account.pubkey());
    config_offline.command = CliCommand::SplitStake {
        stake_account_pubkey: stake_account_pubkey,
        stake_authority: None,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_account_pubkey),
        nonce_authority: None,
        split_stake_account: Rc::new(read_keypair_file(&split_keypair_file).unwrap().into()),
        seed: None,
        lamports: 2 * minimum_stake_balance,
        fee_payer: None,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    config.command = CliCommand::SplitStake {
        stake_account_pubkey: stake_account_pubkey,
        stake_authority: Some(offline_presigner.clone().into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_account_pubkey),
        nonce_authority: Some(offline_presigner.clone().into()),
        split_stake_account: Rc::new(read_keypair_file(&split_keypair_file).unwrap().into()),
        seed: None,
        lamports: 2 * minimum_stake_balance,
        fee_payer: Some(offline_presigner.clone().into()),
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

    let (server, leader_data, alice, ledger_path, _voter) = new_validator_for_tests_ex(1, 42_000);
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_offline = CliConfig::default();
    config_offline.json_rpc_url = String::default();
    let offline_pubkey = config_offline.keypair.pubkey();
    // Verify we're offline
    config_offline.command = CliCommand::ClusterVersion;
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &config.keypair.pubkey(), 500_000)
        .unwrap();
    check_balance(500_000, &rpc_client, &config.keypair.pubkey());

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_pubkey, 100_000).unwrap();
    check_balance(100_000, &rpc_client, &offline_pubkey);

    // Create stake account, identity is authority
    let minimum_stake_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(std::mem::size_of::<StakeState>())
        .unwrap();

    let stake_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    let stake_account_pubkey = stake_keypair.pubkey();
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&stake_keypair, tmp_file.as_file_mut()).unwrap();

    let mut lockup = Lockup::default();
    lockup.custodian = config.keypair.pubkey();

    config.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&stake_keypair_file).unwrap().into()),
        seed: None,
        staker: Some(offline_pubkey),
        withdrawer: Some(offline_pubkey),
        lockup,
        lamports: 10 * minimum_stake_balance,
        sign_only: false,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config).unwrap();
    check_balance(
        10 * minimum_stake_balance,
        &rpc_client,
        &stake_account_pubkey,
    );

    // Online set lockup
    let mut lockup = Lockup {
        unix_timestamp: 1581534570,
        epoch: 200,
        custodian: Pubkey::default(),
    };
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: None,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_lockup = match stake_state {
        StakeState::Initialized(meta) => meta.lockup,
        _ => panic!("Unexpected stake state!"),
    };
    lockup.custodian = config.keypair.pubkey(); // Default new_custodian is config.keypair
    assert_eq!(current_lockup, lockup);

    // Set custodian to another pubkey
    let online_custodian = Keypair::new();
    let online_custodian_pubkey = online_custodian.pubkey();
    let (online_custodian_file, mut tmp_file) = make_tmp_file();
    write_keypair(&online_custodian, tmp_file.as_file_mut()).unwrap();

    let lockup = Lockup {
        unix_timestamp: 1581534571,
        epoch: 201,
        custodian: online_custodian_pubkey,
    };
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: None,
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config).unwrap();

    let mut lockup = Lockup {
        unix_timestamp: 1581534572,
        epoch: 202,
        custodian: Pubkey::default(),
    };
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: Some(read_keypair_file(&online_custodian_file).unwrap().into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_lockup = match stake_state {
        StakeState::Initialized(meta) => meta.lockup,
        _ => panic!("Unexpected stake state!"),
    };
    lockup.custodian = online_custodian_pubkey; // Default new_custodian is designated custodian
    assert_eq!(current_lockup, lockup);

    // Set custodian to offline pubkey
    let lockup = Lockup {
        unix_timestamp: 1581534573,
        epoch: 203,
        custodian: offline_pubkey,
    };
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: Some(online_custodian.into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::default(),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config).unwrap();

    // Create nonce account
    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(NonceState::size())
        .unwrap();
    let nonce_account = keypair_from_seed(&[1u8; 32]).unwrap();
    let nonce_account_pubkey = nonce_account.pubkey();
    let (nonce_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&nonce_account, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: Rc::new(read_keypair_file(&nonce_keypair_file).unwrap().into()),
        seed: None,
        nonce_authority: Some(offline_pubkey),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();
    check_balance(minimum_nonce_balance, &rpc_client, &nonce_account_pubkey);

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account_pubkey).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };

    // Nonced offline set lockup
    let lockup = Lockup {
        unix_timestamp: 1581534576,
        epoch: 222,
        custodian: offline_pubkey,
    };
    config_offline.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: None,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_account_pubkey),
        nonce_authority: None,
        fee_payer: None,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    config.command = CliCommand::StakeSetLockup {
        stake_account_pubkey,
        lockup,
        custodian: Some(offline_presigner.clone().into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_account_pubkey),
        nonce_authority: Some(offline_presigner.clone().into()),
        fee_payer: Some(offline_presigner.clone().into()),
    };
    process_command(&config).unwrap();
    let stake_account = rpc_client.get_account(&stake_account_pubkey).unwrap();
    let stake_state: StakeState = stake_account.state().unwrap();
    let current_lockup = match stake_state {
        StakeState::Initialized(meta) => meta.lockup,
        _ => panic!("Unexpected stake state!"),
    };
    assert_eq!(current_lockup, lockup);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_offline_nonced_create_stake_account_and_withdraw() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config = CliConfig::default();
    config.keypair = keypair_from_seed(&[1u8; 32]).unwrap().into();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_offline = CliConfig::default();
    config_offline.keypair = keypair_from_seed(&[2u8; 32]).unwrap().into();
    let offline_pubkey = config_offline.keypair.pubkey();
    config_offline.json_rpc_url = String::default();
    config_offline.command = CliCommand::ClusterVersion;
    // Verfiy that we cannot reach the cluster
    process_command(&config_offline).unwrap_err();

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &config.keypair.pubkey(), 200_000)
        .unwrap();
    check_balance(200_000, &rpc_client, &config.keypair.pubkey());

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_offline.keypair.pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_offline.keypair.pubkey());

    // Create nonce account
    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(NonceState::size())
        .unwrap();
    let nonce_account = keypair_from_seed(&[3u8; 32]).unwrap();
    let nonce_pubkey = nonce_account.pubkey();
    let (nonce_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&nonce_account, tmp_file.as_file_mut()).unwrap();
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: Rc::new(read_keypair_file(&nonce_keypair_file).unwrap().into()),
        seed: None,
        nonce_authority: Some(offline_pubkey),
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };

    // Create stake account offline
    let stake_keypair = keypair_from_seed(&[4u8; 32]).unwrap();
    let stake_pubkey = stake_keypair.pubkey();
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&stake_keypair, tmp_file.as_file_mut()).unwrap();
    config_offline.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(read_keypair_file(&stake_keypair_file).unwrap().into()),
        seed: None,
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    config.command = CliCommand::CreateStakeAccount {
        stake_account: presigner_from_pubkey_sigs(&stake_pubkey, &signers)
            .map(|p| Rc::new(p.into()))
            .unwrap(),
        seed: None,
        staker: Some(offline_pubkey),
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: Some(offline_presigner.clone().into()),
        fee_payer: Some(offline_presigner.clone().into()),
        from: Some(offline_presigner.clone().into()),
    };
    process_command(&config).unwrap();
    check_balance(50_000, &rpc_client, &stake_pubkey);

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };

    // Offline, nonced stake-withdraw
    let recipient = keypair_from_seed(&[5u8; 32]).unwrap();
    let recipient_pubkey = recipient.pubkey();
    config_offline.command = CliCommand::WithdrawStake {
        stake_account_pubkey: stake_pubkey,
        destination_account_pubkey: recipient_pubkey,
        lamports: 42,
        withdraw_authority: None,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: None,
        fee_payer: None,
    };
    let sig_response = process_command(&config_offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sig_response);
    let offline_presigner = presigner_from_pubkey_sigs(&offline_pubkey, &signers).unwrap();
    config.command = CliCommand::WithdrawStake {
        stake_account_pubkey: stake_pubkey,
        destination_account_pubkey: recipient_pubkey,
        lamports: 42,
        withdraw_authority: Some(offline_presigner.clone().into()),
        sign_only: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: Some(offline_presigner.clone().into()),
        fee_payer: Some(offline_presigner.clone().into()),
    };
    process_command(&config).unwrap();
    check_balance(42, &rpc_client, &recipient_pubkey);

    // Test that offline derived addresses fail
    config_offline.command = CliCommand::CreateStakeAccount {
        stake_account: Rc::new(Box::new(read_keypair_file(&stake_keypair_file).unwrap())),
        seed: Some("fail".to_string()),
        staker: None,
        withdrawer: None,
        lockup: Lockup::default(),
        lamports: 50_000,
        sign_only: true,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_pubkey),
        nonce_authority: None,
        fee_payer: None,
        from: None,
    };
    process_command(&config_offline).unwrap_err();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
