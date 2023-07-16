use {
    bincode::serialize,
    crossbeam_channel::unbounded,
    futures_util::StreamExt,
    log::*,
    reqwest::{self, header::CONTENT_TYPE},
    serde_json::{json, Value},
    solana_account_decoder::UiAccount,
    solana_client::{
        connection_cache::ConnectionCache,
        tpu_client::{TpuClient, TpuClientConfig},
    },
    solana_pubsub_client::nonblocking::pubsub_client::PubsubClient,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::{
        client_error::{ErrorKind as ClientErrorKind, Result as ClientResult},
        config::{RpcAccountInfoConfig, RpcSignatureSubscribeConfig},
        request::RpcError,
        response::{Response as RpcResponse, RpcSignatureResult, SlotUpdate},
    },
    solana_sdk::{
        commitment_config::CommitmentConfig,
        hash::Hash,
        pubkey::Pubkey,
        rent::Rent,
        signature::{Keypair, Signature, Signer},
        system_transaction,
        transaction::Transaction,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
    solana_tpu_client::tpu_client::DEFAULT_TPU_CONNECTION_POOL_SIZE,
    solana_transaction_status::TransactionStatus,
    std::{
        collections::HashSet,
        net::UdpSocket,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        thread::sleep,
        time::{Duration, Instant},
    },
    tokio::runtime::Runtime,
};

macro_rules! json_req {
    ($method: expr, $params: expr) => {{
        json!({
           "jsonrpc": "2.0",
           "id": 1,
           "method": $method,
           "params": $params,
        })
    }}
}

fn post_rpc(request: Value, rpc_url: &str) -> Value {
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(rpc_url)
        .header(CONTENT_TYPE, "application/json")
        .body(request.to_string())
        .send()
        .unwrap();
    serde_json::from_str(&response.text().unwrap()).unwrap()
}

#[test]
fn test_rpc_send_tx() {
    solana_logger::setup();

    let alice = Keypair::new();
    let test_validator =
        TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_url = test_validator.rpc_url();

    let bob_pubkey = solana_sdk::pubkey::new_rand();

    let req = json_req!("getRecentBlockhash", json!([]));
    let json = post_rpc(req, &rpc_url);

    let blockhash: Hash = json["result"]["value"]["blockhash"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    info!("blockhash: {:?}", blockhash);
    let tx = system_transaction::transfer(
        &alice,
        &bob_pubkey,
        Rent::default().minimum_balance(0),
        blockhash,
    );
    let serialized_encoded_tx = bs58::encode(serialize(&tx).unwrap()).into_string();

    let req = json_req!("sendTransaction", json!([serialized_encoded_tx]));
    let json: Value = post_rpc(req, &rpc_url);

    let signature = &json["result"];

    let mut confirmed_tx = false;

    let request = json_req!("getSignatureStatuses", [[signature]]);

    for _ in 0..solana_sdk::clock::DEFAULT_TICKS_PER_SLOT {
        let json = post_rpc(request.clone(), &rpc_url);

        let result: Option<TransactionStatus> =
            serde_json::from_value(json["result"]["value"][0].clone()).unwrap();
        if let Some(result) = result.as_ref() {
            if result.err.is_none() {
                confirmed_tx = true;
                break;
            }
        }

        sleep(Duration::from_millis(500));
    }

    assert!(confirmed_tx);

    use {
        solana_account_decoder::UiAccountEncoding,
        solana_rpc_client_api::config::RpcAccountInfoConfig,
    };
    let config = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        commitment: None,
        data_slice: None,
        min_context_slot: None,
    };
    let req = json_req!(
        "getAccountInfo",
        json!([bs58::encode(bob_pubkey).into_string(), config])
    );
    let json: Value = post_rpc(req, &rpc_url);
    info!("{:?}", json["result"]["value"]);
}

#[test]
fn test_rpc_invalid_requests() {
    solana_logger::setup();

    let alice = Keypair::new();
    let test_validator =
        TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_url = test_validator.rpc_url();

    let bob_pubkey = solana_sdk::pubkey::new_rand();

    // test invalid get_balance request
    let req = json_req!("getBalance", json!(["invalid9999"]));
    let json = post_rpc(req, &rpc_url);

    let the_error = json["error"]["message"].as_str().unwrap();
    assert_eq!(the_error, "Invalid param: Invalid");

    // test invalid get_account_info request
    let req = json_req!("getAccountInfo", json!(["invalid9999"]));
    let json = post_rpc(req, &rpc_url);

    let the_error = json["error"]["message"].as_str().unwrap();
    assert_eq!(the_error, "Invalid param: Invalid");

    // test invalid get_account_info request
    let req = json_req!("getAccountInfo", json!([bob_pubkey.to_string()]));
    let json = post_rpc(req, &rpc_url);

    let the_value = &json["result"]["value"];
    assert!(the_value.is_null());
}

#[test]
fn test_rpc_slot_updates() {
    solana_logger::setup();

    let test_validator =
        TestValidator::with_no_fees(Pubkey::new_unique(), None, SocketAddrSpace::Unspecified);

    // Track when slot updates are ready
    let (update_sender, update_receiver) = unbounded::<SlotUpdate>();
    // Create the pub sub runtime
    let rt = Runtime::new().unwrap();
    let rpc_pubsub_url = test_validator.rpc_pubsub_url();

    rt.spawn(async move {
        let pubsub_client = PubsubClient::new(&rpc_pubsub_url).await.unwrap();
        let (mut slot_notifications, slot_unsubscribe) =
            pubsub_client.slot_updates_subscribe().await.unwrap();

        while let Some(slot_update) = slot_notifications.next().await {
            update_sender.send(slot_update).unwrap();
        }
        slot_unsubscribe().await;
    });

    let first_update = update_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    // Verify that updates are received in order for an upcoming slot
    let verify_slot = first_update.slot() + 2;
    let expected_updates = vec![
        "CreatedBank",
        "Frozen",
        "OptimisticConfirmation",
        "Root", // TODO: debug why root signal is sent twice.
        "Root",
    ];
    let mut expected_updates = expected_updates.into_iter().peekable();
    // SlotUpdate::Completed is sent asynchronous to banking-stage and replay
    // when shreds are inserted into blockstore. When the leader generates
    // blocks, replay may freeze the bank before shreds are all inserted into
    // blockstore; and so SlotUpdate::Completed may be received _after_
    // SlotUpdate::Frozen.
    let mut slot_update_completed = false;

    let test_start = Instant::now();
    while expected_updates.peek().is_some() || !slot_update_completed {
        assert!(test_start.elapsed() < Duration::from_secs(30));
        let update = update_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        if update.slot() == verify_slot {
            let update_name = match update {
                SlotUpdate::CreatedBank { .. } => "CreatedBank",
                SlotUpdate::Completed { .. } => {
                    slot_update_completed = true;
                    continue;
                }
                SlotUpdate::Frozen { .. } => "Frozen",
                SlotUpdate::OptimisticConfirmation { .. } => "OptimisticConfirmation",
                SlotUpdate::Root { .. } => "Root",
                _ => continue,
            };
            assert_eq!(Some(update_name), expected_updates.next());
        }
    }
}

#[test]
fn test_rpc_subscriptions() {
    solana_logger::setup();

    let alice = Keypair::new();
    let test_validator =
        TestValidator::with_no_fees_udp(alice.pubkey(), None, SocketAddrSpace::Unspecified);

    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    transactions_socket.connect(test_validator.tpu()).unwrap();

    let rpc_client = RpcClient::new(test_validator.rpc_url());
    let recent_blockhash = rpc_client.get_latest_blockhash().unwrap();

    // Create transaction signatures to subscribe to
    let transfer_amount = Rent::default().minimum_balance(0);
    let transactions: Vec<Transaction> = (0..1000)
        .map(|_| {
            system_transaction::transfer(
                &alice,
                &solana_sdk::pubkey::new_rand(),
                transfer_amount,
                recent_blockhash,
            )
        })
        .collect();
    let mut signature_set: HashSet<Signature> =
        transactions.iter().map(|tx| tx.signatures[0]).collect();
    let mut account_set: HashSet<Pubkey> = transactions
        .iter()
        .map(|tx| tx.message.account_keys[1])
        .collect();

    // Track account notifications are received
    let (account_sender, account_receiver) = unbounded::<(Pubkey, RpcResponse<UiAccount>)>();
    // Track when status notifications are received
    let (status_sender, status_receiver) =
        unbounded::<(Signature, RpcResponse<RpcSignatureResult>)>();

    // Create the pub sub runtime
    let rt = Runtime::new().unwrap();
    let rpc_pubsub_url = test_validator.rpc_pubsub_url();
    let signature_set_clone = signature_set.clone();
    let account_set_clone = account_set.clone();
    let signature_subscription_ready = Arc::new(AtomicUsize::new(0));
    let account_subscription_ready = Arc::new(AtomicUsize::new(0));
    let signature_subscription_ready_clone = signature_subscription_ready.clone();
    let account_subscription_ready_clone = account_subscription_ready.clone();

    rt.spawn(async move {
        let pubsub_client = Arc::new(PubsubClient::new(&rpc_pubsub_url).await.unwrap());

        // Subscribe to signature notifications
        for signature in signature_set_clone {
            let status_sender = status_sender.clone();
            let signature_subscription_ready_clone = signature_subscription_ready_clone.clone();
            tokio::spawn({
                let pubsub_client = Arc::clone(&pubsub_client);
                async move {
                    let (mut sig_notifications, sig_unsubscribe) = pubsub_client
                        .signature_subscribe(
                            &signature,
                            Some(RpcSignatureSubscribeConfig {
                                commitment: Some(CommitmentConfig::confirmed()),
                                ..RpcSignatureSubscribeConfig::default()
                            }),
                        )
                        .await
                        .unwrap();

                    signature_subscription_ready_clone.fetch_add(1, Ordering::SeqCst);

                    let response = sig_notifications.next().await.unwrap();
                    status_sender.send((signature, response)).unwrap();
                    sig_unsubscribe().await;
                }
            });
        }

        // Subscribe to account notifications
        for pubkey in account_set_clone {
            let account_sender = account_sender.clone();
            let account_subscription_ready_clone = account_subscription_ready_clone.clone();
            tokio::spawn({
                let pubsub_client = Arc::clone(&pubsub_client);
                async move {
                    let (mut account_notifications, account_unsubscribe) = pubsub_client
                        .account_subscribe(
                            &pubkey,
                            Some(RpcAccountInfoConfig {
                                commitment: Some(CommitmentConfig::confirmed()),
                                ..RpcAccountInfoConfig::default()
                            }),
                        )
                        .await
                        .unwrap();

                    account_subscription_ready_clone.fetch_add(1, Ordering::SeqCst);

                    let response = account_notifications.next().await.unwrap();
                    account_sender.send((pubkey, response)).unwrap();
                    account_unsubscribe().await;
                }
            });
        }
    });

    let now = Instant::now();
    while (signature_subscription_ready.load(Ordering::SeqCst) != transactions.len()
        || account_subscription_ready.load(Ordering::SeqCst) != transactions.len())
        && now.elapsed() < Duration::from_secs(15)
    {
        sleep(Duration::from_millis(100))
    }

    // check signature subscription
    let num = signature_subscription_ready.load(Ordering::SeqCst);
    if num != transactions.len() {
        error!(
            "signature subscription didn't setup properly, want: {}, got: {}",
            transactions.len(),
            num
        );
    }

    // check account subscription
    let num = account_subscription_ready.load(Ordering::SeqCst);
    if num != transactions.len() {
        error!(
            "account subscriptions didn't setup properly, want: {}, got: {}",
            transactions.len(),
            num
        );
    }

    let rpc_client = RpcClient::new(test_validator.rpc_url());
    let mut mint_balance = rpc_client
        .get_balance_with_commitment(&alice.pubkey(), CommitmentConfig::processed())
        .unwrap()
        .value;
    assert!(mint_balance >= transactions.len() as u64);

    // Send all transactions to tpu socket for processing
    transactions.iter().for_each(|tx| {
        transactions_socket
            .send(&bincode::serialize(&tx).unwrap())
            .unwrap();
    });

    // Track mint balance to know when transactions have completed
    let now = Instant::now();
    let expected_mint_balance = mint_balance - (transfer_amount * transactions.len() as u64);
    while mint_balance != expected_mint_balance && now.elapsed() < Duration::from_secs(15) {
        mint_balance = rpc_client
            .get_balance_with_commitment(&alice.pubkey(), CommitmentConfig::processed())
            .unwrap()
            .value;
        sleep(Duration::from_millis(100));
    }
    if mint_balance != expected_mint_balance {
        error!("mint-check timeout. mint_balance {:?}", mint_balance);
    }

    // Wait for all signature subscriptions
    /* Set a large 30-sec timeout here because the timing of the above tokio process is
     * highly non-deterministic.  The test was too flaky at 15-second timeout.  Debugging
     * show occasional multi-second delay which could come from multiple sources -- other
     * tokio tasks, tokio scheduler, OS scheduler.  The async nature makes it hard to
     * track down the origin of the delay.
     */
    let deadline = Instant::now() + Duration::from_secs(30);
    while !signature_set.is_empty() {
        let timeout = deadline.saturating_duration_since(Instant::now());
        match status_receiver.recv_timeout(timeout) {
            Ok((sig, result)) => {
                if let RpcSignatureResult::ProcessedSignature(result) = result.value {
                    assert!(result.err.is_none());
                    assert!(signature_set.remove(&sig));
                } else {
                    panic!("Unexpected result");
                }
            }
            Err(_err) => {
                panic!(
                    "recv_timeout, {}/{} signatures remaining",
                    signature_set.len(),
                    transactions.len()
                );
            }
        }
    }

    let deadline = Instant::now() + Duration::from_secs(60);
    while !account_set.is_empty() {
        let timeout = deadline.saturating_duration_since(Instant::now());
        match account_receiver.recv_timeout(timeout) {
            Ok((pubkey, result)) => {
                assert_eq!(result.value.lamports, Rent::default().minimum_balance(0));
                assert!(account_set.remove(&pubkey));
            }
            Err(_err) => {
                panic!(
                    "recv_timeout, {}/{} accounts remaining",
                    account_set.len(),
                    transactions.len()
                );
            }
        }
    }
}

fn run_tpu_send_transaction(tpu_use_quic: bool) {
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, None, SocketAddrSpace::Unspecified);
    let rpc_client = Arc::new(RpcClient::new_with_commitment(
        test_validator.rpc_url(),
        CommitmentConfig::processed(),
    ));
    let connection_cache = match tpu_use_quic {
        true => {
            ConnectionCache::new_quic("connection_cache_test", DEFAULT_TPU_CONNECTION_POOL_SIZE)
        }
        false => {
            ConnectionCache::with_udp("connection_cache_test", DEFAULT_TPU_CONNECTION_POOL_SIZE)
        }
    };
    let recent_blockhash = rpc_client.get_latest_blockhash().unwrap();
    let tx =
        system_transaction::transfer(&mint_keypair, &Pubkey::new_unique(), 42, recent_blockhash);
    let success = match connection_cache {
        ConnectionCache::Quic(cache) => TpuClient::new_with_connection_cache(
            rpc_client.clone(),
            &test_validator.rpc_pubsub_url(),
            TpuClientConfig::default(),
            cache,
        )
        .unwrap()
        .send_transaction(&tx),
        ConnectionCache::Udp(cache) => TpuClient::new_with_connection_cache(
            rpc_client.clone(),
            &test_validator.rpc_pubsub_url(),
            TpuClientConfig::default(),
            cache,
        )
        .unwrap()
        .send_transaction(&tx),
    };
    assert!(success);
    let timeout = Duration::from_secs(5);
    let now = Instant::now();
    let signatures = vec![tx.signatures[0]];
    loop {
        assert!(now.elapsed() < timeout);
        let statuses = rpc_client.get_signature_statuses(&signatures).unwrap();
        if statuses.value.get(0).is_some() {
            return;
        }
    }
}

#[test]
fn test_tpu_send_transaction() {
    run_tpu_send_transaction(/*tpu_use_quic*/ false)
}

#[test]
fn test_tpu_send_transaction_with_quic() {
    run_tpu_send_transaction(/*tpu_use_quic*/ true)
}

#[test]
fn deserialize_rpc_error() -> ClientResult<()> {
    solana_logger::setup();

    let alice = Keypair::new();
    let validator = TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);
    let rpc_client = RpcClient::new(validator.rpc_url());

    let bob = Keypair::new();
    let lamports = 50;
    let blockhash = rpc_client.get_latest_blockhash()?;
    let mut tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, blockhash);

    // This will cause an error
    tx.signatures.clear();

    let err = rpc_client.send_transaction(&tx);
    let err = err.unwrap_err();

    match err.kind {
        ClientErrorKind::RpcError(RpcError::RpcRequestError { .. }) => {
            // This is what used to happen
            panic!()
        }
        ClientErrorKind::RpcError(RpcError::RpcResponseError { .. }) => Ok(()),
        _ => {
            panic!()
        }
    }
}
