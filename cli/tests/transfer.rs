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
    signature::{keypair_from_seed, read_keypair_file, write_keypair, KeypairUtil},
};
use std::fs::remove_dir_all;
use std::sync::mpsc::channel;

#[cfg(test)]
use solana_core::validator::new_validator_for_tests_ex;
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
fn test_transfer() {
    let (server, leader_data, mint_keypair, ledger_path, _) = new_validator_for_tests_ex(1, 42_000);

    let (sender, receiver) = channel();
    run_local_faucet(mint_keypair, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());

    let sender_pubkey = config.keypair.pubkey();
    let recipient_pubkey = Pubkey::new(&[1u8; 32]);
    println!("sender: {:?}", sender_pubkey);
    println!("recipient: {:?}", recipient_pubkey);

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &sender_pubkey, 50_000).unwrap();
    check_balance(50_000, &rpc_client, &sender_pubkey);
    check_balance(0, &rpc_client, &recipient_pubkey);

    // Plain ole transfer
    config.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: None,
        sign_only: false,
        signers: None,
        blockhash_query: BlockhashQuery::All,
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config).unwrap();
    check_balance(49_989, &rpc_client, &sender_pubkey);
    check_balance(10, &rpc_client, &recipient_pubkey);

    let mut offline = CliConfig::default();
    offline.json_rpc_url = String::default();
    // Verify we cannot contact the cluster
    offline.command = CliCommand::ClusterVersion;
    process_command(&offline).unwrap_err();

    let offline_pubkey = offline.keypair.pubkey();
    println!("offline: {:?}", offline_pubkey);
    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_pubkey, 50).unwrap();
    check_balance(50, &rpc_client, &offline_pubkey);

    // Offline transfer
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    offline.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: None,
        sign_only: true,
        signers: None,
        blockhash_query: BlockhashQuery::None(blockhash, FeeCalculator::default()),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: None,
    };
    let sign_only_reply = process_command(&offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sign_only_reply);
    config.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: Some(offline_pubkey.into()),
        sign_only: false,
        signers: Some(signers),
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: None,
        nonce_authority: None,
        fee_payer: Some(offline_pubkey.into()),
    };
    process_command(&config).unwrap();
    check_balance(39, &rpc_client, &offline_pubkey);
    check_balance(20, &rpc_client, &recipient_pubkey);

    // Create nonce account
    let nonce_account = keypair_from_seed(&[3u8; 32]).unwrap();
    let (nonce_account_file, mut tmp_file) = make_tmp_file();
    write_keypair(&nonce_account, tmp_file.as_file_mut()).unwrap();
    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(NonceState::size())
        .unwrap();
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: read_keypair_file(&nonce_account_file).unwrap().into(),
        seed: None,
        nonce_authority: None,
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();
    check_balance(49_987 - minimum_nonce_balance, &rpc_client, &sender_pubkey);

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };

    // Nonced transfer
    config.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: None,
        sign_only: false,
        signers: None,
        blockhash_query: BlockhashQuery::FeeCalculator(nonce_hash),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: None,
        fee_payer: None,
    };
    process_command(&config).unwrap();
    check_balance(49_976 - minimum_nonce_balance, &rpc_client, &sender_pubkey);
    check_balance(30, &rpc_client, &recipient_pubkey);
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let new_nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };
    assert_ne!(nonce_hash, new_nonce_hash);

    // Assign nonce authority to offline
    config.command = CliCommand::AuthorizeNonceAccount {
        nonce_account: nonce_account.pubkey(),
        nonce_authority: None,
        new_authority: offline_pubkey,
    };
    process_command(&config).unwrap();
    check_balance(49_975 - minimum_nonce_balance, &rpc_client, &sender_pubkey);

    // Fetch nonce hash
    let account = rpc_client.get_account(&nonce_account.pubkey()).unwrap();
    let nonce_state: NonceState = account.state().unwrap();
    let nonce_hash = match nonce_state {
        NonceState::Initialized(_meta, hash) => hash,
        _ => panic!("Nonce is not initialized"),
    };

    // Offline, nonced transfer
    offline.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: None,
        sign_only: true,
        signers: None,
        blockhash_query: BlockhashQuery::None(nonce_hash, FeeCalculator::default()),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: None,
        fee_payer: None,
    };
    let sign_only_reply = process_command(&offline).unwrap();
    let (blockhash, signers) = parse_sign_only_reply_string(&sign_only_reply);
    config.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: Some(offline_pubkey.into()),
        sign_only: false,
        signers: Some(signers),
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: Some(offline_pubkey.into()),
        fee_payer: Some(offline_pubkey.into()),
    };
    process_command(&config).unwrap();
    check_balance(28, &rpc_client, &offline_pubkey);
    check_balance(40, &rpc_client, &recipient_pubkey);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
