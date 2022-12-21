use {
    futures_util::StreamExt,
    serde_json::{json, Value},
    solana_ledger::{blockstore::Blockstore, get_tmp_ledger_path},
    solana_pubsub_client::{nonblocking, pubsub_client::PubsubClient},
    solana_rpc::{
        optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank,
        rpc::{create_test_transaction_entries, populate_blockstore_for_tests},
        rpc_pubsub_service::{PubSubConfig, PubSubService},
        rpc_subscriptions::RpcSubscriptions,
    },
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::{
        config::{
            RpcAccountInfoConfig, RpcBlockSubscribeConfig, RpcBlockSubscribeFilter,
            RpcProgramAccountsConfig,
        },
        response::SlotInfo,
    },
    solana_runtime::{
        bank::Bank,
        bank_forks::BankForks,
        commitment::{BlockCommitmentCache, CommitmentSlots},
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    },
    solana_sdk::{
        clock::Slot,
        commitment_config::{CommitmentConfig, CommitmentLevel},
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        rpc_port,
        signature::{Keypair, Signer},
        system_program, system_transaction,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
    solana_transaction_status::{
        BlockEncodingOptions, ConfirmedBlock, TransactionDetails, UiTransactionEncoding,
    },
    std::{
        collections::HashSet,
        net::{IpAddr, SocketAddr},
        sync::{
            atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
            Arc, RwLock,
        },
        thread::sleep,
        time::{Duration, Instant},
    },
    systemstat::Ipv4Addr,
    tungstenite::connect,
};

static NEXT_RPC_PUBSUB_PORT: AtomicU16 = AtomicU16::new(rpc_port::DEFAULT_RPC_PUBSUB_PORT);

fn pubsub_addr() -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        NEXT_RPC_PUBSUB_PORT.fetch_add(1, Ordering::Relaxed),
    )
}

#[test]
fn test_rpc_client() {
    solana_logger::setup();

    let alice = Keypair::new();
    let test_validator =
        TestValidator::with_no_fees(alice.pubkey(), None, SocketAddrSpace::Unspecified);

    let bob_pubkey = solana_sdk::pubkey::new_rand();

    let client = RpcClient::new(test_validator.rpc_url());

    assert_eq!(
        client.get_version().unwrap().solana_core,
        solana_version::semver!()
    );

    assert!(client.get_account(&bob_pubkey).is_err());

    assert_eq!(client.get_balance(&bob_pubkey).unwrap(), 0);

    let original_alice_balance = client.get_balance(&alice.pubkey()).unwrap();

    let blockhash = client.get_latest_blockhash().unwrap();

    let tx = system_transaction::transfer(&alice, &bob_pubkey, sol_to_lamports(20.0), blockhash);
    let signature = client.send_transaction(&tx).unwrap();

    let mut confirmed_tx = false;

    let now = Instant::now();
    while now.elapsed().as_secs() <= 20 {
        let response = client
            .confirm_transaction_with_commitment(&signature, CommitmentConfig::processed())
            .unwrap();

        if response.value {
            confirmed_tx = true;
            break;
        }

        sleep(Duration::from_millis(500));
    }

    assert!(confirmed_tx);

    assert_eq!(
        client
            .get_balance_with_commitment(&bob_pubkey, CommitmentConfig::processed())
            .unwrap()
            .value,
        sol_to_lamports(20.0)
    );
    assert_eq!(
        client
            .get_balance_with_commitment(&alice.pubkey(), CommitmentConfig::processed())
            .unwrap()
            .value,
        original_alice_balance - sol_to_lamports(20.0)
    );
}

