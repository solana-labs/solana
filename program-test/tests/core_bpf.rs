use {
    solana_program_test::ProgramTest,
    solana_sdk::{
        bpf_loader_upgradeable, instruction::Instruction, signature::Signer,
        transaction::Transaction,
    },
};

#[tokio::test]
async fn test_add_bpf_program() {
    // Core BPF program: Address Lookup Lable.
    let program_id = solana_sdk::address_lookup_table::program::id();

    let mut program_test = ProgramTest::default();
    program_test.add_upgradeable_program_to_genesis("noop_program", &program_id);

    let context = program_test.start_with_context().await;

    // Assert the program is a BPF Loader Upgradeable program.
    let program_account = context
        .banks_client
        .get_account(program_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());

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
