use serde_json::{json, Value};
use serial_test::serial;
use solana_client::{
    pubsub_client::PubsubClient,
    rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_response::SlotInfo,
};
use solana_rpc::{
    optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank,
    rpc_pubsub_service::{PubSubConfig, PubSubService},
    rpc_subscriptions::RpcSubscriptions,
};
use solana_runtime::{
    bank::Bank,
    bank_forks::BankForks,
    commitment::{BlockCommitmentCache, CommitmentSlots},
    genesis_utils::{create_genesis_config, GenesisConfigInfo},
};
use solana_sdk::{
    clock::Slot,
    commitment_config::CommitmentConfig,
    native_token::sol_to_lamports,
    pubkey::Pubkey,
    rpc_port,
    signature::{Keypair, Signer},
    system_program, system_transaction,
};
use solana_streamer::socket::SocketAddrSpace;
use solana_test_validator::TestValidator;
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    thread::sleep,
    time::{Duration, Instant},
};
use systemstat::Ipv4Addr;

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
            .confirm_transaction_with_commitment(&signature, CommitmentConfig::default())
            .unwrap();

        if response.value {
            confirmed_tx = true;
            break;
        }

        sleep(Duration::from_millis(500));
    }

    assert!(confirmed_tx);

    assert_eq!(
        client.get_balance(&bob_pubkey).unwrap(),
        sol_to_lamports(20.0)
    );
    assert_eq!(
        client.get_balance(&alice.pubkey()).unwrap(),
        original_alice_balance - sol_to_lamports(20.0)
    );
}

#[test]
#[serial]
fn test_account_subscription() {
    let pubsub_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        rpc_port::DEFAULT_RPC_PUBSUB_PORT,
    );
    let exit = Arc::new(AtomicBool::new(false));

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair: alice,
        ..
    } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let blockhash = bank.last_blockhash();
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
    let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
    let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
    bank_forks.write().unwrap().insert(bank1);
    let bob = Keypair::new();

    let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
        &exit,
        bank_forks.clone(),
        Arc::new(RwLock::new(BlockCommitmentCache::default())),
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
    ));
    let (trigger, pubsub_service) =
        PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);
    std::thread::sleep(Duration::from_millis(400));
    let config = Some(RpcAccountInfoConfig {
        commitment: Some(CommitmentConfig::finalized()),
        encoding: None,
        data_slice: None,
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
#[serial]
fn test_program_subscription() {
    let pubsub_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        rpc_port::DEFAULT_RPC_PUBSUB_PORT,
    );
    let exit = Arc::new(AtomicBool::new(false));

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair: alice,
        ..
    } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let blockhash = bank.last_blockhash();
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
    let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
    let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
    bank_forks.write().unwrap().insert(bank1);
    let bob = Keypair::new();

    let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
        &exit,
        bank_forks.clone(),
        Arc::new(RwLock::new(BlockCommitmentCache::default())),
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
    ));
    let (trigger, pubsub_service) =
        PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);
    std::thread::sleep(Duration::from_millis(400));
    let config = Some(RpcProgramAccountsConfig {
        ..RpcProgramAccountsConfig::default()
    });

    let (mut client, receiver) = PubsubClient::program_subscribe(
        &format!("ws://0.0.0.0:{}/", pubsub_addr.port()),
        &system_program::id(),
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

    // system_transaction::transfer() will generate 7 program account notifications for system_program
    // since accounts need to be created
    assert_eq!(notifications.len(), 7);

    assert!(pubkeys.contains(&alice.pubkey().to_string()));
    assert!(pubkeys.contains(&bob.pubkey().to_string()));
}

#[test]
#[serial]
fn test_root_subscription() {
    let pubsub_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        rpc_port::DEFAULT_RPC_PUBSUB_PORT,
    );
    let exit = Arc::new(AtomicBool::new(false));

    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
    let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
    let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
    bank_forks.write().unwrap().insert(bank1);

    let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
        &exit,
        bank_forks.clone(),
        Arc::new(RwLock::new(BlockCommitmentCache::default())),
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
    ));
    let (trigger, pubsub_service) =
        PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);
    std::thread::sleep(Duration::from_millis(400));
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
#[serial]
fn test_slot_subscription() {
    let pubsub_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        rpc_port::DEFAULT_RPC_PUBSUB_PORT,
    );
    let exit = Arc::new(AtomicBool::new(false));
    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
    let optimistically_confirmed_bank =
        OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
    let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
        &exit,
        bank_forks,
        Arc::new(RwLock::new(BlockCommitmentCache::default())),
        optimistically_confirmed_bank,
    ));
    let (trigger, pubsub_service) =
        PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);
    std::thread::sleep(Duration::from_millis(400));

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
