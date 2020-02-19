use solana_cli::cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig};
use solana_client::rpc_client::RpcClient;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{keypair_from_seed, Keypair, Signer},
    system_instruction::create_address_with_seed,
    system_program,
};
use std::{fs::remove_dir_all, sync::mpsc::channel, thread::sleep, time::Duration};

#[cfg(test)]
use solana_core::validator::new_validator_for_tests;

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
    let json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    full_battery_tests(&rpc_client, &faucet_addr, json_rpc_url, None, false);

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
    let json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    full_battery_tests(
        &rpc_client,
        &faucet_addr,
        json_rpc_url,
        Some(String::from("seed")),
        false,
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
    let json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    full_battery_tests(&rpc_client, &faucet_addr, json_rpc_url, None, true);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

fn full_battery_tests(
    rpc_client: &RpcClient,
    faucet_addr: &std::net::SocketAddr,
    json_rpc_url: String,
    seed: Option<String>,
    use_nonce_authority: bool,
) {
    let mut config_payer = CliConfig::default();
    config_payer.json_rpc_url = json_rpc_url.clone();
    let payer = Keypair::new();
    config_payer.signers = vec![&payer];

    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &config_payer.signers[0].pubkey(),
        2000,
    )
    .unwrap();
    check_balance(2000, &rpc_client, &config_payer.signers[0].pubkey());

    let mut config_nonce = CliConfig::default();
    config_nonce.json_rpc_url = json_rpc_url;
    let nonce_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
    config_nonce.signers = vec![&nonce_keypair];

    let nonce_account = if let Some(seed) = seed.as_ref() {
        create_address_with_seed(
            &config_nonce.signers[0].pubkey(),
            seed,
            &system_program::id(),
        )
        .unwrap()
    } else {
        nonce_keypair.pubkey()
    };

    let nonce_authority = Keypair::new();
    let optional_authority = if use_nonce_authority {
        Some(nonce_authority.pubkey())
    } else {
        None
    };

    // Create nonce account
    config_payer.signers.push(&nonce_keypair);
    config_payer.command = CliCommand::CreateNonceAccount {
        nonce_account: 1,
        seed,
        nonce_authority: optional_authority,
        lamports: 1000,
    };

    process_command(&config_payer).unwrap();
    check_balance(1000, &rpc_client, &config_payer.signers[0].pubkey());
    check_balance(1000, &rpc_client, &nonce_account);

    // Get nonce
    config_payer.signers.pop();
    config_payer.command = CliCommand::GetNonce(nonce_account);
    let first_nonce_string = process_command(&config_payer).unwrap();
    let first_nonce = first_nonce_string.parse::<Hash>().unwrap();

    // Get nonce
    config_payer.command = CliCommand::GetNonce(nonce_account);
    let second_nonce_string = process_command(&config_payer).unwrap();
    let second_nonce = second_nonce_string.parse::<Hash>().unwrap();

    assert_eq!(first_nonce, second_nonce);

    let mut authorized_signers: Vec<&dyn Signer> = vec![&payer];
    let index = if use_nonce_authority {
        authorized_signers.push(&nonce_authority);
        1
    } else {
        0
    };

    // New nonce
    config_payer.signers = authorized_signers.clone();
    config_payer.command = CliCommand::NewNonce {
        nonce_account,
        nonce_authority: index,
    };
    process_command(&config_payer).unwrap();

    // Get nonce
    config_payer.signers = vec![&payer];
    config_payer.command = CliCommand::GetNonce(nonce_account);
    let third_nonce_string = process_command(&config_payer).unwrap();
    let third_nonce = third_nonce_string.parse::<Hash>().unwrap();

    assert_ne!(first_nonce, third_nonce);

    // Withdraw from nonce account
    let payee_pubkey = Pubkey::new_rand();
    config_payer.signers = authorized_signers;
    config_payer.command = CliCommand::WithdrawFromNonceAccount {
        nonce_account,
        nonce_authority: index,
        destination_account_pubkey: payee_pubkey,
        lamports: 100,
    };
    process_command(&config_payer).unwrap();
    check_balance(1000, &rpc_client, &config_payer.signers[0].pubkey());
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
    config_payer.command = CliCommand::AuthorizeNonceAccount {
        nonce_account,
        nonce_authority: index,
        new_authority: new_authority.pubkey(),
    };
    process_command(&config_payer).unwrap();

    // Old authority fails now
    config_payer.command = CliCommand::NewNonce {
        nonce_account,
        nonce_authority: index,
    };
    process_command(&config_payer).unwrap_err();

    // New authority can advance nonce
    config_payer.signers = vec![&payer, &new_authority];
    config_payer.command = CliCommand::NewNonce {
        nonce_account,
        nonce_authority: 1,
    };
    process_command(&config_payer).unwrap();

    // New authority can withdraw from nonce account
    config_payer.command = CliCommand::WithdrawFromNonceAccount {
        nonce_account,
        nonce_authority: 1,
        destination_account_pubkey: payee_pubkey,
        lamports: 100,
    };
    process_command(&config_payer).unwrap();
    check_balance(1000, &rpc_client, &config_payer.signers[0].pubkey());
    check_balance(800, &rpc_client, &nonce_account);
    check_balance(200, &rpc_client, &payee_pubkey);
}
