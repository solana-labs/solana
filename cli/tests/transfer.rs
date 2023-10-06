#![allow(clippy::arithmetic_side_effects)]
#![allow(clippy::redundant_closure)]
use {
    solana_cli::{
        check_balance,
        cli::{process_command, request_and_confirm_airdrop, CliCommand, CliConfig},
        spend_utils::SpendAmount,
        test_utils::check_ready,
    },
    solana_cli_output::{parse_sign_only_reply_string, OutputFormat},
    solana_faucet::faucet::run_local_faucet,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_nonce_utils::blockhash_query::{self, BlockhashQuery},
    solana_sdk::{
        commitment_config::CommitmentConfig,
        fee::FeeStructure,
        native_token::sol_to_lamports,
        nonce::State as NonceState,
        pubkey::Pubkey,
        signature::{keypair_from_seed, Keypair, NullSigner, Signer},
        stake,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
};

#[test]
fn test_transfer() {
    solana_logger::setup();
    let fee_one_sig = FeeStructure::default().get_max_fee(1, 0);
    let fee_two_sig = FeeStructure::default().get_max_fee(2, 0);
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator = TestValidator::with_custom_fees(
        mint_pubkey,
        1,
        Some(faucet_addr),
        SocketAddrSpace::Unspecified,
    );

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let default_signer = Keypair::new();
    let default_offline_signer = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&default_signer];

    let sender_pubkey = config.signers[0].pubkey();
    let recipient_pubkey = Pubkey::from([1u8; 32]);

    request_and_confirm_airdrop(&rpc_client, &config, &sender_pubkey, sol_to_lamports(5.0))
        .unwrap();
    check_balance!(sol_to_lamports(5.0), &rpc_client, &sender_pubkey);
    check_balance!(0, &rpc_client, &recipient_pubkey);

    check_ready(&rpc_client);

    // Plain ole transfer
    config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(1.0)),
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    check_balance!(
        sol_to_lamports(4.0) - fee_one_sig,
        &rpc_client,
        &sender_pubkey
    );
    check_balance!(sol_to_lamports(1.0), &rpc_client, &recipient_pubkey);

    // Plain ole transfer, failure due to InsufficientFundsForSpendAndFee
    config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(4.0)),
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    assert!(process_command(&config).is_err());
    check_balance!(
        sol_to_lamports(4.0) - fee_one_sig,
        &rpc_client,
        &sender_pubkey
    );
    check_balance!(sol_to_lamports(1.0), &rpc_client, &recipient_pubkey);

    let mut offline = CliConfig::recent_for_tests();
    offline.json_rpc_url = String::default();
    offline.signers = vec![&default_offline_signer];
    // Verify we cannot contact the cluster
    offline.command = CliCommand::ClusterVersion;
    process_command(&offline).unwrap_err();

    let offline_pubkey = offline.signers[0].pubkey();
    request_and_confirm_airdrop(&rpc_client, &offline, &offline_pubkey, sol_to_lamports(1.0))
        .unwrap();
    check_balance!(sol_to_lamports(1.0), &rpc_client, &offline_pubkey);

    // Offline transfer
    let blockhash = rpc_client.get_latest_blockhash().unwrap();
    offline.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(0.5)),
        to: recipient_pubkey,
        from: 0,
        sign_only: true,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    offline.output_format = OutputFormat::JsonCompact;
    let sign_only_reply = process_command(&offline).unwrap();
    let sign_only = parse_sign_only_reply_string(&sign_only_reply);
    assert!(sign_only.has_all_signers());
    let offline_presigner = sign_only.presigner_of(&offline_pubkey).unwrap();
    config.signers = vec![&offline_presigner];
    config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(0.5)),
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    check_balance!(
        sol_to_lamports(0.5) - fee_one_sig,
        &rpc_client,
        &offline_pubkey
    );
    check_balance!(sol_to_lamports(1.5), &rpc_client, &recipient_pubkey);

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
        memo: None,
        amount: SpendAmount::Some(minimum_nonce_balance),
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    check_balance!(
        sol_to_lamports(4.0) - fee_one_sig - fee_two_sig - minimum_nonce_balance,
        &rpc_client,
        &sender_pubkey,
    );

    // Fetch nonce hash
    let nonce_hash = solana_rpc_client_nonce_utils::get_account_with_commitment(
        &rpc_client,
        &nonce_account.pubkey(),
        CommitmentConfig::processed(),
    )
    .and_then(|ref a| solana_rpc_client_nonce_utils::data_from_account(a))
    .unwrap()
    .blockhash();

    // Nonced transfer
    config.signers = vec![&default_signer];
    config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(1.0)),
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::FeeCalculator(
            blockhash_query::Source::NonceAccount(nonce_account.pubkey()),
            nonce_hash,
        ),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    check_balance!(
        sol_to_lamports(3.0) - 2 * fee_one_sig - fee_two_sig - minimum_nonce_balance,
        &rpc_client,
        &sender_pubkey,
    );
    check_balance!(sol_to_lamports(2.5), &rpc_client, &recipient_pubkey);
    let new_nonce_hash = solana_rpc_client_nonce_utils::get_account_with_commitment(
        &rpc_client,
        &nonce_account.pubkey(),
        CommitmentConfig::processed(),
    )
    .and_then(|ref a| solana_rpc_client_nonce_utils::data_from_account(a))
    .unwrap()
    .blockhash();
    assert_ne!(nonce_hash, new_nonce_hash);

    // Assign nonce authority to offline
    config.signers = vec![&default_signer];
    config.command = CliCommand::AuthorizeNonceAccount {
        nonce_account: nonce_account.pubkey(),
        nonce_authority: 0,
        memo: None,
        new_authority: offline_pubkey,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    check_balance!(
        sol_to_lamports(3.0) - 3 * fee_one_sig - fee_two_sig - minimum_nonce_balance,
        &rpc_client,
        &sender_pubkey,
    );

    // Fetch nonce hash
    let nonce_hash = solana_rpc_client_nonce_utils::get_account_with_commitment(
        &rpc_client,
        &nonce_account.pubkey(),
        CommitmentConfig::processed(),
    )
    .and_then(|ref a| solana_rpc_client_nonce_utils::data_from_account(a))
    .unwrap()
    .blockhash();

    // Offline, nonced transfer
    offline.signers = vec![&default_offline_signer];
    offline.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(0.4)),
        to: recipient_pubkey,
        from: 0,
        sign_only: true,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::None(nonce_hash),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    let sign_only_reply = process_command(&offline).unwrap();
    let sign_only = parse_sign_only_reply_string(&sign_only_reply);
    assert!(sign_only.has_all_signers());
    let offline_presigner = sign_only.presigner_of(&offline_pubkey).unwrap();
    config.signers = vec![&offline_presigner];
    config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(0.4)),
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::FeeCalculator(
            blockhash_query::Source::NonceAccount(nonce_account.pubkey()),
            sign_only.blockhash,
        ),
        nonce_account: Some(nonce_account.pubkey()),
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    check_balance!(
        sol_to_lamports(0.1) - 2 * fee_one_sig,
        &rpc_client,
        &offline_pubkey
    );
    check_balance!(sol_to_lamports(2.9), &rpc_client, &recipient_pubkey);
}