#[test]
fn test_account_subscription() {
    let pubsub_addr = pubsub_addr();
    let exit = Arc::new(AtomicBool::new(false));

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair: alice,
        ..
    } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let blockhash = bank.last_blockhash();
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
    let bank0 = bank_forks.read().unwrap().get(0).unwrap();
    let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
    bank_forks.write().unwrap().insert(bank1);
    let bob = Keypair::new();
    let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
    let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
        &exit,
        max_complete_transaction_status_slot,
        bank_forks.clone(),
        Arc::new(RwLock::new(BlockCommitmentCache::default())),
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
    ));
    let (trigger, pubsub_service) =
        PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);

    check_server_is_ready_or_panic(&pubsub_addr, 10, Duration::from_millis(300));

    let config = Some(RpcAccountInfoConfig {
        commitment: Some(CommitmentConfig::finalized()),
        encoding: None,
        data_slice: None,
        min_context_slot: None,
    });
    let (mut client, receiver) = PubsubClient::account_subscribe(
        &format!("ws://0.0.0.0:{}/", pubsub_addr.port()),
        &bob.pubkey(),
        config,
    )
    .unwrap();

    // Transfer 100 lamports from alice to bob
    let tx = system_transaction::transfer(&alice, &bob.pubkey(), 100, blockhash);
    bank_forks
        .write()
        .unwrap()
        .get(1)
        .unwrap()
        .process_transaction(&tx)
        .unwrap();
    let commitment_slots = CommitmentSlots {
        slot: 1,
        ..CommitmentSlots::default()
    };
    subscriptions.notify_subscribers(commitment_slots);
    let commitment_slots = CommitmentSlots {
        slot: 2,
        root: 1,
        highest_confirmed_slot: 1,
        highest_confirmed_root: 1,
    };
    subscriptions.notify_subscribers(commitment_slots);

    let expected = json!({
    "context": { "slot": 1 },
        "value": {
            "owner": system_program::id().to_string(),
            "lamports": 100,
            "data": "",
            "executable": false,
            "rentEpoch": 0,
            "space": 0,
        },
    });

    // Read notification
    let mut errors: Vec<(Value, Value)> = Vec::new();
    let response = receiver.recv();
    match response {
        Ok(response) => {
            let actual = serde_json::to_value(response).unwrap();
            if expected != actual {
                errors.push((expected, actual));
            }
        }
        Err(_) => eprintln!("unexpected websocket receive timeout"),
    }

    exit.store(true, Ordering::Relaxed);
    trigger.cancel();
    client.shutdown().unwrap();
    pubsub_service.close().unwrap();
    assert_eq!(errors, [].to_vec());
}

#[test]
fn test_block_subscription() {
    // setup BankForks
    let exit = Arc::new(AtomicBool::new(false));
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair: alice,
        ..
    } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let rent_exempt_amount = bank.get_minimum_balance_for_rent_exemption(0);
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

    // setup Blockstore
    let ledger_path = get_tmp_ledger_path!();
    let blockstore = Blockstore::open(&ledger_path).unwrap();
    let blockstore = Arc::new(blockstore);

    // populate ledger with test txs
    let bank = bank_forks.read().unwrap().working_bank();
    let keypair1 = Keypair::new();
    let keypair2 = Keypair::new();
    let keypair3 = Keypair::new();
    let max_complete_transaction_status_slot = Arc::new(AtomicU64::new(blockstore.max_root()));
    bank.transfer(rent_exempt_amount, &alice, &keypair2.pubkey())
        .unwrap();
    populate_blockstore_for_tests(
        create_test_transaction_entries(
            vec![&alice, &keypair1, &keypair2, &keypair3],
            bank.clone(),
        )
        .0,
        bank,
        blockstore.clone(),
        max_complete_transaction_status_slot,
    );
    let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
    // setup RpcSubscriptions && PubSubService
    let subscriptions = Arc::new(RpcSubscriptions::new_for_tests_with_blockstore(
        &exit,
        max_complete_transaction_status_slot,
        blockstore.clone(),
        bank_forks.clone(),
        Arc::new(RwLock::new(BlockCommitmentCache::default())),
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
    ));
    let pubsub_addr = pubsub_addr();
    let pub_cfg = PubSubConfig {
        enable_block_subscription: true,
        ..PubSubConfig::default()
    };
    let (trigger, pubsub_service) = PubSubService::new(pub_cfg, &subscriptions, pubsub_addr);

    check_server_is_ready_or_panic(&pubsub_addr, 10, Duration::from_millis(300));

    // setup PubsubClient
    let (mut client, receiver) = PubsubClient::block_subscribe(
        &format!("ws://0.0.0.0:{}/", pubsub_addr.port()),
        RpcBlockSubscribeFilter::All,
        Some(RpcBlockSubscribeConfig {
            commitment: Some(CommitmentConfig {
                commitment: CommitmentLevel::Confirmed,
            }),
            encoding: Some(UiTransactionEncoding::Json),
            transaction_details: Some(TransactionDetails::Signatures),
            show_rewards: None,
            max_supported_transaction_version: None,
        }),
    )
    .unwrap();

    // trigger Gossip notification
    let slot = bank_forks.read().unwrap().highest_slot();
    subscriptions.notify_gossip_subscribers(slot);
    let maybe_actual = receiver.recv_timeout(Duration::from_millis(400));
    match maybe_actual {
        Ok(actual) => {
            let versioned_block = blockstore.get_complete_block(slot, false).unwrap();
            let confirmed_block = ConfirmedBlock::from(versioned_block);
            let block = confirmed_block
                .encode_with_options(
                    UiTransactionEncoding::Json,
                    BlockEncodingOptions {
                        transaction_details: TransactionDetails::Signatures,
                        show_rewards: false,
                        max_supported_transaction_version: None,
                    },
                )
                .unwrap();
            assert_eq!(actual.value.slot, slot);
            assert!(block.eq(&actual.value.block.unwrap()));
        }
        Err(e) => {
            eprintln!("unexpected websocket receive timeout");
            assert_eq!(Some(e), None);
        }
    }

    // cleanup
    exit.store(true, Ordering::Relaxed);
    trigger.cancel();
    client.shutdown().unwrap();
    pubsub_service.close().unwrap();
}

