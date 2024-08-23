use {
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        account_info::AccountInfo,
        entrypoint::ProgramResult,
        instruction::{Instruction, InstructionError},
        pubkey::Pubkey,
        signature::Signer,
        transaction::{Transaction, TransactionError},
    },
};

fn panic(_program_id: &Pubkey, _accounts: &[AccountInfo], _input: &[u8]) -> ProgramResult {
    panic!("I panicked");
}

#[tokio::test]
async fn panic_test() {
    let program_id = Pubkey::new_unique();

    let program_test = ProgramTest::new("panic", program_id, processor!(panic));

    let context = program_test.start_with_context().await;

    let instruction = Instruction::new_with_bytes(program_id, &[], vec![]);

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    assert_eq!(
        context
            .banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::ProgramFailedToComplete)
    );
}
