use bincode::serialize;
use jsonrpc_core::futures::{
    future::{self, Future},
    stream::Stream,
};
use jsonrpc_core_client::transports::ws;
use log::*;
use reqwest::{self, header::CONTENT_TYPE};
use serde_json::{json, Value};
use solana_client::rpc_client::get_rpc_request_str;
use solana_core::{rpc_pubsub::gen_client::Client as PubsubClient, validator::TestValidator};
use solana_sdk::{hash::Hash, pubkey::Pubkey, system_transaction, transaction};
use std::{
    collections::HashSet,
    fs::remove_dir_all,
    net::UdpSocket,
    sync::mpsc::channel,
    sync::{Arc, Mutex},
    thread::sleep,
    time::Duration,
    time::SystemTime,
};
use tokio::runtime::Runtime;

#[test]
fn test_rpc_send_tx() {
    solana_logger::setup();

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run();
    let bob_pubkey = Pubkey::new_rand();

    let client = reqwest::blocking::Client::new();
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
        .header(CONTENT_TYPE, "application/json")
        .body(request.to_string())
        .send()
        .unwrap();
    let json: Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    let blockhash: Hash = json["result"]["value"]["blockhash"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    info!("blockhash: {:?}", blockhash);
    let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
    let serialized_encoded_tx = bs58::encode(serialize(&tx).unwrap()).into_string();

    let client = reqwest::blocking::Client::new();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "sendTransaction",
       "params": json!([serialized_encoded_tx])
    });
    let rpc_addr = leader_data.rpc;
    let rpc_string = get_rpc_request_str(rpc_addr, false);
    let response = client
        .post(&rpc_string)
        .header(CONTENT_TYPE, "application/json")
        .body(request.to_string())
        .send()
        .unwrap();
    let json: Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    let signature = &json["result"];

    let mut confirmed_tx = false;

    let client = reqwest::blocking::Client::new();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "confirmTransaction",
       "params": [signature],
    });

    for _ in 0..solana_sdk::clock::DEFAULT_TICKS_PER_SLOT {
        let response = client
            .post(&rpc_string)
            .header(CONTENT_TYPE, "application/json")
            .body(request.to_string())
            .send()
            .unwrap();
        let response_json_text = response.text().unwrap();
        let json: Value = serde_json::from_str(&response_json_text).unwrap();

        if true == json["result"]["value"] {
            confirmed_tx = true;
            break;
        }

        sleep(Duration::from_millis(500));
    }

    assert_eq!(confirmed_tx, true);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_rpc_invalid_requests() {
    solana_logger::setup();

    let TestValidator {
        server,
        leader_data,
        ledger_path,
        ..
    } = TestValidator::run();
    let bob_pubkey = Pubkey::new_rand();

    // test invalid get_balance request
    let client = reqwest::blocking::Client::new();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "getBalance",
       "params": json!(["invalid9999"])
    });
    let rpc_addr = leader_data.rpc;
    let rpc_string = get_rpc_request_str(rpc_addr, false);
    let response = client
        .post(&rpc_string)
        .header(CONTENT_TYPE, "application/json")
        .body(request.to_string())
        .send()
        .unwrap();
    let json: Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    let the_error = json["error"]["message"].as_str().unwrap();
    assert_eq!(the_error, "Invalid request");

    // test invalid get_account_info request
    let client = reqwest::blocking::Client::new();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "getAccountInfo",
       "params": json!(["invalid9999"])
    });
    let rpc_addr = leader_data.rpc;
    let rpc_string = get_rpc_request_str(rpc_addr, false);
    let response = client
        .post(&rpc_string)
        .header(CONTENT_TYPE, "application/json")
        .body(request.to_string())
        .send()
        .unwrap();
    let json: Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    let the_error = json["error"]["message"].as_str().unwrap();
    assert_eq!(the_error, "Invalid request");

    // test invalid get_account_info request
    let client = reqwest::blocking::Client::new();
    let request = json!({
       "jsonrpc": "2.0",
       "id": 1,
       "method": "getAccountInfo",
       "params": json!([bob_pubkey.to_string()])
    });
    let rpc_addr = leader_data.rpc;
    let rpc_string = get_rpc_request_str(rpc_addr, false);
    let response = client
        .post(&rpc_string)
        .header(CONTENT_TYPE, "application/json")
        .body(request.to_string())
        .send()
        .unwrap();
    let json: Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    let the_value = &json["result"]["value"];
    assert!(the_value.is_null());

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_rpc_subscriptions() {
    solana_logger::setup();

    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        genesis_hash,
        ..
    } = TestValidator::run();

    // Create transaction signatures to subscribe to
    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let mut signature_set: HashSet<String> = (0..1000)
        .map(|_| {
            let tx = system_transaction::transfer(&alice, &Pubkey::new_rand(), 1, genesis_hash);
            transactions_socket
                .send_to(&bincode::serialize(&tx).unwrap(), leader_data.tpu)
                .unwrap();
            tx.signatures[0].to_string()
        })
        .collect();

    // Create the pub sub runtime
    let mut rt = Runtime::new().unwrap();
    let rpc_pubsub_url = format!("ws://{}/", leader_data.rpc_pubsub);
    let (sender, receiver) = channel::<(String, transaction::Result<()>)>();
    let sender = Arc::new(Mutex::new(sender));

    rt.spawn({
        let connect = ws::try_connect::<PubsubClient>(&rpc_pubsub_url).unwrap();
        let signature_set = signature_set.clone();
        connect
            .and_then(move |client| {
                for sig in signature_set {
                    let sender = sender.clone();
                    tokio::spawn(
                        client
                            .signature_subscribe(sig.clone(), None)
                            .and_then(move |sig_stream| {
                                sig_stream.for_each(move |result| {
                                    sender.lock().unwrap().send((sig.clone(), result)).unwrap();
                                    future::ok(())
                                })
                            })
                            .map_err(|err| {
                                eprintln!("sig sub err: {:#?}", err);
                            }),
                    );
                }
                future::ok(())
            })
            .map_err(|_| ())
    });

    // Wait for all signature subscriptions
    let now = SystemTime::now();
    let timeout = Duration::from_secs(5);
    while !signature_set.is_empty() {
        assert!(now.elapsed().unwrap() < timeout);
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok((sig, result)) => {
                assert!(result.is_ok());
                assert!(signature_set.remove(&sig));
            }
            Err(_err) => {
                eprintln!("unexpected receive timeout");
                assert!(false)
            }
        }
    }

    rt.shutdown_now().wait().unwrap();
    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