#[test]
fn test_program_subscription() {
    let pubsub_addr = pubsub_addr();
    let exit = Arc::new(AtomicBool::new(false));

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair: alice,
        ..
    } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let blockhash = bank.last_blockhash();
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
    let bank0 = bank_forks.read().unwrap().get(0).unwrap();
    let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
    bank_forks.write().unwrap().insert(bank1);
    let bob = Keypair::new();
    let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
    let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
        &exit,
        max_complete_transaction_status_slot,
        bank_forks.clone(),
        Arc::new(RwLock::new(BlockCommitmentCache::default())),
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
    ));
    let (trigger, pubsub_service) =
        PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);

    check_server_is_ready_or_panic(&pubsub_addr, 10, Duration::from_millis(300));

    let config = Some(RpcProgramAccountsConfig {
        ..RpcProgramAccountsConfig::default()
    });

    let program_id = Pubkey::new_unique();
    let (mut client, receiver) = PubsubClient::program_subscribe(
        &format!("ws://0.0.0.0:{}/", pubsub_addr.port()),
        &program_id,
        config,
    )
    .unwrap();

    // Create new program account at bob's address
    let tx = system_transaction::create_account(&alice, &bob, blockhash, 100, 0, &program_id);
    bank_forks
        .write()
        .unwrap()
        .get(1)
        .unwrap()
        .process_transaction(&tx)
        .unwrap();
    let commitment_slots = CommitmentSlots {
        slot: 1,
        ..CommitmentSlots::default()
    };
    subscriptions.notify_subscribers(commitment_slots);
    let commitment_slots = CommitmentSlots {
        slot: 2,
        root: 1,
        highest_confirmed_slot: 1,
        highest_confirmed_root: 1,
    };
    subscriptions.notify_subscribers(commitment_slots);

    // Poll notifications generated by the transfer
    let mut notifications = Vec::new();
    let mut pubkeys = HashSet::new();
    loop {
        let response = receiver.recv_timeout(Duration::from_millis(100));
        match response {
            Ok(response) => {
                notifications.push(response.clone());
                pubkeys.insert(response.value.pubkey);
            }
            Err(_) => {
                break;
            }
        }
    }

    // Shutdown
    exit.store(true, Ordering::Relaxed);
    trigger.cancel();
    client.shutdown().unwrap();
    pubsub_service.close().unwrap();

    assert_eq!(notifications.len(), 1);
    assert!(pubkeys.contains(&bob.pubkey().to_string()));
}

#[test]
fn test_root_subscription() {
    let pubsub_addr = pubsub_addr();
    let exit = Arc::new(AtomicBool::new(false));

    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
    let bank0 = bank_forks.read().unwrap().get(0).unwrap();
    let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
    bank_forks.write().unwrap().insert(bank1);
    let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
    let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
        &exit,
        max_complete_transaction_status_slot,
        bank_forks.clone(),
        Arc::new(RwLock::new(BlockCommitmentCache::default())),
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
    ));
    let (trigger, pubsub_service) =
        PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);

    check_server_is_ready_or_panic(&pubsub_addr, 10, Duration::from_millis(300));

    let (mut client, receiver) =
        PubsubClient::root_subscribe(&format!("ws://0.0.0.0:{}/", pubsub_addr.port())).unwrap();

    let roots = vec![1, 2, 3];
    subscriptions.notify_roots(roots.clone());

    // Read notifications
    let mut errors: Vec<(Slot, Slot)> = Vec::new();
    for expected in roots {
        let response = receiver.recv();
        match response {
            Ok(response) => {
                if expected != response {
                    errors.push((expected, response));
                }
            }
            Err(_) => eprintln!("unexpected websocket receive timeout"),
        }
    }

    exit.store(true, Ordering::Relaxed);
    trigger.cancel();
    client.shutdown().unwrap();
    pubsub_service.close().unwrap();
    assert_eq!(errors, [].to_vec());
}

