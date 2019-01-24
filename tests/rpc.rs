use bincode::serialize;
use reqwest;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};
use solana::bank::Bank;
use solana::cluster_info::Node;
use solana::db_ledger::create_tmp_ledger_with_mint;
use solana::fullnode::Fullnode;
use solana::leader_scheduler::LeaderScheduler;
use solana::mint::Mint;
use solana::rpc_request::get_rpc_request_str;
use solana::storage_stage::STORAGE_ROTATE_TEST_COUNT;
use solana::vote_signer_proxy::VoteSignerProxy;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use solana_vote_signer::rpc::LocalVoteSigner;
use std::fs::remove_dir_all;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;

#[test]
#[ignore]
fn test_rpc_send_tx() {
    let leader_keypair = Arc::new(Keypair::new());
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

    let alice = Mint::new(10_000_000);
    let mut bank = Bank::new(&alice);
    let bob_pubkey = Keypair::new().pubkey();
    let leader_data = leader.info.clone();
    let ledger_path = create_tmp_ledger_with_mint("rpc_send_tx", &alice);

    let last_id = bank.last_id();
    let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 20, last_id, 0);
    let serial_tx = serialize(&tx).unwrap();

    let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
        leader_data.id,
    )));
    bank.leader_scheduler = leader_scheduler;

    let vote_account_keypair = Arc::new(Keypair::new());
    let vote_signer =
        VoteSignerProxy::new(&vote_account_keypair, Box::new(LocalVoteSigner::default()));
    let entry_height = alice.create_entries().len() as u64;
    let server = Fullnode::new_with_bank(
        leader_keypair,
        Some(Arc::new(vote_signer)),
        bank,
        None,
        entry_height,
        &last_id,
        leader,
        None,
        &ledger_path,
        false,
        None,
        STORAGE_ROTATE_TEST_COUNT,
    );

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
