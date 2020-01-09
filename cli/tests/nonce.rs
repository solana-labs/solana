use solana_cli::cli::{
    process_command, request_and_confirm_airdrop, CliCommand, CliConfig, KeypairEq,
};
use solana_client::rpc_client::RpcClient;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{read_keypair_file, write_keypair, Keypair, KeypairUtil},
    system_instruction::create_address_with_seed,
    system_program,
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
        &faucet_addr,
        &mut config_payer,
        &mut config_nonce,
        &keypair_file,
        None,
        None,
    );

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_nonce_with_seed() {
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
        &faucet_addr,
        &mut config_payer,
        &mut config_nonce,
        &keypair_file,
        Some(String::from("seed")),
        None,
    );

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_nonce_with_authority() {
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
    let (nonce_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&config_nonce.keypair, tmp_file.as_file_mut()).unwrap();

    let nonce_authority = Keypair::new();
    let (authority_keypair_file, mut tmp_file2) = make_tmp_file();
    write_keypair(&nonce_authority, tmp_file2.as_file_mut()).unwrap();

    full_battery_tests(
        &rpc_client,
        &faucet_addr,
        &mut config_payer,
        &mut config_nonce,
        &nonce_keypair_file,
        None,
        Some(&authority_keypair_file),
    );

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

fn read_keypair_from_option(keypair_file: &Option<&str>) -> Option<KeypairEq> {
    keypair_file.map(|akf| read_keypair_file(&akf).unwrap().into())
}

fn full_battery_tests(
    rpc_client: &RpcClient,
    faucet_addr: &std::net::SocketAddr,
    config_payer: &mut CliConfig,
    config_nonce: &mut CliConfig,
    nonce_keypair_file: &str,
    seed: Option<String>,
    authority_keypair_file: Option<&str>,
) {
    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_payer.keypair.pubkey(),
        2000,
    )
    .unwrap();
    check_balance(2000, &rpc_client, &config_payer.keypair.pubkey());

    let nonce_account = if let Some(seed) = seed.as_ref() {
        create_address_with_seed(&config_nonce.keypair.pubkey(), seed, &system_program::id())
            .unwrap()
    } else {
        config_nonce.keypair.pubkey()
    };

    // Create nonce account
    config_payer.command = CliCommand::CreateNonceAccount {
        nonce_account: read_keypair_file(&nonce_keypair_file).unwrap().into(),
        seed,
        nonce_authority: read_keypair_from_option(&authority_keypair_file)
            .map(|na: KeypairEq| na.pubkey()),
        lamports: 1000,
    };

    process_command(&config_payer).unwrap();
    check_balance(1000, &rpc_client, &config_payer.keypair.pubkey());
    check_balance(1000, &rpc_client, &nonce_account);

    // Get nonce
    config_payer.command = CliCommand::GetNonce(nonce_account);
    let first_nonce_string = process_command(&config_payer).unwrap();
    let first_nonce = first_nonce_string.parse::<Hash>().unwrap();

    // Get nonce
    config_payer.command = CliCommand::GetNonce(nonce_account);
    let second_nonce_string = process_command(&config_payer).unwrap();
    let second_nonce = second_nonce_string.parse::<Hash>().unwrap();

    assert_eq!(first_nonce, second_nonce);

    // New nonce
    config_payer.command = CliCommand::NewNonce {
        nonce_account,
        nonce_authority: read_keypair_from_option(&authority_keypair_file),
    };
    process_command(&config_payer).unwrap();

    // Get nonce
    config_payer.command = CliCommand::GetNonce(nonce_account);
    let third_nonce_string = process_command(&config_payer).unwrap();
    let third_nonce = third_nonce_string.parse::<Hash>().unwrap();

    assert_ne!(first_nonce, third_nonce);

    // Withdraw from nonce account
    let payee_pubkey = Pubkey::new_rand();
    config_payer.command = CliCommand::WithdrawFromNonceAccount {
        nonce_account,
        nonce_authority: read_keypair_from_option(&authority_keypair_file),
        destination_account_pubkey: payee_pubkey,
        lamports: 100,
    };
    process_command(&config_payer).unwrap();
    check_balance(1000, &rpc_client, &config_payer.keypair.pubkey());
    check_balance(900, &rpc_client, &nonce_account);
    check_balance(100, &rpc_client, &payee_pubkey);

    // Show nonce account
    config_payer.command = CliCommand::ShowNonceAccount {
        nonce_account_pubkey: nonce_account,
        use_lamports_unit: true,
    };
    process_command(&config_payer).unwrap();

    // Set new authority
    let new_authority = Keypair::new();
    let (new_authority_keypair_file, mut tmp_file) = make_tmp_file();
    write_keypair(&new_authority, tmp_file.as_file_mut()).unwrap();
    config_payer.command = CliCommand::AuthorizeNonceAccount {
        nonce_account,
        nonce_authority: read_keypair_from_option(&authority_keypair_file),
        new_authority: read_keypair_file(&new_authority_keypair_file)
            .unwrap()
            .pubkey(),
    };
    process_command(&config_payer).unwrap();

    // Old authority fails now
    config_payer.command = CliCommand::NewNonce {
        nonce_account,
        nonce_authority: read_keypair_from_option(&authority_keypair_file),
    };
    process_command(&config_payer).unwrap_err();

    // New authority can advance nonce
    config_payer.command = CliCommand::NewNonce {
        nonce_account,
        nonce_authority: Some(
            read_keypair_file(&new_authority_keypair_file)
                .unwrap()
                .into(),
        ),
    };
    process_command(&config_payer).unwrap();

    // New authority can withdraw from nonce account
    config_payer.command = CliCommand::WithdrawFromNonceAccount {
        nonce_account,
        nonce_authority: Some(
            read_keypair_file(&new_authority_keypair_file)
                .unwrap()
                .into(),
        ),
        destination_account_pubkey: payee_pubkey,
        lamports: 100,
    };
    process_command(&config_payer).unwrap();
    check_balance(1000, &rpc_client, &config_payer.keypair.pubkey());
    check_balance(800, &rpc_client, &nonce_account);
    check_balance(200, &rpc_client, &payee_pubkey);
}
