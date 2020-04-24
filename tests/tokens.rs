use solana_client::rpc_client::RpcClient;
use solana_core::validator::TestValidator;
use solana_sdk::{
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use solana_tokens::{thin_client::ThinClient, tokens::test_process_distribute_with_client};
use std::fs::remove_dir_all;

#[test]
#[ignore]
fn test_process_distribute_with_rpc_client() {
    let validator = TestValidator::run();
    let rpc_client = RpcClient::new_socket(validator.leader_data.rpc);

    let bob = Keypair::new();
    let instruction = system_instruction::transfer(&validator.alice.pubkey(), &bob.pubkey(), 1);
    let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash().unwrap();
    let signers = vec![&validator.alice];

    let mut transaction =
        Transaction::new_signed_instructions(&signers, vec![instruction], blockhash);
    rpc_client
        .send_and_confirm_transaction_with_spinner(&mut transaction, &signers)
        .unwrap();
    assert_ne!(rpc_client.get_balance(&bob.pubkey()).unwrap(), 0);

    let thin_client = ThinClient(rpc_client);
    test_process_distribute_with_client(&thin_client, bob);

    validator.server.close().unwrap();
    remove_dir_all(validator.ledger_path).unwrap();
}
