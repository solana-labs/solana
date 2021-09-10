use log::trace;
use solana_client::{
    client_error::{ClientError, ClientErrorKind, Result as ClientResult},
    rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient},
    rpc_config::{
        RpcBlockConfig, RpcBlockProductionConfig, RpcBlockProductionConfigRange,
        RpcGetVoteAccountsConfig, RpcLargestAccountsConfig, RpcLargestAccountsFilter,
        RpcLeaderScheduleConfig, RpcSendTransactionConfig, RpcSimulateTransactionConfig,
        RpcTransactionConfig,
    },
    rpc_custom_error::{
        JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE,
        JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE,
    },
    rpc_request::RpcError,
    rpc_response::StakeActivationState,
};
use solana_core::test_validator::{TestValidator, TestValidatorGenesis};
use solana_rpc::rpc::JsonRpcConfig;
use solana_sdk::stake;
use solana_sdk::stake::state::{Authorized, Lockup};
use solana_sdk::transaction::Transaction;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    fee_calculator::FeeRateGovernor,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    system_transaction,
    sysvar::rent::Rent,
};
use solana_streamer::socket::SocketAddrSpace;
use solana_transaction_status::{
    TransactionConfirmationStatus, TransactionDetails, UiTransactionEncoding,
};
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

#[test]
fn confirm_transaction() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_transaction(&tx)?;

    for _ in 0..1000 {
        let confirmed = rpc_client.confirm_transaction(&signature)?;
        if confirmed {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    panic!();
}

#[test]
fn confirm_transaction_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;

    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_transaction(&tx)?;
    let commitment_config = CommitmentConfig::confirmed();

    for _ in 0..1000 {
        let confirmed =
            rpc_client.confirm_transaction_with_commitment(&signature, commitment_config)?;
        if confirmed.value {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    panic!();
}

#[test]
fn get_signature_statuses() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_transaction(&tx)?;

    let mut state = None;

    // Test that the status of the transaction progresses
    // from none, to processed, to confirmations, to finalized.
    for _ in 0..1000 {
        let statuses = rpc_client.get_signature_statuses(&[signature])?;
        let status = statuses.value[0].as_ref();
        if let Some(status) = status {
            let cstatus = status.confirmation_status.clone().unwrap();
            match (state, cstatus) {
                (
                    None | Some(TransactionConfirmationStatus::Processed),
                    TransactionConfirmationStatus::Processed,
                ) => {
                    state = Some(TransactionConfirmationStatus::Processed);
                    trace!("processed");
                }
                (
                    None
                    | Some(TransactionConfirmationStatus::Processed)
                    | Some(TransactionConfirmationStatus::Confirmed),
                    TransactionConfirmationStatus::Confirmed,
                ) => {
                    state = Some(TransactionConfirmationStatus::Confirmed);
                    trace!("confirmed");
                }
                (
                    None
                    | Some(TransactionConfirmationStatus::Processed)
                    | Some(TransactionConfirmationStatus::Confirmed),
                    TransactionConfirmationStatus::Finalized,
                ) => {
                    assert!(status.confirmations.is_none());
                    trace!("finalized");
                    return Ok(());
                }
                _ => {
                    panic!("unexpected transaction status");
                }
            }
        } else {
            assert_eq!(state, None);
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    panic!("tx not finalized in time");
}

#[test]
fn get_signature_statuses_2() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;

    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_transaction(&tx)?;

    let status = loop {
        let statuses = rpc_client.get_signature_statuses(&[signature])?.value;
        if let Some(status) = statuses[0].clone() {
            break status;
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    assert!(status.err.is_none());

    Ok(())
}

#[test]
fn get_signature_statuses_with_history() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;

    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_and_confirm_transaction(&tx)?;

    let statuses = rpc_client
        .get_signature_statuses_with_history(&[signature])?
        .value;
    assert!(statuses.len() > 0);

    Ok(())
}

#[test]
fn get_signature_status_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_and_confirm_transaction(&tx)?;
    let commitment_config = CommitmentConfig::processed();

    let status = rpc_client.get_signature_status_with_commitment(&signature, commitment_config)?;
    assert!(status.is_some());

    Ok(())
}

#[test]
fn get_signature_status_with_commitment_and_history() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_and_confirm_transaction(&tx)?;
    let commitment_config = CommitmentConfig::processed();
    let search_transaction_history = true;

    let status = rpc_client.get_signature_status_with_commitment_and_history(
        &signature,
        commitment_config,
        search_transaction_history,
    )?;
    assert!(status.is_some());

    Ok(())
}

#[test]
fn simulate_transaction() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;

    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);

    let result = rpc_client.simulate_transaction(&tx)?;

    assert!(result.value.err.is_none());
    Ok(())
}

