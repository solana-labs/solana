use bincode::serialize;
use jsonrpc_core::futures::{
    future::{self, Future},
    stream::Stream,
};
use jsonrpc_core_client::transports::ws;
use log::*;
use reqwest::{self, header::CONTENT_TYPE};
use serde_json::{json, Value};
use solana_account_decoder::UiAccount;
use solana_client::{
    rpc_client::RpcClient,
    rpc_response::{Response, RpcSignatureResult},
};
use solana_core::{rpc_pubsub::gen_client::Client as PubsubClient, test_validator::TestValidator};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    signature::{Keypair, Signer},
    system_transaction,
    transaction::Transaction,
};
use std::{
    collections::HashSet,
    net::UdpSocket,
    sync::mpsc::channel,
    thread::sleep,
    time::{Duration, Instant},
};
use tokio_01::runtime::Runtime;

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
    let test_validator = TestValidator::with_no_fees(alice.pubkey());
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
    let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
    let serialized_encoded_tx = bs58::encode(serialize(&tx).unwrap()).into_string();

    let req = json_req!("sendTransaction", json!([serialized_encoded_tx]));
    let json: Value = post_rpc(req, &rpc_url);

    let signature = &json["result"];

    let mut confirmed_tx = false;

    let request = json_req!("confirmTransaction", [signature]);

    for _ in 0..solana_sdk::clock::DEFAULT_TICKS_PER_SLOT {
        let json = post_rpc(request.clone(), &rpc_url);

        if true == json["result"]["value"] {
            confirmed_tx = true;
            break;
        }

        sleep(Duration::from_millis(500));
    }

    assert_eq!(confirmed_tx, true);

    use solana_account_decoder::UiAccountEncoding;
    use solana_client::rpc_config::RpcAccountInfoConfig;
    let config = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        commitment: None,
        data_slice: None,
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
    let test_validator = TestValidator::with_no_fees(alice.pubkey());
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
fn test_rpc_subscriptions() {
    solana_logger::setup();

    let alice = Keypair::new();
    let test_validator = TestValidator::with_no_fees(alice.pubkey());

    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    transactions_socket.connect(test_validator.tpu()).unwrap();

    let rpc_client = RpcClient::new(test_validator.rpc_url());
    let recent_blockhash = rpc_client.get_recent_blockhash().unwrap().0;

    // Create transaction signatures to subscribe to
    let transactions: Vec<Transaction> = (0..1000)
        .map(|_| {
            system_transaction::transfer(
                &alice,
                &solana_sdk::pubkey::new_rand(),
                1,
                recent_blockhash,
            )
        })
        .collect();
    let mut signature_set: HashSet<String> = transactions
        .iter()
        .map(|tx| tx.signatures[0].to_string())
        .collect();
    let account_set: HashSet<String> = transactions
        .iter()
        .map(|tx| tx.message.account_keys[1].to_string())
        .collect();

    // Track when subscriptions are ready
    let (ready_sender, ready_receiver) = channel::<()>();
    // Track account notifications are received
    let (account_sender, account_receiver) = channel::<Response<UiAccount>>();
    // Track when status notifications are received
    let (status_sender, status_receiver) = channel::<(String, Response<RpcSignatureResult>)>();

    // Create the pub sub runtime
    let mut rt = Runtime::new().unwrap();

    // Subscribe to all signatures
    rt.spawn({
        let connect = ws::try_connect::<PubsubClient>(&test_validator.rpc_pubsub_url()).unwrap();
        let signature_set = signature_set.clone();
        connect
            .and_then(move |client| {
                for sig in signature_set {
                    let status_sender = status_sender.clone();
                    tokio_01::spawn(
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
                tokio_01::spawn(
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
                for pubkey in account_set {
                    let account_sender = account_sender.clone();
                    tokio_01::spawn(
                        client
                            .account_subscribe(pubkey, None)
                            .and_then(move |account_stream| {
                                account_stream.for_each(move |result| {
                                    account_sender.send(result).unwrap();
                                    future::ok(())
                                })
                            })
                            .map_err(|err| {
                                eprintln!("acct sub err: {:#?}", err);
                            }),
                    );
                }
                future::ok(())
            })
            .map_err(|_| ())
    });

    // Wait for signature subscriptions
    ready_receiver.recv_timeout(Duration::from_secs(2)).unwrap();

    let rpc_client = RpcClient::new(test_validator.rpc_url());
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
    let deadline = Instant::now() + Duration::from_secs(7);
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
                assert!(
                    false,
                    "recv_timeout, {}/{} signatures remaining",
                    signature_set.len(),
                    transactions.len()
                );
            }
        }
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut account_notifications = transactions.len();
    while account_notifications > 0 {
        let timeout = deadline.saturating_duration_since(Instant::now());
        match account_receiver.recv_timeout(timeout) {
            Ok(result) => {
                assert_eq!(result.value.lamports, 1);
                account_notifications -= 1;
            }
            Err(_err) => {
                assert!(
                    false,
                    "recv_timeout, {}/{} accounts remaining",
                    account_notifications,
                    transactions.len()
                );
            }
        }
    }

    rt.shutdown_now().wait().unwrap();
}
