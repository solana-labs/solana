use solana_client::rpc_client::RpcClient;
use solana_core::test_validator::TestValidator;
use solana_tokens::commands::test_process_distribute_tokens_with_client;
use std::fs::remove_dir_all;

#[test]
fn test_process_distribute_with_rpc_client() {
    let validator = TestValidator::run();
    let rpc_client = RpcClient::new_socket(validator.leader_data.rpc);
    test_process_distribute_tokens_with_client(rpc_client, validator.alice, None);

    validator.server.close().unwrap();
    remove_dir_all(validator.ledger_path).unwrap();
}