#[test]
fn simulate_transaction_with_config() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;

    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let config = RpcSimulateTransactionConfig {
        sig_verify: false,
        ..RpcSimulateTransactionConfig::default()
    };

    let result = rpc_client.simulate_transaction_with_config(&tx, config)?;
    assert!(result.value.err.is_none());

    Ok(())
}

#[test]
fn get_slot() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let slot = rpc_client.get_slot()?;
    assert!(slot < 10);

    Ok(())
}

#[test]
fn get_slot_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let commitment_config = CommitmentConfig::processed();
    let slot = rpc_client.get_slot_with_commitment(commitment_config)?;
    assert!(slot < 10);

    Ok(())
}

#[test]
fn get_block_height_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());
    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;
    let commitment_config = CommitmentConfig::processed();
    let block_height = rpc_client.get_block_height_with_commitment(commitment_config)?;
    assert!(block_height > 0);

    Ok(())
}

#[test]
fn get_block_production() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let result = rpc_client.get_block_production()?;
    assert!(!result.value.by_identity.is_empty());

    Ok(())
}

#[test]
fn get_block_production_with_config() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let start_slot = 1;
    let limit = 3;
    let leader = rpc_client.get_slot_leaders(start_slot, limit)?;
    let leader = leader[0];

    let range = RpcBlockProductionConfigRange {
        first_slot: 0,
        last_slot: Some(0),
    };

    let config = RpcBlockProductionConfig {
        identity: Some(leader.to_string()),
        range: Some(range),
        commitment: Some(CommitmentConfig::processed()),
    };

    let result = rpc_client.get_block_production_with_config(config)?;
    assert!(!result.value.by_identity.is_empty());

    Ok(())
}

#[test]
fn get_stake_activation() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    // Find some vote account to delegate to
    let vote_accounts = rpc_client.get_vote_accounts()?;
    let vote_account = vote_accounts
        .current
        .get(0)
        .unwrap_or_else(|| &vote_accounts.delinquent[0]);
    let vote_account_pubkey = &vote_account.vote_pubkey;
    let vote_account_pubkey = Pubkey::from_str(vote_account_pubkey).expect("pubkey");

    // Create a stake account
    let stake_account = Keypair::new();
    let stake_account_pubkey = stake_account.pubkey();

    // Build the instructions to create new stake account,
    // funded by alice, and delegate to a validator's vote account.
    let instrs = stake::instruction::create_account_and_delegate_stake(
        &alice.pubkey(),
        &stake_account_pubkey,
        &vote_account_pubkey,
        &Authorized::auto(&stake_account_pubkey),
        &Lockup::default(),
        1_000_000,
    );

    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = Transaction::new_signed_with_payer(
        &instrs,
        Some(&alice.pubkey()),
        &[&alice, &stake_account],
        latest_blockhash,
    );

    rpc_client.send_and_confirm_transaction(&tx)?;

    let epoch_info = rpc_client.get_epoch_info()?;
    let stake_activation =
        rpc_client.get_stake_activation(stake_account_pubkey, Some(epoch_info.epoch))?;

    assert_ne!(stake_activation.state, StakeActivationState::Inactive);
    assert_ne!(stake_activation.state, StakeActivationState::Deactivating);
    assert!(stake_activation.active > 0 || stake_activation.inactive > 0);

    Ok(())
}

#[test]
fn get_supply() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let supply = rpc_client.supply()?;
    assert!(supply.value.total != 0);

    Ok(())
}

#[test]
fn get_supply_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let commitment_config = CommitmentConfig::processed();
    let supply = rpc_client.supply_with_commitment(commitment_config)?;
    assert!(supply.value.total != 0);

    Ok(())
}

