use solana_banks_client::start_tcp_client;
use solana_core::validator::{TestValidator, TestValidatorOptions};
use solana_sdk::native_token::sol_to_lamports;
use solana_tokens::commands::test_process_distribute_tokens_with_client;
use std::fs::remove_dir_all;
use tokio::runtime::Runtime;

#[test]
fn test_process_distribute_with_rpc_client() {
    let TestValidator {
        server,
        leader_data,
        alice,
        ledger_path,
        ..
    } = TestValidator::run_with_options(TestValidatorOptions {
        mint_lamports: sol_to_lamports(9_000_000.0),
        ..TestValidatorOptions::default()
    });

    Runtime::new().unwrap().block_on(async {
        let mut banks_client = start_tcp_client(leader_data.rpc_banks).await.unwrap();
        test_process_distribute_tokens_with_client(&mut banks_client, alice, None).await
    });

    // Explicit cleanup, otherwise "pure virtual method called" crash in Docker
    server.close().unwrap();
    remove_dir_all(ledger_path).unwrap();
}
