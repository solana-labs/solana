use {
    solana_client::{
        nonblocking::tpu_client::TpuClient,
        send_and_confirm_transactions_in_parallel::{
            send_and_confirm_transactions_in_parallel_blocking, SendAndConfirmConfig,
        },
    },
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        commitment_config::CommitmentConfig, message::Message, native_token::sol_to_lamports,
        pubkey::Pubkey, signature::Keypair, signer::Signer, system_instruction,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
    std::sync::Arc,
};

const NUM_TRANSACTIONS: usize = 1000;

fn create_messages(from: Pubkey, to: Pubkey) -> (Vec<Message>, f64) {
    let mut messages = vec![];
    let mut sum = 0.0;
    for i in 1..NUM_TRANSACTIONS {
        let amount_to_transfer = i as f64;
        let ix = system_instruction::transfer(&from, &to, sol_to_lamports(amount_to_transfer));
        let message = Message::new(&[ix], Some(&from));
        messages.push(message);
        sum += amount_to_transfer;
    }
    (messages, sum)
}

#[test]
fn test_send_and_confirm_transactions_in_parallel_without_tpu_client() {
    solana_logger::setup();

    let alice = Keypair::new();
    let test_validator =
        TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);

    let bob_pubkey = solana_sdk::pubkey::new_rand();
    let alice_pubkey = alice.pubkey();

    let rpc_client = Arc::new(RpcClient::new(test_validator.rpc_url()));

    assert_eq!(
        rpc_client.get_version().unwrap().solana_core,
        solana_version::semver!()
    );

    let original_alice_balance = rpc_client.get_balance(&alice.pubkey()).unwrap();
    let (messages, sum) = create_messages(alice_pubkey, bob_pubkey);

    let txs_errors = send_and_confirm_transactions_in_parallel_blocking(
        rpc_client.clone(),
        None,
        &messages,
        &[&alice],
        SendAndConfirmConfig {
            with_spinner: false,
            resign_txs_count: Some(5),
        },
    );
    assert!(txs_errors.is_ok());
    assert!(txs_errors.unwrap().iter().all(|x| x.is_none()));

    assert_eq!(
        rpc_client
            .get_balance_with_commitment(&bob_pubkey, CommitmentConfig::processed())
            .unwrap()
            .value,
        sol_to_lamports(sum)
    );
    assert_eq!(
        rpc_client
            .get_balance_with_commitment(&alice_pubkey, CommitmentConfig::processed())
            .unwrap()
            .value,
        original_alice_balance - sol_to_lamports(sum)
    );
}

#[test]
fn test_send_and_confirm_transactions_in_parallel_with_tpu_client() {
    solana_logger::setup();

    let alice = Keypair::new();
    let test_validator =
        TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);

    let bob_pubkey = solana_sdk::pubkey::new_rand();
    let alice_pubkey = alice.pubkey();

    let rpc_client = Arc::new(RpcClient::new(test_validator.rpc_url()));

    assert_eq!(
        rpc_client.get_version().unwrap().solana_core,
        solana_version::semver!()
    );

    let original_alice_balance = rpc_client.get_balance(&alice.pubkey()).unwrap();
    let (messages, sum) = create_messages(alice_pubkey, bob_pubkey);
    let ws_url = test_validator.rpc_pubsub_url();
    let tpu_client_fut = TpuClient::new(
        "temp",
        rpc_client.get_inner_client().clone(),
        ws_url.as_str(),
        solana_client::tpu_client::TpuClientConfig::default(),
    );
    let tpu_client = rpc_client.runtime().block_on(tpu_client_fut).unwrap();

    let txs_errors = send_and_confirm_transactions_in_parallel_blocking(
        rpc_client.clone(),
        Some(tpu_client),
        &messages,
        &[&alice],
        SendAndConfirmConfig {
            with_spinner: false,
            resign_txs_count: Some(5),
        },
    );
    assert!(txs_errors.is_ok());
    assert!(txs_errors.unwrap().iter().all(|x| x.is_none()));

    assert_eq!(
        rpc_client
            .get_balance_with_commitment(&bob_pubkey, CommitmentConfig::processed())
            .unwrap()
            .value,
        sol_to_lamports(sum)
    );
    assert_eq!(
        rpc_client
            .get_balance_with_commitment(&alice_pubkey, CommitmentConfig::processed())
            .unwrap()
            .value,
        original_alice_balance - sol_to_lamports(sum)
    );
}