#[test]
fn get_largest_accounts_with_config() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let commitment_config = CommitmentConfig::processed();
    let config = RpcLargestAccountsConfig {
        commitment: Some(commitment_config),
        filter: Some(RpcLargestAccountsFilter::Circulating),
    };
    let accounts = rpc_client.get_largest_accounts_with_config(config)?;
    assert!(accounts.value.len() > 0);

    Ok(())
}

#[test]
fn get_vote_accounts_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let commitment_config = CommitmentConfig::processed();
    let accounts = rpc_client.get_vote_accounts_with_commitment(commitment_config)?;
    assert!(accounts.delinquent.len() + accounts.current.len() > 0);

    Ok(())
}

#[test]
fn get_vote_accounts_with_config() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let vote_account_pubkey = validator.vote_account_address();
    let commitment = CommitmentConfig::processed();
    let config = RpcGetVoteAccountsConfig {
        vote_pubkey: Some(vote_account_pubkey.to_string()),
        commitment: Some(commitment),
        keep_unstaked_delinquents: Some(true),
        delinquent_slot_distance: Some(10),
    };
    let accounts = rpc_client.get_vote_accounts_with_config(config)?;
    assert!(accounts.delinquent.len() + accounts.current.len() > 0);

    Ok(())
}

#[test]
fn get_cluster_nodes() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let cluster_nodes = rpc_client.get_cluster_nodes()?;
    assert!(cluster_nodes.len() > 0);

    Ok(())
}

#[test]
fn get_block() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;
    let slot = rpc_client.get_slot()?;
    let block = rpc_client.get_block(slot)?;
    assert!(block.block_time.is_some());

    Ok(())
}

#[test]
fn get_block_with_encoding() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;
    let slot = rpc_client.get_slot()?;

    let block = rpc_client.get_block_with_encoding(slot, UiTransactionEncoding::Base58)?;
    assert!(block.block_time.is_some());

    Ok(())
}

#[test]
fn get_block_with_config() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;
    let slot = rpc_client.get_slot()?;
    let config = RpcBlockConfig {
        encoding: Some(UiTransactionEncoding::Base58),
        transaction_details: Some(TransactionDetails::None),
        rewards: Some(true),
        commitment: None,
    };

    let block = rpc_client.get_block_with_config(slot, config)?;
    assert!(block.block_time.is_some());

    Ok(())
}

#[test]
fn get_blocks() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;
    let start_slot = 0;
    let end_slot = 3;

    let blocks = rpc_client.get_blocks(start_slot, Some(end_slot))?;
    assert!(blocks.len() > 0);

    Ok(())
}

#[test]
fn get_blocks_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;
    let end_slot = rpc_client.get_slot()?;
    let start_slot = 0;
    // Method does not support commitment below `confirmed`
    let commitment_config = CommitmentConfig::confirmed();

    let blocks =
        rpc_client.get_blocks_with_commitment(start_slot, Some(end_slot), commitment_config)?;
    assert!(blocks.len() > 0);

    Ok(())
}

#[test]
fn get_blocks_with_limit() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;
    let start_slot = 0;
    let limit = 3;

    let blocks = rpc_client.get_blocks_with_limit(start_slot, limit)?;
    assert!(blocks.len() > 0);

    Ok(())
}

#[test]
fn get_blocks_with_limit_and_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;
    let start_slot = 0;
    let limit = 3;
    let commitment_config = CommitmentConfig::confirmed();

    let blocks =
        rpc_client.get_blocks_with_limit_and_commitment(start_slot, limit, commitment_config)?;
    assert!(blocks.len() > 0);

    Ok(())
}

#[test]
fn get_signatures_for_address() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;
    let signatures = rpc_client.get_signatures_for_address(&alice.pubkey())?;
    assert!(signatures.len() > 0);

    Ok(())
}

#[test]
fn get_signatures_for_address_with_config() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;

    let config = GetConfirmedSignaturesForAddress2Config {
        before: None,
        until: None,
        limit: Some(3),
        commitment: Some(CommitmentConfig::confirmed()),
    };
    let signatures = rpc_client.get_signatures_for_address_with_config(&alice.pubkey(), config)?;
    assert!(signatures.len() > 0);

    Ok(())
}

