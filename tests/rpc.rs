use bincode::serialize;
use reqwest;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};
use solana::rpc_request::get_rpc_request_str;
use solana::thin_client::new_fullnode;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use std::fs::remove_dir_all;
use std::thread::sleep;
use std::time::Duration;

#[test]
#[ignore]
fn test_rpc_send_tx() {
    let (server, leader_data, genesis_block, alice, ledger_path) = new_fullnode("test_rpc_send_tx");
    let bob_pubkey = Keypair::new().pubkey();

    let last_id = genesis_block.last_id();
    let tx = Transaction::system_move(&alice, bob_pubkey, 20, last_id, 0);
    let serial_tx = serialize(&tx).unwrap();

    let client = reqwest::Client::new();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "sendTransaction",
       "params": json!([serial_tx])
    });
    let rpc_addr = leader_data.rpc;
    let rpc_string = get_rpc_request_str(rpc_addr, false);
    let mut response = client
        .post(&rpc_string)
        .header(CONTENT_TYPE, "application/json")
        .body(request.to_string())
        .send()
        .unwrap();
    let json: Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    let signature = &json["result"];

    let mut confirmed_tx = false;

    let client = reqwest::Client::new();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "confirmTransaction",
       "params": [signature],
    });

    for _ in 0..5 {
        let mut response = client
            .post(&rpc_string)
            .header(CONTENT_TYPE, "application/json")
            .body(request.to_string())
            .send()
            .unwrap();
        let response_json_text = response.text().unwrap();
        let json: Value = serde_json::from_str(&response_json_text).unwrap();

        if true == json["result"] {
            confirmed_tx = true;
            break;
        }

        sleep(Duration::from_millis(250));
    }

    assert_eq!(confirmed_tx, true);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
