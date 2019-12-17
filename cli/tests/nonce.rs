use solana_cli::cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig};
use solana_client::rpc_client::RpcClient;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{read_keypair_file, write_keypair, Keypair, KeypairUtil},
};
use std::fs::remove_dir_all;
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
fn test_nonce() {
    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_payer = CliConfig::default();
    config_payer.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_nonce = CliConfig::default();
    config_nonce.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let (keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&config_nonce.keypair, tmp_file.as_file_mut()).unwrap();

    full_battery_tests(
        &rpc_client,
        &drone_addr,
        &mut config_payer,
        &mut config_nonce,
        &keypair_file,
        &keypair_file,
    );

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_nonce_with_authority() {
    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let (sender, receiver) = channel();
    run_local_drone(alice, sender, None);
    let drone_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_payer = CliConfig::default();
    config_payer.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_nonce = CliConfig::default();
    config_nonce.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let (nonce_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&config_nonce.keypair, tmp_file.as_file_mut()).unwrap();

    let nonce_authority = Keypair::new();
    let (authority_keypair_file, mut tmp_file2) = make_tmp_file();
    write_keypair(&nonce_authority, tmp_file2.as_file_mut()).unwrap();

    full_battery_tests(
        &rpc_client,
        &drone_addr,
        &mut config_payer,
        &mut config_nonce,
        &nonce_keypair_file,
        &authority_keypair_file,
    );

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

fn full_battery_tests(
    rpc_client: &RpcClient,
    drone_addr: &std::net::SocketAddr,
    config_payer: &mut CliConfig,
    config_nonce: &mut CliConfig,
    nonce_keypair_file: &str,
    authority_keypair_file: &str,
) {
    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_payer.keypair.pubkey(),
        2000,
    )
    .unwrap();
    check_balance(2000, &rpc_client, &config_payer.keypair.pubkey());

    // Create nonce account
    config_payer.command = CliCommand::CreateNonceAccount {
        nonce_account: read_keypair_file(&nonce_keypair_file).unwrap().into(),
        nonce_authority: read_keypair_file(&authority_keypair_file).unwrap().pubkey(),
        lamports: 1000,
    };
    process_command(&config_payer).unwrap();
    check_balance(1000, &rpc_client, &config_payer.keypair.pubkey());
    check_balance(1000, &rpc_client, &config_nonce.keypair.pubkey());

    // Get nonce
    config_payer.command = CliCommand::GetNonce(config_nonce.keypair.pubkey());
    let first_nonce_string = process_command(&config_payer).unwrap();
    let first_nonce = first_nonce_string.parse::<Hash>().unwrap();

    // Get nonce
    config_payer.command = CliCommand::GetNonce(config_nonce.keypair.pubkey());
    let second_nonce_string = process_command(&config_payer).unwrap();
    let second_nonce = second_nonce_string.parse::<Hash>().unwrap();

    assert_eq!(first_nonce, second_nonce);

    // New nonce
    config_payer.command = CliCommand::NewNonce {
        nonce_account: read_keypair_file(&nonce_keypair_file).unwrap().pubkey(),
        nonce_authority: read_keypair_file(&authority_keypair_file).unwrap().into(),
    };
    process_command(&config_payer).unwrap();

    // Get nonce
    config_payer.command = CliCommand::GetNonce(config_nonce.keypair.pubkey());
    let third_nonce_string = process_command(&config_payer).unwrap();
    let third_nonce = third_nonce_string.parse::<Hash>().unwrap();

    assert_ne!(first_nonce, third_nonce);

    // Withdraw from nonce account
    let payee_pubkey = Pubkey::new_rand();
    config_payer.command = CliCommand::WithdrawFromNonceAccount {
        nonce_account: read_keypair_file(&nonce_keypair_file).unwrap().pubkey(),
        nonce_authority: read_keypair_file(&authority_keypair_file).unwrap().into(),
        destination_account_pubkey: payee_pubkey,
        lamports: 100,
    };
    process_command(&config_payer).unwrap();
    check_balance(1000, &rpc_client, &config_payer.keypair.pubkey());
    check_balance(900, &rpc_client, &config_nonce.keypair.pubkey());
    check_balance(100, &rpc_client, &payee_pubkey);

    // Show nonce account
    config_payer.command = CliCommand::ShowNonceAccount {
        nonce_account_pubkey: config_nonce.keypair.pubkey(),
        use_lamports_unit: true,
    };
    process_command(&config_payer).unwrap();
}