#[test]
fn test_transfer_multisession_signing() {
    solana_logger::setup();
    let fee = FeeStructure::default().get_max_fee(2, 0);
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator = TestValidator::with_custom_fees(
        mint_pubkey,
        1,
        Some(faucet_addr),
        SocketAddrSpace::Unspecified,
    );

    let to_pubkey = Pubkey::from([1u8; 32]);
    let offline_from_signer = keypair_from_seed(&[2u8; 32]).unwrap();
    let offline_fee_payer_signer = keypair_from_seed(&[3u8; 32]).unwrap();
    let from_null_signer = NullSigner::new(&offline_from_signer.pubkey());

    // Setup accounts
    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());
    request_and_confirm_airdrop(
        &rpc_client,
        &CliConfig::recent_for_tests(),
        &offline_from_signer.pubkey(),
        sol_to_lamports(43.0),
    )
    .unwrap();
    request_and_confirm_airdrop(
        &rpc_client,
        &CliConfig::recent_for_tests(),
        &offline_fee_payer_signer.pubkey(),
        sol_to_lamports(1.0) + 2 * fee,
    )
    .unwrap();
    check_balance!(
        sol_to_lamports(43.0),
        &rpc_client,
        &offline_from_signer.pubkey(),
    );
    check_balance!(
        sol_to_lamports(1.0) + 2 * fee,
        &rpc_client,
        &offline_fee_payer_signer.pubkey(),
    );
    check_balance!(0, &rpc_client, &to_pubkey);

    check_ready(&rpc_client);

    let blockhash = rpc_client.get_latest_blockhash().unwrap();

    // Offline fee-payer signs first
    let mut fee_payer_config = CliConfig::recent_for_tests();
    fee_payer_config.json_rpc_url = String::default();
    fee_payer_config.signers = vec![&offline_fee_payer_signer, &from_null_signer];
    // Verify we cannot contact the cluster
    fee_payer_config.command = CliCommand::ClusterVersion;
    process_command(&fee_payer_config).unwrap_err();
    fee_payer_config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(42.0)),
        to: to_pubkey,
        from: 1,
        sign_only: true,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    fee_payer_config.output_format = OutputFormat::JsonCompact;
    let sign_only_reply = process_command(&fee_payer_config).unwrap();
    let sign_only = parse_sign_only_reply_string(&sign_only_reply);
    assert!(!sign_only.has_all_signers());
    let fee_payer_presigner = sign_only
        .presigner_of(&offline_fee_payer_signer.pubkey())
        .unwrap();

    // Now the offline fund source
    let mut from_config = CliConfig::recent_for_tests();
    from_config.json_rpc_url = String::default();
    from_config.signers = vec![&fee_payer_presigner, &offline_from_signer];
    // Verify we cannot contact the cluster
    from_config.command = CliCommand::ClusterVersion;
    process_command(&from_config).unwrap_err();
    from_config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(42.0)),
        to: to_pubkey,
        from: 1,
        sign_only: true,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::None(blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    from_config.output_format = OutputFormat::JsonCompact;
    let sign_only_reply = process_command(&from_config).unwrap();
    let sign_only = parse_sign_only_reply_string(&sign_only_reply);
    assert!(sign_only.has_all_signers());
    let from_presigner = sign_only
        .presigner_of(&offline_from_signer.pubkey())
        .unwrap();

    // Finally submit to the cluster
    let mut config = CliConfig::recent_for_tests();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&fee_payer_presigner, &from_presigner];
    config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(42.0)),
        to: to_pubkey,
        from: 1,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();

    check_balance!(
        sol_to_lamports(1.0),
        &rpc_client,
        &offline_from_signer.pubkey(),
    );
    check_balance!(
        sol_to_lamports(1.0) + fee,
        &rpc_client,
        &offline_fee_payer_signer.pubkey(),
    );
    check_balance!(sol_to_lamports(42.0), &rpc_client, &to_pubkey);
}

