use {
    solana_program_test::ProgramTest,
    solana_sdk::{
        bpf_loader, instruction::Instruction, pubkey::Pubkey, signature::Signer,
        transaction::Transaction,
    },
};

#[tokio::test]
async fn test_add_bpf_program() {
    let program_id = Pubkey::new_unique();

    let mut program_test = ProgramTest::default();
    program_test.prefer_bpf(true);
    program_test.add_program("noop_program", program_id, None);

    let context = program_test.start_with_context().await;

    // Assert the program is a BPF Loader 2 program.
    let program_account = context
        .banks_client
        .get_account(program_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(program_account.owner, bpf_loader::id());

    // Invoke the program.
    let instruction = Instruction::new_with_bytes(program_id, &[], Vec::new());
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
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
