use solana_client::rpc_client::RpcClient;
use solana_core::validator::new_validator_for_tests;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::KeypairUtil;
use solana_sdk::system_transaction;
use std::fs::remove_dir_all;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[test]
fn test_rpc_client() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let bob_pubkey = Pubkey::new_rand();

    let client = RpcClient::new_socket(leader_data.rpc);

    assert_eq!(
        client.get_version().unwrap(),
        format!("{{\"solana-core\":\"{}\"}}", solana_core::version!())
    );

    assert_eq!(client.get_balance(&bob_pubkey).unwrap(), 0);
    assert_eq!(client.get_balance(&alice.pubkey()).unwrap(), 10000);

    let (blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();

    let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
    let signature = client.send_transaction(&tx).unwrap();

    let mut confirmed_tx = false;

    let now = Instant::now();
    while now.elapsed().as_secs() <= 20 {
        let response = client
            .confirm_transaction_with_commitment(signature.as_str(), CommitmentConfig::default())
            .unwrap();

        if response.value {
            confirmed_tx = true;
            break;
        }

        sleep(Duration::from_millis(500));
    }

    assert!(confirmed_tx);

    assert_eq!(client.get_balance(&bob_pubkey).unwrap(), 20);
    assert_eq!(client.get_balance(&alice.pubkey()).unwrap(), 9980);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
