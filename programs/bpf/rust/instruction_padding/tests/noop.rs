#![cfg(feature = "test-bpf")]

use {
    solana_bpf_rust_instruction_padding::{instruction::noop, process_instruction},
    solana_program_test::{processor, tokio, ProgramTest},
    solana_sdk::{
        instruction::AccountMeta, pubkey::Pubkey, signature::Signer, transaction::Transaction,
    },
};

#[tokio::test]
async fn success_with_noop() {
    let program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "solana_bpf_rust_instruction_padding",
        program_id,
        processor!(process_instruction),
    );

    let mut context = program_test.start_with_context().await;

    let padding_accounts = vec![
        AccountMeta::new_readonly(Pubkey::new_unique(), false),
        AccountMeta::new_readonly(Pubkey::new_unique(), false),
        AccountMeta::new_readonly(Pubkey::new_unique(), false),
    ];

    let padding_data = 800;

    let transaction = Transaction::new_signed_with_payer(
        &[noop(program_id, padding_accounts, padding_data).unwrap()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}
