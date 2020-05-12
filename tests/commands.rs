use solana_client::rpc_client::RpcClient;
use solana_core::validator::{TestValidator, TestValidatorOptions};
use solana_sdk::native_token::sol_to_lamports;
use solana_tokens::commands::test_process_distribute_tokens_with_client;
use std::fs::remove_dir_all;

#[test]
fn test_process_distribute_with_rpc_client() {
    let validator = TestValidator::run_with_options(TestValidatorOptions {
        mint_lamports: sol_to_lamports(9_000_000.0),
        ..TestValidatorOptions::default()
    });
    let rpc_client = RpcClient::new_socket(validator.leader_data.rpc);
    test_process_distribute_tokens_with_client(rpc_client, validator.alice);

    validator.server.close().unwrap();
    remove_dir_all(validator.ledger_path).unwrap();
}
