use solana_cli::{
    cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig},
    nonce,
    offline::{
        blockhash_query::{self, BlockhashQuery},
        parse_sign_only_reply_string,
    },
};
use solana_client::rpc_client::RpcClient;
use solana_core::validator::{TestValidator, TestValidatorOptions};
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::{
    nonce::State as NonceState,
    pubkey::Pubkey,
    signature::{keypair_from_seed, Keypair, NullSigner, Signer},
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
fn test_transfer() {
    let TestValidator {
        server,
        leader_data,
        alice: mint_keypair,
        ledger_path,
        ..
    } = TestValidator::run_with_options(TestValidatorOptions {
        fees: 1,
        bootstrap_validator_lamports: 42_000,
        ..TestValidatorOptions::default()
    });

    let (sender, receiver) = channel();
    run_local_faucet(mint_keypair, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let default_signer = Keypair::new();
    let default_offline_signer = Keypair::new();

    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config.signers = vec![&default_signer];

    let sender_pubkey = config.signers[0].pubkey();
    let recipient_pubkey = Pubkey::new(&[1u8; 32]);

    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &sender_pubkey, 50_000).unwrap();
    check_balance(50_000, &rpc_client, &sender_pubkey);
    check_balance(0, &rpc_client, &recipient_pubkey);

    // Plain ole transfer
    config.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        no_wait: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    check_balance(49_989, &rpc_client, &sender_pubkey);
    check_balance(10, &rpc_client, &recipient_pubkey);

    let mut offline = CliConfig::default();
    offline.json_rpc_url = String::default();
    offline.signers = vec![&default_offline_signer];
    // Verify we cannot contact the cluster
    offline.command = CliCommand::ClusterVersion;
    process_command(&offline).unwrap_err();

    let offline_pubkey = offline.signers[0].pubkey();
    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_pubkey, 50).unwrap();
    check_balance(50, &rpc_client, &offline_pubkey);

    // Offline transfer
    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();
    offline.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: 0,
        sign_only: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sign_only_reply = process_command(&offline).unwrap();
    let sign_only = parse_sign_only_reply_string(&sign_only_reply);
    assert!(sign_only.has_all_signers());
    let offline_presigner = sign_only.presigner_of(&offline_pubkey).unwrap();
    config.signers = vec![&offline_presigner];
    config.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        no_wait: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    check_balance(39, &rpc_client, &offline_pubkey);
    check_balance(20, &rpc_client, &recipient_pubkey);

    // Create nonce account
    let nonce_account = keypair_from_seed(&[3u8; 32]).unwrap();
    let minimum_nonce_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(NonceState::size())
        .unwrap();
    config.signers = vec![&default_signer, &nonce_account];
    config.command = CliCommand::CreateNonceAccount {
        nonce_account: 1,
        seed: None,
        nonce_authority: None,
        lamports: minimum_nonce_balance,
    };
    process_command(&config).unwrap();
    check_balance(49_987 - minimum_nonce_balance, &rpc_client, &sender_pubkey);

    // Fetch nonce hash
    let nonce_hash = nonce::get_account(&rpc_client, &nonce_account.pubkey())
        .and_then(|ref a| nonce::data_from_account(a))
        .unwrap()
        .blockhash;

    // Nonced transfer
    config.signers = vec![&default_signer];
    config.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        no_wait: false,
        blockhash_query: BlockhashQuery::FeeCalculator(
            blockhash_query::Source::NonceAccount(nonce_account.pubkey()),
            nonce_hash,
        ),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    check_balance(49_976 - minimum_nonce_balance, &rpc_client, &sender_pubkey);
    check_balance(30, &rpc_client, &recipient_pubkey);
    let new_nonce_hash = nonce::get_account(&rpc_client, &nonce_account.pubkey())
        .and_then(|ref a| nonce::data_from_account(a))
        .unwrap()
        .blockhash;
    assert_ne!(nonce_hash, new_nonce_hash);

    // Assign nonce authority to offline
    config.signers = vec![&default_signer];
    config.command = CliCommand::AuthorizeNonceAccount {
        nonce_account: nonce_account.pubkey(),
        nonce_authority: 0,
        new_authority: offline_pubkey,
    };
    process_command(&config).unwrap();
    check_balance(49_975 - minimum_nonce_balance, &rpc_client, &sender_pubkey);

    // Fetch nonce hash
    let nonce_hash = nonce::get_account(&rpc_client, &nonce_account.pubkey())
        .and_then(|ref a| nonce::data_from_account(a))
        .unwrap()
        .blockhash;

    // Offline, nonced transfer
    offline.signers = vec![&default_offline_signer];
    offline.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: 0,
        sign_only: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::None(nonce_hash),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sign_only_reply = process_command(&offline).unwrap();
    let sign_only = parse_sign_only_reply_string(&sign_only_reply);
    assert!(sign_only.has_all_signers());
    let offline_presigner = sign_only.presigner_of(&offline_pubkey).unwrap();
    config.signers = vec![&offline_presigner];
    config.command = CliCommand::Transfer {
        lamports: 10,
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        no_wait: false,
        blockhash_query: BlockhashQuery::FeeCalculator(
            blockhash_query::Source::NonceAccount(nonce_account.pubkey()),
            sign_only.blockhash,
        ),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();
    check_balance(28, &rpc_client, &offline_pubkey);
    check_balance(40, &rpc_client, &recipient_pubkey);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_transfer_multisession_signing() {
    let TestValidator {
        server,
        leader_data,
        alice: mint_keypair,
        ledger_path,
        ..
    } = TestValidator::run_with_options(TestValidatorOptions {
        fees: 1,
        bootstrap_validator_lamports: 42_000,
        ..TestValidatorOptions::default()
    });

    let (sender, receiver) = channel();
    run_local_faucet(mint_keypair, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let to_pubkey = Pubkey::new(&[1u8; 32]);
    let offline_from_signer = keypair_from_seed(&[2u8; 32]).unwrap();
    let offline_fee_payer_signer = keypair_from_seed(&[3u8; 32]).unwrap();
    let from_null_signer = NullSigner::new(&offline_from_signer.pubkey());

    // Setup accounts
    let rpc_client = RpcClient::new_socket(leader_data.rpc);
    request_and_confirm_airdrop(&rpc_client, &faucet_addr, &offline_from_signer.pubkey(), 43)
        .unwrap();
    request_and_confirm_airdrop(
        &rpc_client,
        &faucet_addr,
        &offline_fee_payer_signer.pubkey(),
        3,
    )
    .unwrap();
    check_balance(43, &rpc_client, &offline_from_signer.pubkey());
    check_balance(3, &rpc_client, &offline_fee_payer_signer.pubkey());
    check_balance(0, &rpc_client, &to_pubkey);

    let (blockhash, _) = rpc_client.get_recent_blockhash().unwrap();

    // Offline fee-payer signs first
    let mut fee_payer_config = CliConfig::default();
    fee_payer_config.json_rpc_url = String::default();
    fee_payer_config.signers = vec![&offline_fee_payer_signer, &from_null_signer];
    // Verify we cannot contact the cluster
    fee_payer_config.command = CliCommand::ClusterVersion;
    process_command(&fee_payer_config).unwrap_err();
    fee_payer_config.command = CliCommand::Transfer {
        lamports: 42,
        to: to_pubkey,
        from: 1,
        sign_only: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sign_only_reply = process_command(&fee_payer_config).unwrap();
    let sign_only = parse_sign_only_reply_string(&sign_only_reply);
    assert!(!sign_only.has_all_signers());
    let fee_payer_presigner = sign_only
        .presigner_of(&offline_fee_payer_signer.pubkey())
        .unwrap();

    // Now the offline fund source
    let mut from_config = CliConfig::default();
    from_config.json_rpc_url = String::default();
    from_config.signers = vec![&fee_payer_presigner, &offline_from_signer];
    // Verify we cannot contact the cluster
    from_config.command = CliCommand::ClusterVersion;
    process_command(&from_config).unwrap_err();
    from_config.command = CliCommand::Transfer {
        lamports: 42,
        to: to_pubkey,
        from: 1,
        sign_only: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    let sign_only_reply = process_command(&from_config).unwrap();
    let sign_only = parse_sign_only_reply_string(&sign_only_reply);
    assert!(sign_only.has_all_signers());
    let from_presigner = sign_only
        .presigner_of(&offline_from_signer.pubkey())
        .unwrap();

    // Finally submit to the cluster
    let mut config = CliConfig::default();
    config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    config.signers = vec![&fee_payer_presigner, &from_presigner];
    config.command = CliCommand::Transfer {
        lamports: 42,
        to: to_pubkey,
        from: 1,
        sign_only: false,
        no_wait: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        nonce_account: None,
        nonce_authority: 0,
        fee_payer: 0,
    };
    process_command(&config).unwrap();

    check_balance(1, &rpc_client, &offline_from_signer.pubkey());
    check_balance(1, &rpc_client, &offline_fee_payer_signer.pubkey());
    check_balance(42, &rpc_client, &to_pubkey);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
