use bincode::{deserialize, serialize};
use log::*;
use solana::cluster_info::FULLNODE_PORT_RANGE;
use solana::fullnode::new_fullnode_for_tests;
use solana::gossip_service::discover;
use solana_client::thin_client::create_client;
use solana_client::thin_client::{retry_get_balance, ThinClient};
use solana_logger;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::system_transaction::SystemTransaction;
use solana_vote_api::vote_state::VoteState;
use solana_vote_api::vote_transaction::VoteTransaction;
use std::fs::remove_dir_all;
use std::net::UdpSocket;
use std::thread::sleep;
use std::time::Duration;

pub fn transfer(
    thin_client: &ThinClient,
    lamports: u64,
    keypair: &Keypair,
    to: &Pubkey,
    blockhash: &Hash,
) -> std::io::Result<Signature> {
    debug!(
        "transfer: lamports={} from={:?} to={:?} blockhash={:?}",
        lamports,
        keypair.pubkey(),
        to,
        blockhash
    );
    let transaction = SystemTransaction::new_account(keypair, to, lamports, *blockhash, 0);
    thin_client.transfer_signed(&transaction)
}

#[test]
fn test_thin_client_basic() {
    solana_logger::setup();
    let (server, leader_data, alice, ledger_path) = new_fullnode_for_tests();
    let bob_pubkey = Keypair::new().pubkey();
    discover(&leader_data.gossip, 1).unwrap();

    let client = create_client(leader_data.client_facing_addr(), FULLNODE_PORT_RANGE);

    let transaction_count = client.get_transaction_count().unwrap();
    assert_eq!(transaction_count, 0);

    let blockhash = client.get_recent_blockhash();
    info!("test_thin_client blockhash: {:?}", blockhash);

    let signature = transfer(&client, 500, &alice, &bob_pubkey, &blockhash).unwrap();
    info!("test_thin_client signature: {:?}", signature);
    client.poll_for_signature(&signature).unwrap();

    let balance = client.get_balance(&bob_pubkey);
    assert_eq!(balance.unwrap(), 500);

    let transaction_count = client.get_transaction_count().unwrap();
    assert_eq!(transaction_count, 1);
    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
#[ignore]
fn test_bad_sig() {
    solana_logger::setup();
    let (server, leader_data, alice, ledger_path) = new_fullnode_for_tests();
    let bob_pubkey = Keypair::new().pubkey();
    discover(&leader_data.gossip, 1).unwrap();

    let client = create_client(leader_data.client_facing_addr(), FULLNODE_PORT_RANGE);

    let blockhash = client.get_recent_blockhash();

    let tx = SystemTransaction::new_account(&alice, &bob_pubkey, 500, blockhash, 0);

    let _sig = client.transfer_signed(&tx).unwrap();

    let blockhash = client.get_recent_blockhash();

    let mut tr2 = SystemTransaction::new_account(&alice, &bob_pubkey, 501, blockhash, 0);
    let mut instruction2 = deserialize(tr2.data(0)).unwrap();
    if let SystemInstruction::Move { ref mut lamports } = instruction2 {
        *lamports = 502;
    }
    tr2.instructions[0].data = serialize(&instruction2).unwrap();
    let signature = client.transfer_signed(&tr2).unwrap();
    client.poll_for_signature(&signature).unwrap();

    let balance = client.get_balance(&bob_pubkey);
    assert_eq!(balance.unwrap(), 1001);
    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_register_vote_account() {
    solana_logger::setup();
    let (server, leader_data, alice, ledger_path) = new_fullnode_for_tests();
    discover(&leader_data.gossip, 1).unwrap();

    let client = create_client(leader_data.client_facing_addr(), FULLNODE_PORT_RANGE);

    // Create the validator account, transfer some lamports to that account
    let validator_keypair = Keypair::new();
    let blockhash = client.get_recent_blockhash();
    let signature = transfer(
        &client,
        500,
        &alice,
        &validator_keypair.pubkey(),
        &blockhash,
    )
    .unwrap();

    client.poll_for_signature(&signature).unwrap();

    // Create and register the vote account
    let validator_vote_account_keypair = Keypair::new();
    let vote_account_id = validator_vote_account_keypair.pubkey();
    let blockhash = client.get_recent_blockhash();

    let transaction =
        VoteTransaction::new_account(&validator_keypair, &vote_account_id, blockhash, 1, 1);
    let signature = client.transfer_signed(&transaction).unwrap();
    client.poll_for_signature(&signature).unwrap();

    let balance = retry_get_balance(&client, &vote_account_id, Some(1))
        .expect("Expected balance for new account to exist");
    assert_eq!(balance, 1);

    const LAST: usize = 30;
    for run in 0..=LAST {
        let account_user_data = client
            .get_account_data(&vote_account_id)
            .expect("Expected valid response for account data");

        let vote_state = VoteState::deserialize(&account_user_data);

        if vote_state.map(|vote_state| vote_state.delegate_id) == Ok(vote_account_id) {
            break;
        }

        if run == LAST {
            panic!("Expected successful vote account registration");
        }
        sleep(Duration::from_millis(900));
    }

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}

#[test]
fn test_transaction_count() {
    // set a bogus address, see that we don't hang
    solana_logger::setup();
    let addr = "0.0.0.0:1234".parse().unwrap();
    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let client = ThinClient::new_socket_with_timeout(
        addr,
        addr,
        transactions_socket,
        Duration::from_secs(2),
    );
    client.get_transaction_count().unwrap_err();
}

#[test]
fn test_zero_balance_after_nonzero() {
    solana_logger::setup();
    let (server, leader_data, alice, ledger_path) = new_fullnode_for_tests();
    let bob_keypair = Keypair::new();
    discover(&leader_data.gossip, 1).unwrap();

    let client = create_client(leader_data.client_facing_addr(), FULLNODE_PORT_RANGE);
    let blockhash = client.get_recent_blockhash();
    info!("test_thin_client blockhash: {:?}", blockhash);

    let starting_alice_balance = client.poll_get_balance(&alice.pubkey()).unwrap();
    info!("Alice has {} lamports", starting_alice_balance);

    info!("Give Bob 500 lamports");
    let signature = transfer(&client, 500, &alice, &bob_keypair.pubkey(), &blockhash).unwrap();
    client.poll_for_signature(&signature).unwrap();

    let bob_balance = client.poll_get_balance(&bob_keypair.pubkey());
    assert_eq!(bob_balance.unwrap(), 500);

    info!("Take Bob's 500 lamports away");
    let signature = transfer(&client, 500, &bob_keypair, &alice.pubkey(), &blockhash).unwrap();
    client.poll_for_signature(&signature).unwrap();
    let alice_balance = client.poll_get_balance(&alice.pubkey()).unwrap();
    assert_eq!(alice_balance, starting_alice_balance);

    info!("Should get an error when Bob's balance hits zero and is purged");
    let bob_balance = client.poll_get_balance(&bob_keypair.pubkey());
    info!("Bob's balance is {:?}", bob_balance);
    assert!(bob_balance.is_err(),);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
