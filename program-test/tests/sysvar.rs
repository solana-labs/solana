use {
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        account_info::AccountInfo, clock::Clock, entrypoint::ProgramResult,
        epoch_schedule::EpochSchedule, instruction::Instruction, msg, pubkey::Pubkey, rent::Rent,
        signature::Signer, sysvar::Sysvar, transaction::Transaction,
    },
};

// Process instruction to invoke into another program
fn sysvar_getter_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    msg!("sysvar_getter");

    let clock = Clock::get()?;
    assert_eq!(42, clock.slot);

    let epoch_schedule = EpochSchedule::get()?;
    assert_eq!(epoch_schedule, EpochSchedule::default());

    let rent = Rent::get()?;
    assert_eq!(rent, Rent::default());

    Ok(())
}

#[tokio::test]
async fn get_sysvar() {
    let program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "sysvar_getter",
        program_id,
        processor!(sysvar_getter_process_instruction),
    );

    let mut context = program_test.start_with_context().await;
    context.warp_to_slot(42).unwrap();
    let instructions = vec![Instruction::new_with_bincode(program_id, &(), vec![])];

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
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
