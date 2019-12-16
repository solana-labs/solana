use serde_json::Value;
use solana_cli::cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig};
use solana_client::rpc_client::RpcClient;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{read_keypair_file, write_keypair, KeypairUtil, Signature},
};
use solana_stake_program::stake_state::Lockup;
use std::fs::remove_dir_all;
use std::str::FromStr;
use std::sync::mpsc::channel;

#[cfg(test)]
use solana_core::validator::new_validator_for_tests;
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
fn test_stake_delegation_and_deactivation() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_validator = CliConfig::default();
    config_validator.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_vote = CliConfig::default();
    config_vote.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let (vote_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&config_vote.keypair, tmp_file.as_file_mut()).unwrap();

    let mut config_stake = CliConfig::default();
    config_stake.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&config_stake.keypair, tmp_file.as_file_mut()).unwrap();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_validator.keypair.pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_validator.keypair.pubkey());

    // Create vote account
    config_validator.command = CliCommand::CreateVoteAccount {
        vote_account: read_keypair_file(&vote_keypair_file).unwrap().into(),
        node_pubkey: config_validator.keypair.pubkey(),
        authorized_voter: None,
        authorized_withdrawer: None,
        commission: 0,
    };
    process_command(&config_validator).unwrap();

    // Create stake account
    config_validator.command = CliCommand::CreateStakeAccount {
        stake_account: read_keypair_file(&stake_keypair_file).unwrap().into(),
        staker: None,
        withdrawer: None,
        lockup: Lockup {
            custodian: Pubkey::default(),
            epoch: 0,
        },
        lamports: 50_000,
    };
    process_command(&config_validator).unwrap();

    // Delegate stake
    config_validator.command = CliCommand::DelegateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        vote_account_pubkey: config_vote.keypair.pubkey(),
        force: true,
        sign_only: false,
        signers: None,
        blockhash: None,
    };
    process_command(&config_validator).unwrap();

    // Deactivate stake
    config_validator.command = CliCommand::DeactivateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        sign_only: false,
        signers: None,
        blockhash: None,
    };
    process_command(&config_validator).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_stake_delegation_and_deactivation_offline() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
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

    let mut config_vote = CliConfig::default();
    config_vote.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let (vote_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&config_vote.keypair, tmp_file.as_file_mut()).unwrap();

    let mut config_stake = CliConfig::default();
    config_stake.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let (stake_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&config_stake.keypair, tmp_file.as_file_mut()).unwrap();

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_validator.keypair.pubkey(),
        100_000,
    )
    .unwrap();
    check_balance(100_000, &rpc_client, &config_validator.keypair.pubkey());

    // Create vote account
    config_validator.command = CliCommand::CreateVoteAccount {
        vote_account: read_keypair_file(&vote_keypair_file).unwrap().into(),
        node_pubkey: config_validator.keypair.pubkey(),
        authorized_voter: None,
        authorized_withdrawer: None,
        commission: 0,
    };
    process_command(&config_validator).unwrap();

    // Create stake account
    config_validator.command = CliCommand::CreateStakeAccount {
        stake_account: read_keypair_file(&stake_keypair_file).unwrap().into(),
        staker: None,
        withdrawer: None,
        lockup: Lockup {
            custodian: Pubkey::default(),
            epoch: 0,
        },
        lamports: 50_000,
    };
    process_command(&config_validator).unwrap();

    // Delegate stake offline
    config_validator.command = CliCommand::DelegateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        vote_account_pubkey: config_vote.keypair.pubkey(),
        force: true,
        sign_only: true,
        signers: None,
        blockhash: None,
    };
    let sig_response = process_command(&config_validator).unwrap();
    let object: Value = serde_json::from_str(&sig_response).unwrap();
    let blockhash_str = object.get("blockhash").unwrap().as_str().unwrap();
    let signer_strings = object.get("signers").unwrap().as_array().unwrap();
    let signers: Vec<_> = signer_strings
        .iter()
        .map(|signer_string| {
            let mut signer = signer_string.as_str().unwrap().split('=');
            let key = Pubkey::from_str(signer.next().unwrap()).unwrap();
            let sig = Signature::from_str(signer.next().unwrap()).unwrap();
            (key, sig)
        })
        .collect();

    // Delegate stake online
    config_payer.command = CliCommand::DelegateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        vote_account_pubkey: config_vote.keypair.pubkey(),
        force: true,
        sign_only: false,
        signers: Some(signers),
        blockhash: Some(blockhash_str.parse::<Hash>().unwrap()),
    };
    process_command(&config_payer).unwrap();

    // Deactivate stake offline
    config_validator.command = CliCommand::DeactivateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        sign_only: true,
        signers: None,
        blockhash: None,
    };
    let sig_response = process_command(&config_validator).unwrap();
    let object: Value = serde_json::from_str(&sig_response).unwrap();
    let blockhash_str = object.get("blockhash").unwrap().as_str().unwrap();
    let signer_strings = object.get("signers").unwrap().as_array().unwrap();
    let signers: Vec<_> = signer_strings
        .iter()
        .map(|signer_string| {
            let mut signer = signer_string.as_str().unwrap().split('=');
            let key = Pubkey::from_str(signer.next().unwrap()).unwrap();
            let sig = Signature::from_str(signer.next().unwrap()).unwrap();
            (key, sig)
        })
        .collect();

    // Deactivate stake online
    config_payer.command = CliCommand::DeactivateStake {
        stake_account_pubkey: config_stake.keypair.pubkey(),
        sign_only: false,
        signers: Some(signers),
        blockhash: Some(blockhash_str.parse::<Hash>().unwrap()),
    };
    process_command(&config_payer).unwrap();

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
