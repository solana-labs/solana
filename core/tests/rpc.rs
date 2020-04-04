use bincode::serialize;
use jsonrpc_core::futures::{
    future::{self, Future},
    stream::Stream,
};
use jsonrpc_core_client::transports::ws;
use log::*;
use reqwest::{self, header::CONTENT_TYPE};
use serde_json::{json, Value};
use solana_client::{
    rpc_client::{get_rpc_request_str, RpcClient},
    rpc_response::{Response, RpcSignatureResult},
};
use solana_core::{rpc_pubsub::gen_client::Client as PubsubClient, validator::TestValidator};
use solana_sdk::{
    commitment_config::CommitmentConfig, hash::Hash, pubkey::Pubkey, signature::Signer,
    system_transaction, transaction::Transaction,
};
use std::{
    collections::HashSet,
    fs::remove_dir_all,
    net::UdpSocket,
    sync::mpsc::channel,
    thread::sleep,
    time::{Duration, Instant},
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

    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    transactions_socket.connect(leader_data.tpu).unwrap();

    // Create transaction signatures to subscribe to
    let transactions: Vec<Transaction> = (0..1000)
        .map(|_| system_transaction::transfer(&alice, &Pubkey::new_rand(), 1, genesis_hash))
        .collect();
    let mut signature_set: HashSet<String> = transactions
        .iter()
        .map(|tx| tx.signatures[0].to_string())
        .collect();

    // Track when subscriptions are ready
    let (ready_sender, ready_receiver) = channel::<()>();
    // Track when status notifications are received
    let (status_sender, status_receiver) = channel::<(String, Response<RpcSignatureResult>)>();

    // Create the pub sub runtime
    let mut rt = Runtime::new().unwrap();
    let rpc_pubsub_url = format!("ws://{}/", leader_data.rpc_pubsub);

    // Subscribe to all signatures
    rt.spawn({
        let connect = ws::try_connect::<PubsubClient>(&rpc_pubsub_url).unwrap();
        let signature_set = signature_set.clone();
        connect
            .and_then(move |client| {
                for sig in signature_set {
                    let status_sender = status_sender.clone();
                    tokio::spawn(
                        client
                            .signature_subscribe(sig.clone(), None)
                            .and_then(move |sig_stream| {
                                sig_stream.for_each(move |result| {
                                    status_sender.send((sig.clone(), result)).unwrap();
                                    future::ok(())
                                })
                            })
                            .map_err(|err| {
                                eprintln!("sig sub err: {:#?}", err);
                            }),
                    );
                }
                tokio::spawn(
                    client
                        .slot_subscribe()
                        .and_then(move |slot_stream| {
                            slot_stream.for_each(move |_| {
                                ready_sender.send(()).unwrap();
                                future::ok(())
                            })
                        })
                        .map_err(|err| {
                            eprintln!("slot sub err: {:#?}", err);
                        }),
                );
                future::ok(())
            })
            .map_err(|_| ())
    });

    // Wait for signature subscriptions
    ready_receiver.recv_timeout(Duration::from_secs(2)).unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);
    let mut mint_balance = rpc_client
        .get_balance_with_commitment(&alice.pubkey(), CommitmentConfig::recent())
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
    let expected_mint_balance = mint_balance - transactions.len() as u64;
    while mint_balance != expected_mint_balance && now.elapsed() < Duration::from_secs(5) {
        mint_balance = rpc_client
            .get_balance_with_commitment(&alice.pubkey(), CommitmentConfig::recent())
            .unwrap()
            .value;
        sleep(Duration::from_millis(100));
    }

    // Wait for all signature subscriptions
    let deadline = Instant::now() + Duration::from_secs(5);
    while !signature_set.is_empty() {
        let timeout = deadline.saturating_duration_since(Instant::now());
        match status_receiver.recv_timeout(timeout) {
            Ok((sig, result)) => {
                assert!(result.value.err.is_none());
                assert!(signature_set.remove(&sig));
            }
            Err(_err) => {
                assert!(
                    false,
                    "recv_timeout, {}/{} signatures remaining",
                    signature_set.len(),
                    transactions.len()
                );
            }
        }
    }

    rt.shutdown_now().wait().unwrap();
    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
