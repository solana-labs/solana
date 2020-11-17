<<<<<<< HEAD
use solana_client::rpc_client::RpcClient;
use solana_core::test_validator::{TestValidator, TestValidatorOptions};
use solana_sdk::native_token::sol_to_lamports;
=======
use solana_banks_client::start_tcp_client;
use solana_core::test_validator::TestValidator;
>>>>>>> ef9968959... Improve TestValidator instantiation (#13627)
use solana_tokens::commands::test_process_distribute_tokens_with_client;
use std::fs::remove_dir_all;

#[test]
fn test_process_distribute_with_rpc_client() {
<<<<<<< HEAD
    let validator = TestValidator::run_with_options(TestValidatorOptions {
        mint_lamports: sol_to_lamports(9_000_000.0),
        ..TestValidatorOptions::default()
    });
    let rpc_client = RpcClient::new_socket(validator.leader_data.rpc);
    test_process_distribute_tokens_with_client(rpc_client, validator.alice, None);
=======
    solana_logger::setup();
    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run();
>>>>>>> ef9968959... Improve TestValidator instantiation (#13627)

    validator.server.close().unwrap();
    remove_dir_all(validator.ledger_path).unwrap();
}
