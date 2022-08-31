#![cfg(feature = "test-bpf")]

use {
    solana_bpf_rust_instruction_padding::{
        instructions::create_padded_instruction, process_instruction,
    },
    solana_program_test::{processor, tokio, ProgramTest},
    solana_sdk::{
        instruction::AccountMeta, native_token::LAMPORTS_PER_SOL, pubkey::Pubkey,
        signature::Signer, system_instruction, transaction::Transaction,
    },
};

#[tokio::test]
async fn no_panic() {
    let program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "solana_bpf_rust_instruction_padding",
        program_id,
        processor!(process_instruction),
    );

    let mut context = program_test.start_with_context().await;
    let to = Pubkey::new_unique();

    let transfer_amount = LAMPORTS_PER_SOL;
    let transfer_instruction =
        system_instruction::transfer(&context.payer.pubkey(), &to, transfer_amount);

    let padding_accounts = vec![
        AccountMeta::new_readonly(Pubkey::new_unique(), false),
        AccountMeta::new_readonly(Pubkey::new_unique(), false),
        AccountMeta::new_readonly(Pubkey::new_unique(), false),
    ];

    let padding_data = 800;

    let transaction = Transaction::new_signed_with_payer(
        &[create_padded_instruction(
            program_id,
            transfer_instruction,
            padding_accounts,
            padding_data,
        )
        .unwrap()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // make sure the transfer went through
    assert_eq!(
        transfer_amount,
        context
            .banks_client
            .get_account(to)
            .await
            .unwrap()
            .unwrap()
            .lamports
    );
}