#[test]
fn get_transaction() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_and_confirm_transaction(&tx)?;

    let transaction = rpc_client.get_transaction(&signature, UiTransactionEncoding::Json)?;
    assert!(transaction.block_time.is_some());

    Ok(())
}

#[test]
fn get_transaction_with_config() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_and_confirm_transaction(&tx)?;

    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Json),
        commitment: Some(CommitmentConfig::confirmed()),
    };

    let transaction = rpc_client.get_transaction_with_config(&signature, config)?;
    assert!(transaction.block_time.is_some());

    Ok(())
}

#[test]
fn get_block_time() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;

    let slot = rpc_client.get_slot()?;
    let block_time = rpc_client.get_block_time(slot)?;
    assert!(block_time != 0);

    Ok(())
}

#[test]
fn get_epoch_info() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;

    let epoch_info = rpc_client.get_epoch_info()?;
    assert!(epoch_info.block_height > 0);

    Ok(())
}

#[test]
fn get_epoch_info_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;

    let epoch_info = rpc_client.get_epoch_info_with_commitment(CommitmentConfig::confirmed())?;
    assert!(epoch_info.block_height > 0);

    Ok(())
}

#[test]
fn get_leader_schedule() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let slot = rpc_client.get_slot()?;
    let leader_schedule = rpc_client.get_leader_schedule(Some(slot))?;
    assert!(leader_schedule.is_some());

    Ok(())
}

#[test]
fn get_leader_schedule_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let slot = rpc_client.get_slot()?;
    let leader_schedule = rpc_client
        .get_leader_schedule_with_commitment(Some(slot), CommitmentConfig::processed())?;
    assert!(leader_schedule.is_some());

    Ok(())
}

#[test]
fn get_leader_schedule_with_config() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;

    let slot = rpc_client.get_slot()?;
    let config = RpcLeaderScheduleConfig {
        identity: Some(validator.vote_account_address().to_string()),
        commitment: Some(CommitmentConfig::confirmed()),
    };

    let leader_schedule = rpc_client.get_leader_schedule_with_config(Some(slot), config)?;
    assert!(leader_schedule.is_some());

    Ok(())
}

#[test]
fn get_epoch_schedule() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let epoch_schedule = rpc_client.get_epoch_schedule()?;
    assert!(epoch_schedule.slots_per_epoch > 0);

    Ok(())
}

#[test]
fn get_recent_performance_samples() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = create_test_validator_with_rpc_transaction_history_enabled(
        alice.pubkey(),
        None,
        SocketAddrSpace::Unspecified,
    );

    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    rpc_client.send_and_confirm_transaction(&tx)?;

    for _ in 0..1000 {
        let limit = Some(10);
        let performance_samples = rpc_client.get_recent_performance_samples(limit)?;
        if performance_samples.len() > 0 {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    panic!();
}

#[test]
fn get_identity() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);

    let rpc_client = RpcClient::new(validator.rpc_url());
    let identity = rpc_client.get_identity()?;
    assert!(identity.to_bytes().len() == 32);

    Ok(())
}

#[test]
fn get_inflation_governor() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);

    let rpc_client = RpcClient::new(validator.rpc_url());
    let inflation_governor = rpc_client.get_inflation_governor()?;
    assert!(inflation_governor.initial > 0.0);

    Ok(())
}

#[test]
fn get_inflation_rate() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);

    let rpc_client = RpcClient::new(validator.rpc_url());
    let inflation_rate = rpc_client.get_inflation_rate()?;
    assert!(inflation_rate.total > 0.0);

    Ok(())
}

#[test]
fn get_version() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let version = rpc_client.get_version()?;
    assert!(version.solana_core.len() > 0);

    Ok(())
}

#[test]
fn minimum_ledger_slot() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let slot = rpc_client.minimum_ledger_slot()?;
    assert!(slot == 0);

    Ok(())
}

#[test]
fn send_and_confirm_transaction() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;

    let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    let signature = rpc_client.send_and_confirm_transaction(&tx)?;
    assert!(tx.signatures.contains(&signature));

    Ok(())
}

