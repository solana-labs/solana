use serde_json::json;
use solana::bank::Bank;
use solana::cluster_info::Node;
use solana::db_ledger::create_tmp_ledger;
use solana::fullnode::Fullnode;
use solana::genesis_block::GenesisBlock;
use solana::leader_scheduler::LeaderScheduler;
use solana::rpc_request::{RpcClient, RpcRequest, RpcRequestHandler};
use solana::storage_stage::STORAGE_ROTATE_TEST_COUNT;
use solana::vote_signer_proxy::VoteSignerProxy;
use solana_drone::drone::run_local_drone;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_vote_signer::rpc::LocalVoteSigner;
use solana_wallet::wallet::{process_command, WalletCommand, WalletConfig};
use std::fs::remove_dir_all;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};

#[test]
fn test_wallet_request_airdrop() {
    let leader_keypair = Arc::new(Keypair::new());
    let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
    let leader_data = leader.info.clone();

    let (genesis_block, alice) = GenesisBlock::new(10_000);
    let mut bank = Bank::new(&genesis_block);
    let ledger_path = create_tmp_ledger("thin_client", &genesis_block);
    let entry_height = 0;

    let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
        leader_data.id,
    )));
    bank.leader_scheduler = leader_scheduler;
    let vote_account_keypair = Arc::new(Keypair::new());
    let vote_signer =
        VoteSignerProxy::new(&vote_account_keypair, Box::new(LocalVoteSigner::default()));
    let last_id = bank.last_id();
    let server = Fullnode::new_with_bank(
        leader_keypair,
        Some(Arc::new(vote_signer)),
        bank,
        &ledger_path,
        entry_height,
        &last_id,
        leader,
        None,
        false,
        None,
        STORAGE_ROTATE_TEST_COUNT,
    );

    let (sender, receiver) = channel();
    run_local_drone(alice, sender);
    let drone_addr = receiver.recv().unwrap();

    let mut bob_config = WalletConfig::default();
    bob_config.drone_port = drone_addr.port();
    bob_config.rpc_port = leader_data.rpc.port();
    bob_config.command = WalletCommand::Airdrop(50);

    let sig_response = process_command(&bob_config);
    assert!(sig_response.is_ok());

    let rpc_client = RpcClient::new_from_socket(leader_data.rpc);

    let params = json!([format!("{}", bob_config.id.pubkey())]);
    let balance = rpc_client
        .make_rpc_request(1, RpcRequest::GetBalance, Some(params))
        .unwrap()
        .as_u64()
        .unwrap();
    assert_eq!(balance, 50);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
