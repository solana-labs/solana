use serde_json::{json, Value};
use solana::bank::Bank;
use solana::cluster_info::Node;
use solana::db_ledger::create_tmp_ledger_with_mint;
use solana::fullnode::Fullnode;
use solana::leader_scheduler::LeaderScheduler;
use solana::mint::Mint;
use solana::rpc_request::{RpcClient, RpcRequest, RpcRequestHandler};
use solana::vote_signer_proxy::VoteSignerProxy;
use solana_drone::drone::run_local_drone;
use solana_sdk::bpf_loader;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_vote_signer::rpc::LocalVoteSigner;
use solana_wallet::wallet::{process_command, WalletCommand, WalletConfig};
use std::fs::{remove_dir_all, File};
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};

#[test]
fn test_wallet_deploy_program() {
    let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pathbuf.push("tests");
    pathbuf.push("fixtures");
    pathbuf.push("noop");
    pathbuf.set_extension("so");

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
    let vote_signer =
        VoteSignerProxy::new(&vote_account_keypair, Box::new(LocalVoteSigner::default()));
    let last_id = bank.last_id();
    let server = Fullnode::new_with_bank(
        leader_keypair,
        Arc::new(vote_signer),
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

    let (sender, receiver) = channel();
    run_local_drone(alice.keypair(), sender);
    let drone_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_from_socket(leader_data.rpc);

    let mut config = WalletConfig::default();
    config.drone_port = drone_addr.port();
    config.rpc_port = leader_data.rpc.port();
    config.command = WalletCommand::Airdrop(50);
    let response = process_command(&config);
    assert!(response.is_ok());

    config.command = WalletCommand::Deploy(pathbuf.to_str().unwrap().to_string());

    let response = process_command(&config);
    assert!(response.is_ok());
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_id_str = json
        .as_object()
        .unwrap()
        .get("programId")
        .unwrap()
        .as_str()
        .unwrap();

    let params = json!([program_id_str]);
    let account_info = rpc_client
        .make_rpc_request(1, RpcRequest::GetAccountInfo, Some(params))
        .unwrap();
    let account_info_obj = account_info.as_object().unwrap();
    assert_eq!(account_info_obj.get("tokens").unwrap().as_u64().unwrap(), 1);
    let loader_array = account_info.get("loader").unwrap();
    assert_eq!(loader_array, &json!(bpf_loader::id()));
    assert_eq!(
        account_info_obj
            .get("executable")
            .unwrap()
            .as_bool()
            .unwrap(),
        true
    );

    let mut file = File::open(pathbuf.to_str().unwrap().to_string()).unwrap();
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();

    assert_eq!(
        account_info_obj
            .get("userdata")
            .unwrap()
            .as_array()
            .unwrap(),
        &elf
    );

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
