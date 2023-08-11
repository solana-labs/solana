use {
    solana_program_test::ProgramTest,
    solana_sdk::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
        sysvar::rent,
        transaction::Transaction,
    },
};

#[should_panic]
#[test]
fn overflow_compute_units() {
    let mut program_test = ProgramTest::default();
    program_test.set_compute_max_units(i64::MAX as u64 + 1);
}

#[tokio::test]
async fn max_compute_units() {
    let mut program_test = ProgramTest::default();
    program_test.set_compute_max_units(i64::MAX as u64);
    let mut context = program_test.start_with_context().await;

    // Invalid compute unit maximums are only triggered by BPF programs, so send
    // a valid instruction into a BPF program to make sure the issue doesn't
    // manifest.
    let token_2022_id = Pubkey::try_from("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap();
    let mint = Keypair::new();
    let rent = context.banks_client.get_rent().await.unwrap();
    let space = 82;
    let transaction = Transaction::new_signed_with_payer(
        &[
            system_instruction::create_account(
                &context.payer.pubkey(),
                &mint.pubkey(),
                rent.minimum_balance(space),
                space as u64,
                &token_2022_id,
            ),
            Instruction::new_with_bytes(
                token_2022_id,
                &[0; 35], // initialize mint
                vec![
                    AccountMeta::new(mint.pubkey(), false),
                    AccountMeta::new_readonly(rent::id(), false),
                ],
            ),
        ],
        Some(&context.payer.pubkey()),
        &[&context.payer, &mint],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}