#[test]
fn test_transfer_all() {
    solana_logger::setup();
    let fee = FeeStructure::default().get_max_fee(1, 0);
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator = TestValidator::with_custom_fees(
        mint_pubkey,
        1,
        Some(faucet_addr),
        SocketAddrSpace::Unspecified,
    );

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let default_signer = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&default_signer];

    let sender_pubkey = config.signers[0].pubkey();
    let recipient_pubkey = Pubkey::from([1u8; 32]);

    request_and_confirm_airdrop(&rpc_client, &config, &sender_pubkey, 500_000).unwrap();
    check_balance!(500_000, &rpc_client, &sender_pubkey);
    check_balance!(0, &rpc_client, &recipient_pubkey);

    check_ready(&rpc_client);

    // Plain ole transfer
    config.command = CliCommand::Transfer {
        amount: SpendAmount::All,
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    check_balance!(0, &rpc_client, &sender_pubkey);
    check_balance!(500_000 - fee, &rpc_client, &recipient_pubkey);
}

#[test]
fn test_transfer_unfunded_recipient() {
    solana_logger::setup();
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator = TestValidator::with_custom_fees(
        mint_pubkey,
        1,
        Some(faucet_addr),
        SocketAddrSpace::Unspecified,
    );

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let default_signer = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&default_signer];

    let sender_pubkey = config.signers[0].pubkey();
    let recipient_pubkey = Pubkey::from([1u8; 32]);

    request_and_confirm_airdrop(&rpc_client, &config, &sender_pubkey, 50_000).unwrap();
    check_balance!(50_000, &rpc_client, &sender_pubkey);
    check_balance!(0, &rpc_client, &recipient_pubkey);

    check_ready(&rpc_client);

    // Plain ole transfer
    config.command = CliCommand::Transfer {
        amount: SpendAmount::All,
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: false,
        no_wait: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: None,
        derived_address_program_id: None,
        compute_unit_price: None,
    };

    // Expect failure due to unfunded recipient and the lack of the `allow_unfunded_recipient` flag
    process_command(&config).unwrap_err();
}

#[test]
fn test_transfer_with_seed() {
    solana_logger::setup();
    let fee = FeeStructure::default().get_max_fee(1, 0);
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator = TestValidator::with_custom_fees(
        mint_pubkey,
        1,
        Some(faucet_addr),
        SocketAddrSpace::Unspecified,
    );

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let default_signer = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&default_signer];

    let sender_pubkey = config.signers[0].pubkey();
    let recipient_pubkey = Pubkey::from([1u8; 32]);
    let derived_address_seed = "seed".to_string();
    let derived_address_program_id = stake::program::id();
    let derived_address = Pubkey::create_with_seed(
        &sender_pubkey,
        &derived_address_seed,
        &derived_address_program_id,
    )
    .unwrap();

    request_and_confirm_airdrop(&rpc_client, &config, &sender_pubkey, sol_to_lamports(1.0))
        .unwrap();
    request_and_confirm_airdrop(&rpc_client, &config, &derived_address, sol_to_lamports(5.0))
        .unwrap();
    check_balance!(sol_to_lamports(1.0), &rpc_client, &sender_pubkey);
    check_balance!(sol_to_lamports(5.0), &rpc_client, &derived_address);
    check_balance!(0, &rpc_client, &recipient_pubkey);

    check_ready(&rpc_client);

    // Transfer with seed
    config.command = CliCommand::Transfer {
        amount: SpendAmount::Some(sol_to_lamports(5.0)),
        to: recipient_pubkey,
        from: 0,
        sign_only: false,
        dump_transaction_message: false,
        allow_unfunded_recipient: true,
        no_wait: false,
        blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
        nonce_account: None,
        nonce_authority: 0,
        memo: None,
        fee_payer: 0,
        derived_address_seed: Some(derived_address_seed),
        derived_address_program_id: Some(derived_address_program_id),
        compute_unit_price: None,
    };
    process_command(&config).unwrap();
    check_balance!(sol_to_lamports(1.0) - fee, &rpc_client, &sender_pubkey);
    check_balance!(sol_to_lamports(5.0), &rpc_client, &recipient_pubkey);
    check_balance!(0, &rpc_client, &derived_address);
}
