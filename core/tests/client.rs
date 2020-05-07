use solana_client::{
    pubsub_client::{PubsubClient, SlotInfoMessage},
    rpc_client::RpcClient,
};
use solana_core::{
    commitment::BlockCommitmentCache, rpc_pubsub_service::PubSubService,
    rpc_subscriptions::RpcSubscriptions, validator::TestValidator,
};
use solana_ledger::{
    bank_forks::BankForks,
    blockstore::Blockstore,
    genesis_utils::{create_genesis_config, GenesisConfigInfo},
    get_tmp_ledger_path,
};
use solana_runtime::bank::Bank;
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, rpc_port, signature::Signer,
    system_transaction,
};
use std::{
    fs::remove_dir_all,
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

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run();
    let bob_pubkey = Pubkey::new_rand();

    let client = RpcClient::new_socket(leader_data.rpc);

    assert_eq!(
        client.get_version().unwrap().solana_core,
        solana_clap_utils::version!()
    );

    assert!(client.get_account(&bob_pubkey).is_err());

    assert_eq!(client.get_balance(&bob_pubkey).unwrap(), 0);

    assert_eq!(client.get_balance(&alice.pubkey()).unwrap(), 1_000_000);

    let (blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();

    let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
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

    assert_eq!(client.get_balance(&bob_pubkey).unwrap(), 20);
    assert_eq!(client.get_balance(&alice.pubkey()).unwrap(), 999980);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_slot_subscription() {
    let pubsub_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        rpc_port::DEFAULT_RPC_PUBSUB_PORT,
    );
    let exit = Arc::new(AtomicBool::new(false));
    let ledger_path = get_tmp_ledger_path!();
    let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
    let bank = Bank::new(&genesis_config);
    let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
    let subscriptions = Arc::new(RpcSubscriptions::new(
        &exit,
        bank_forks,
        Arc::new(RwLock::new(BlockCommitmentCache::default_with_blockstore(
            blockstore,
        ))),
    ));
    let pubsub_service = PubSubService::new(&subscriptions, pubsub_addr, &exit);
    std::thread::sleep(Duration::from_millis(400));

    let (mut client, receiver) =
        PubsubClient::slot_subscribe(&format!("ws://0.0.0.0:{}/", pubsub_addr.port())).unwrap();

    let mut errors: Vec<(SlotInfoMessage, SlotInfoMessage)> = Vec::new();

    for i in 0..3 {
        subscriptions.notify_slot(i + 1, i, i);

        let maybe_actual = receiver.recv_timeout(Duration::from_millis(400));

        match maybe_actual {
            Ok(actual) => {
                let expected = SlotInfoMessage {
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
    client.shutdown().unwrap();
    pubsub_service.close().unwrap();

    assert_eq!(errors, [].to_vec());
}
