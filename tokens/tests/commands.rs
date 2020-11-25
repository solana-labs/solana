use solana_client::rpc_client::RpcClient;
use solana_core::test_validator::TestValidator;
use solana_tokens::commands::test_process_distribute_tokens_with_client;
use std::fs::remove_dir_all;

#[test]
fn test_process_distribute_with_rpc_client() {
    solana_logger::setup();
    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::with_no_fee();

    let client = RpcClient::new_socket(leader_data.rpc);
    test_process_distribute_tokens_with_client(&client, alice, None);

    // Explicit cleanup, otherwise "pure virtual method called" crash in Docker
    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