#[test]
fn get_account() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let account = rpc_client.get_account(&alice.pubkey())?;
    assert!(account.lamports > 0);

    Ok(())
}

#[test]
fn get_account_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let account =
        rpc_client.get_account_with_commitment(&alice.pubkey(), CommitmentConfig::processed())?;
    assert!(account.value.is_some());

    Ok(())
}

#[test]
fn get_account_data() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let account_data = rpc_client.get_account_data(&alice.pubkey())?;
    assert!(account_data.len() == 0);

    Ok(())
}

#[test]
fn get_max_retransmit_slot() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let slot = rpc_client.get_max_retransmit_slot()?;
    assert!(slot == 0);

    Ok(())
}

#[test]
fn get_max_shred_insert_slot() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let slot = rpc_client.get_max_shred_insert_slot()?;
    assert!(slot == 0);

    Ok(())
}

#[test]
fn get_multiple_accounts() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let pubkeys = vec![alice.pubkey(), bob.pubkey()];
    let accounts = rpc_client.get_multiple_accounts(&pubkeys)?;
    assert!(accounts.len() == 2);

    Ok(())
}

#[test]
fn get_multiple_accounts_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let pubkeys = vec![alice.pubkey(), bob.pubkey()];
    let commitment_config = CommitmentConfig::processed();
    let accounts = rpc_client.get_multiple_accounts_with_commitment(&pubkeys, commitment_config)?;
    assert!(accounts.value.len() == 2);

    Ok(())
}

#[test]
fn get_minimum_balance_for_rent_exemption() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let balance = rpc_client.get_minimum_balance_for_rent_exemption(300)?;
    assert!(balance == 428);

    Ok(())
}

#[test]
fn get_balance() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let balance = rpc_client.get_balance(&alice.pubkey())?;
    assert!(balance == 500000000000000000);

    Ok(())
}

#[test]
fn get_balance_with_commitment() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let balance =
        rpc_client.get_balance_with_commitment(&alice.pubkey(), CommitmentConfig::processed())?;
    assert!(balance.value == 500000000000000000);

    Ok(())
}

#[test]
fn send_transaction_no_signatures() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let mut tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);

    tx.signatures[0] = Signature::default();

    let sig = rpc_client.send_transaction(&tx);

    match sig {
        Err(ClientError {
            kind:
                ClientErrorKind::RpcError(RpcError::RpcResponseError {
                    code: JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE,
                    ..
                }),
            ..
        }) => Ok(()),
        _ => panic!(),
    }
}

#[test]
fn send_transaction_with_config_no_signatures_no_preflight() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let mut tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);

    tx.signatures[0] = Signature::default();

    let config = RpcSendTransactionConfig {
        skip_preflight: true,
        ..RpcSendTransactionConfig::default()
    };

    let sig = rpc_client.send_transaction_with_config(&tx, config);

    match sig {
        Ok(_) => Ok(()),
        _ => panic!(),
    }
}

#[test]
fn send_transaction_bad_transaction() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let latest_blockhash = rpc_client.get_latest_blockhash()?;
    let tx = system_transaction::transfer(&bob, &bob.pubkey(), lamports, latest_blockhash);

    let sig = rpc_client.send_transaction(&tx);

    match sig {
        Err(ClientError {
            kind:
                ClientErrorKind::RpcError(RpcError::RpcResponseError {
                    code: JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE,
                    ..
                }),
            ..
        }) => Ok(()),
        _ => panic!(),
    }
}

fn create_test_validator_with_rpc_transaction_history_enabled(
    mint_address: Pubkey,
    faucet_addr: Option<SocketAddr>,
    socket_addr_space: SocketAddrSpace,
) -> TestValidator {
    TestValidatorGenesis::default()
        .rpc_config(JsonRpcConfig {
            enable_rpc_transaction_history: true,
            ..JsonRpcConfig::default()
        })
        .fee_rate_governor(FeeRateGovernor::new(0, 0))
        .rent(Rent {
            lamports_per_byte_year: 1,
            exemption_threshold: 1.0,
            ..Rent::default()
        })
        .faucet_addr(faucet_addr)
        .start_with_mint_address(mint_address, socket_addr_space)
        .expect("test validator start failed")
}
