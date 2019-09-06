use bincode::serialize;
use log::*;
use serde_json::{json, Value};
use solana_client::rpc_client::get_rpc_request_str;
use solana_core::validator::new_validator_for_tests;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_transaction;
use std::fs::remove_dir_all;
use std::thread::sleep;
use std::time::Duration;

#[test]
fn test_rpc_send_tx() {
    solana_logger::setup();

    let (server, leader_data, alice, ledger_path) = new_validator_for_tests();
    let bob_pubkey = Pubkey::new_rand();

    let client = ureq::agent();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "getRecentBlockhash",
       "params": json!([])
    });
    let rpc_addr = leader_data.rpc;
    let rpc_string = get_rpc_request_str(rpc_addr, false);
    let response = client
        .post(&rpc_string)
        .set("Content-Type", "application/json")
        .send_json(request);
    let json: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let blockhash: Hash = json["result"][0].as_str().unwrap().parse().unwrap();

    info!("blockhash: {:?}", blockhash);
    let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
    let serial_tx = serialize(&tx).unwrap();

    let client = ureq::agent();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "sendTransaction",
       "params": json!([serial_tx])
    });
    let rpc_addr = leader_data.rpc;
    let rpc_string = get_rpc_request_str(rpc_addr, false);
    let response = client
        .post(&rpc_string)
        .set("Content-Type", "application/json")
        .send_json(request);
    let json: Value = serde_json::from_str(&response.into_string().unwrap()).unwrap();
    let signature = &json["result"];

    let mut confirmed_tx = false;

    let client = ureq::agent();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "confirmTransaction",
       "params": [signature],
    });

    for _ in 0..solana_sdk::clock::DEFAULT_TICKS_PER_SLOT {
        let response = client
            .post(&rpc_string)
            .set("Content-Type", "application/json")
            .send_json(request.clone());
        let response_json_text = response.into_string().unwrap();
        let json: Value = serde_json::from_str(&response_json_text).unwrap();

        if true == json["result"] {
            confirmed_tx = true;
            break;
        }

        sleep(Duration::from_millis(500));
    }

    assert_eq!(confirmed_tx, true);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
