use solana_cli::cli::{process_command, CliCommand, CliConfig};
use solana_client::rpc_client::RpcClient;
use solana_core::validator::TestValidator;
use solana_faucet::faucet::run_local_faucet;
use solana_sdk::signature::Keypair;
use std::{fs::remove_dir_all, sync::mpsc::channel};

#[test]
fn test_cli_request_airdrop() {
    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run();
    let (sender, receiver) = channel();
    run_local_faucet(alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let mut bob_config = CliConfig::default();
    bob_config.json_rpc_url = format!("http://{}:{}", leader_data.rpc.ip(), leader_data.rpc.port());
    bob_config.command = CliCommand::Airdrop {
        faucet_host: None,
        faucet_port: faucet_addr.port(),
        pubkey: None,
        lamports: 50,
    };
    let keypair = Keypair::new();
    bob_config.signers = vec![&keypair];

    let sig_response = process_command(&bob_config);
    sig_response.unwrap();

    let rpc_client = RpcClient::new_socket(leader_data.rpc);

    let balance = rpc_client
        .retry_get_balance(&bob_config.signers[0].pubkey(), 1)
        .unwrap()
        .unwrap();
    assert_eq!(balance, 50);

    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