#[test]
fn test_slot_subscription() {
    let pubsub_addr = pubsub_addr();
    let exit = Arc::new(AtomicBool::new(false));
    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
    let optimistically_confirmed_bank =
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
    let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
    let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
        &exit,
        max_complete_transaction_status_slot,
        bank_forks,
        Arc::new(RwLock::new(BlockCommitmentCache::default())),
        optimistically_confirmed_bank,
    ));
    let (trigger, pubsub_service) =
        PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);

    check_server_is_ready_or_panic(&pubsub_addr, 10, Duration::from_millis(300));

    let (mut client, receiver) =
        PubsubClient::slot_subscribe(&format!("ws://0.0.0.0:{}/", pubsub_addr.port())).unwrap();

    let mut errors: Vec<(SlotInfo, SlotInfo)> = Vec::new();

    for i in 0..3 {
        subscriptions.notify_slot(i + 1, i, i);

        let maybe_actual = receiver.recv_timeout(Duration::from_millis(400));

        match maybe_actual {
            Ok(actual) => {
                let expected = SlotInfo {
                    slot: i + 1,
                    parent: i,
                    root: i,
                };

                if actual != expected {
                    errors.push((actual, expected));
                }
            }
            Err(_err) => {
                eprintln!("unexpected websocket receive timeout");
                break;
            }
        }
    }

    exit.store(true, Ordering::Relaxed);
    trigger.cancel();
    client.shutdown().unwrap();
    pubsub_service.close().unwrap();

    assert_eq!(errors, [].to_vec());
}

#[tokio::test]
async fn test_slot_subscription_async() {
    let sync_service = Arc::new(AtomicU64::new(0));
    let sync_client = Arc::clone(&sync_service);

    fn wait_until(atomic: &Arc<AtomicU64>, value: u64) {
        let now = Instant::now();
        while atomic.load(Ordering::Relaxed) != value {
            if now.elapsed() > Duration::from_secs(5) {
                panic!("wait for too long")
            }
            sleep(Duration::from_millis(1))
        }
    }

    let pubsub_addr = pubsub_addr();

    tokio::task::spawn_blocking(move || {
        let exit = Arc::new(AtomicBool::new(false));
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            &exit,
            max_complete_transaction_status_slot,
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::default())),
            optimistically_confirmed_bank,
        ));
        let (trigger, pubsub_service) =
            PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);

        check_server_is_ready_or_panic(&pubsub_addr, 10, Duration::from_millis(100));

        sync_service.store(1, Ordering::Relaxed);

        wait_until(&sync_service, 2);
        subscriptions.notify_slot(1, 0, 0);
        sync_service.store(3, Ordering::Relaxed);

        wait_until(&sync_service, 4);
        subscriptions.notify_slot(2, 1, 1);
        sync_service.store(5, Ordering::Relaxed);

        wait_until(&sync_service, 6);
        exit.store(true, Ordering::Relaxed);
        trigger.cancel();
        pubsub_service.close().unwrap();
    });

    wait_until(&sync_client, 1);
    let url = format!("ws://0.0.0.0:{}/", pubsub_addr.port());
    let pubsub_client = nonblocking::pubsub_client::PubsubClient::new(&url)
        .await
        .unwrap();
    let (mut notifications, unsubscribe) = pubsub_client.slot_subscribe().await.unwrap();
    sync_client.store(2, Ordering::Relaxed);

    wait_until(&sync_client, 3);
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(25), notifications.next()).await,
        Ok(Some(SlotInfo {
            slot: 1,
            parent: 0,
            root: 0,
        }))
    );
    sync_client.store(4, Ordering::Relaxed);

    wait_until(&sync_client, 5);
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(25), notifications.next()).await,
        Ok(Some(SlotInfo {
            slot: 2,
            parent: 1,
            root: 1,
        }))
    );
    sync_client.store(6, Ordering::Relaxed);

    unsubscribe().await;
}

fn check_server_is_ready_or_panic(
    socket_addr: &SocketAddr,
    mut retry: u8,
    sleep_duration: Duration,
) {
    loop {
        if retry == 0 {
            break;
        } else {
            retry = retry.checked_sub(1).unwrap();
        }

        if connect(format!("ws://{socket_addr}")).is_ok() {
            return;
        }
        sleep(sleep_duration);
    }

    panic!("server hasn't been ready");
}
