use chrono::prelude::*;
use serde_json::Value;
use solana_cli::cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig};
use solana_client::rpc_client::RpcClient;
use solana_drone::drone::run_local_drone;
use solana_sdk::{hash::Hash, pubkey::Pubkey, signature::KeypairUtil, signature::Signature};
use std::fs::remove_dir_all;
use std::str::FromStr;
use std::sync::mpsc::channel;

#[cfg(test)]
use solana_core::validator::new_validator_for_tests;
use std::thread::sleep;
use std::time::Duration;

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
fn test_cli_timestamp_tx() {
    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let bob_pubkey = Pubkey::new_rand();

    let (sender, receiver) = channel();
    run_local_drone(alice, sender, None);
    let drone_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_payer = CliConfig::default();
    config_payer.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_witness = CliConfig::default();
    config_witness.json_rpc_url = config_payer.json_rpc_url.clone();

    assert_ne!(
        config_payer.keypair.pubkey(),
        config_witness.keypair.pubkey()
    );

    request_and_confirm_airdrop(&rpc_client, &drone_addr, &config_payer.keypair.pubkey(), 50)
        .unwrap();
    check_balance(50, &rpc_client, &config_payer.keypair.pubkey());

    request_and_confirm_airdrop(
        &rpc_client,
        &drone_addr,
        &config_witness.keypair.pubkey(),
        1,
    )
    .unwrap();

    // Make transaction (from config_payer to bob_pubkey) requiring timestamp from config_witness
    let date_string = "\"2018-09-19T17:30:59Z\"";
    let dt: DateTime<Utc> = serde_json::from_str(&date_string).unwrap();
    config_payer.command = CliCommand::Pay {
        lamports: 10,
        to: bob_pubkey,
        timestamp: Some(dt),
        timestamp_pubkey: Some(config_witness.keypair.pubkey()),
        witnesses: None,
        cancelable: false,
        sign_only: false,
        signers: None,
        blockhash: None,
    };
    let sig_response = process_command(&config_payer);

    let object: Value = serde_json::from_str(&sig_response.unwrap()).unwrap();
    let process_id_str = object.get("processId").unwrap().as_str().unwrap();
    let process_id_vec = bs58::decode(process_id_str)
        .into_vec()
        .expect("base58-encoded public key");
    let process_id = Pubkey::new(&process_id_vec);

    check_balance(40, &rpc_client, &config_payer.keypair.pubkey()); // config_payer balance
    check_balance(10, &rpc_client, &process_id); // contract balance
    check_balance(0, &rpc_client, &bob_pubkey); // recipient balance

    // Sign transaction by config_witness
    config_witness.command = CliCommand::TimeElapsed(bob_pubkey, process_id, dt);
    process_command(&config_witness).unwrap();

    check_balance(40, &rpc_client, &config_payer.keypair.pubkey()); // config_payer balance
    check_balance(0, &rpc_client, &process_id); // contract balance
    check_balance(10, &rpc_client, &bob_pubkey); // recipient balance

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_cli_witness_tx() {
    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let bob_pubkey = Pubkey::new_rand();

    let (sender, receiver) = channel();
    run_local_drone(alice, sender, None);
    let drone_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_payer = CliConfig::default();
    config_payer.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_witness = CliConfig::default();
    config_witness.json_rpc_url = config_payer.json_rpc_url.clone();

    assert_ne!(
        config_payer.keypair.pubkey(),
        config_witness.keypair.pubkey()
    );

    request_and_confirm_airdrop(&rpc_client, &drone_addr, &config_payer.keypair.pubkey(), 50)
        .unwrap();
    request_and_confirm_airdrop(
        &rpc_client,
        &drone_addr,
        &config_witness.keypair.pubkey(),
        1,
    )
    .unwrap();

    // Make transaction (from config_payer to bob_pubkey) requiring witness signature from config_witness
    config_payer.command = CliCommand::Pay {
        lamports: 10,
        to: bob_pubkey,
        timestamp: None,
        timestamp_pubkey: None,
        witnesses: Some(vec![config_witness.keypair.pubkey()]),
        cancelable: false,
        sign_only: false,
        signers: None,
        blockhash: None,
    };
    let sig_response = process_command(&config_payer);

    let object: Value = serde_json::from_str(&sig_response.unwrap()).unwrap();
    let process_id_str = object.get("processId").unwrap().as_str().unwrap();
    let process_id_vec = bs58::decode(process_id_str)
        .into_vec()
        .expect("base58-encoded public key");
    let process_id = Pubkey::new(&process_id_vec);

    check_balance(40, &rpc_client, &config_payer.keypair.pubkey()); // config_payer balance
    check_balance(10, &rpc_client, &process_id); // contract balance
    check_balance(0, &rpc_client, &bob_pubkey); // recipient balance

    // Sign transaction by config_witness
    config_witness.command = CliCommand::Witness(bob_pubkey, process_id);
    process_command(&config_witness).unwrap();

    check_balance(40, &rpc_client, &config_payer.keypair.pubkey()); // config_payer balance
    check_balance(0, &rpc_client, &process_id); // contract balance
    check_balance(10, &rpc_client, &bob_pubkey); // recipient balance

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_cli_cancel_tx() {
    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let bob_pubkey = Pubkey::new_rand();

    let (sender, receiver) = channel();
    run_local_drone(alice, sender, None);
    let drone_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_payer = CliConfig::default();
    config_payer.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let mut config_witness = CliConfig::default();
    config_witness.json_rpc_url = config_payer.json_rpc_url.clone();

    assert_ne!(
        config_payer.keypair.pubkey(),
        config_witness.keypair.pubkey()
    );

    request_and_confirm_airdrop(&rpc_client, &drone_addr, &config_payer.keypair.pubkey(), 50)
        .unwrap();

    // Make transaction (from config_payer to bob_pubkey) requiring witness signature from config_witness
    config_payer.command = CliCommand::Pay {
        lamports: 10,
        to: bob_pubkey,
        timestamp: None,
        timestamp_pubkey: None,
        witnesses: Some(vec![config_witness.keypair.pubkey()]),
        cancelable: true,
        sign_only: false,
        signers: None,
        blockhash: None,
    };
    let sig_response = process_command(&config_payer).unwrap();

    let object: Value = serde_json::from_str(&sig_response).unwrap();
    let process_id_str = object.get("processId").unwrap().as_str().unwrap();
    let process_id_vec = bs58::decode(process_id_str)
        .into_vec()
        .expect("base58-encoded public key");
    let process_id = Pubkey::new(&process_id_vec);

    check_balance(40, &rpc_client, &config_payer.keypair.pubkey()); // config_payer balance
    check_balance(10, &rpc_client, &process_id); // contract balance
    check_balance(0, &rpc_client, &bob_pubkey); // recipient balance

    // Sign transaction by config_witness
    config_payer.command = CliCommand::Cancel(process_id);
    process_command(&config_payer).unwrap();

    check_balance(50, &rpc_client, &config_payer.keypair.pubkey()); // config_payer balance
    check_balance(0, &rpc_client, &process_id); // contract balance
    check_balance(0, &rpc_client, &bob_pubkey); // recipient balance

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_offline_pay_tx() {
    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let bob_pubkey = Pubkey::new_rand();

    let (sender, receiver) = channel();
    run_local_drone(alice, sender, None);
    let drone_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config_offline = CliConfig::default();
    config_offline.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    let mut config_online = CliConfig::default();
    config_online.json_rpc_url =
        format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    assert_ne!(
        config_offline.keypair.pubkey(),
        config_online.keypair.pubkey()
    );

    request_and_confirm_airdrop(
        &rpc_client,
        &drone_addr,
        &config_offline.keypair.pubkey(),
        50,
    )
    .unwrap();

    request_and_confirm_airdrop(
        &rpc_client,
        &drone_addr,
        &config_online.keypair.pubkey(),
        50,
    )
    .unwrap();
    check_balance(50, &rpc_client, &config_offline.keypair.pubkey());
    check_balance(50, &rpc_client, &config_online.keypair.pubkey());

    config_offline.command = CliCommand::Pay {
        lamports: 10,
        to: bob_pubkey,
        timestamp: None,
        timestamp_pubkey: None,
        witnesses: None,
        cancelable: false,
        sign_only: true,
        signers: None,
        blockhash: None,
    };
    let sig_response = process_command(&config_offline).unwrap();

    check_balance(50, &rpc_client, &config_offline.keypair.pubkey());
    check_balance(50, &rpc_client, &config_online.keypair.pubkey());
    check_balance(0, &rpc_client, &bob_pubkey);

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

    config_online.command = CliCommand::Pay {
        lamports: 10,
        to: bob_pubkey,
        timestamp: None,
        timestamp_pubkey: None,
        witnesses: None,
        cancelable: false,
        sign_only: false,
        signers: Some(signers),
        blockhash: Some(blockhash_str.parse::<Hash>().unwrap()),
    };
    process_command(&config_online).unwrap();

    check_balance(40, &rpc_client, &config_offline.keypair.pubkey());
    check_balance(50, &rpc_client, &config_online.keypair.pubkey());
    check_balance(10, &rpc_client, &bob_pubkey);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
