use serde_json::{json, Value};
use solana::rpc_request::{RpcClient, RpcRequest, RpcRequestHandler};
use solana::thin_client::new_fullnode;
use solana_drone::drone::run_local_drone;
use solana_sdk::bpf_loader;
use solana_wallet::wallet::{process_command, WalletCommand, WalletConfig};
use std::fs::{remove_dir_all, File};
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::channel;

#[test]
fn test_wallet_deploy_program() {
    let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pathbuf.push("tests");
    pathbuf.push("fixtures");
    pathbuf.push("noop");
    pathbuf.set_extension("so");

    let (server, leader_data, alice, ledger_path) = new_fullnode("test_wallet_deploy_program");

    let (sender, receiver) = channel();
    run_local_drone(alice, sender);
    let drone_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_from_socket(leader_data.rpc);

    let mut config = WalletConfig::default();
    config.drone_port = drone_addr.port();
    config.rpc_port = leader_data.rpc.port();
    config.command = WalletCommand::Airdrop(50);
    process_command(&config).unwrap();

    config.command = WalletCommand::Deploy(pathbuf.to_str().unwrap().to_string());

    let response = process_command(&config);
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
