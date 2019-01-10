use serde_json::json;
use solana::bank::Bank;
use solana::cluster_info::Node;
use solana::db_ledger::create_tmp_ledger_with_mint;
use solana::fullnode::Fullnode;
use solana::leader_scheduler::LeaderScheduler;
use solana::mint::Mint;
use solana::rpc_request::{RpcClient, RpcRequest, RpcRequestHandler};
use solana_drone::drone::run_local_drone;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_wallet::wallet::{process_command, WalletCommand, WalletConfig};
use std::fs::remove_dir_all;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;

#[test]
fn test_wallet_request_airdrop() {
    let leader_keypair = Arc::new(Keypair::new());
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();

    let alice = Mint::new(10_000);
    let mut bank = Bank::new(&alice);
    let ledger_path = create_tmp_ledger_with_mint("thin_client", &alice);
    let entry_height = alice.create_entries().len() as u64;

    let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
        leader_data.id,
    )));
    bank.leader_scheduler = leader_scheduler;
    let vote_account_keypair = Arc::new(Keypair::new());
    let last_id = bank.last_id();
    let server = Fullnode::new_with_bank(
        leader_keypair,
        &vote_account_keypair.pubkey(),
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
        bank,
        None,
        entry_height,
        &last_id,
        leader,
        None,
        &ledger_path,
        false,
        None,
    );
    sleep(Duration::from_millis(900));

    let (sender, receiver) = channel();
    run_local_drone(alice.keypair(), sender);
    let drone_addr = receiver.recv().unwrap();

    let mut bob_config = WalletConfig::default();
    bob_config.network = leader_data.gossip;
    bob_config.drone_port = Some(drone_addr.port());
    bob_config.command = WalletCommand::Airdrop(50);

    let sig_response = process_command(&bob_config);
    assert!(sig_response.is_ok());

    let rpc_client = RpcClient::new_from_socket(leader_data.rpc);

    let params = json!([format!("{}", bob_config.id.pubkey())]);
    let balance = RpcRequest::GetBalance
        .make_rpc_request(&rpc_client, 1, Some(params))
        .unwrap()
        .as_u64()
        .unwrap();
    assert_eq!(balance, 50);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
