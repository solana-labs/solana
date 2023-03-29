#![cfg(feature = "test-bpf")]

use {
    solana_program_test::*,
    solana_sbf_rust_mem::entrypoint::process_instruction,
    solana_sdk::{
        instruction::Instruction, pubkey::Pubkey, signature::Signer, transaction::Transaction,
    },
};

#[tokio::test]
async fn test_mem() {
    let program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "solana_sbf_rust_mem",
        program_id,
        processor!(process_instruction),
    );
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[Instruction::new_with_bincode(program_id, &(), vec![])],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer], recent_blockhash);
    banks_client.process_transaction(transaction).await.unwrap();
}
