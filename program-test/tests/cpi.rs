use {
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        account_info::{next_account_info, AccountInfo},
        entrypoint::ProgramResult,
        instruction::{AccountMeta, Instruction},
        msg,
        program::invoke,
        pubkey::Pubkey,
        signature::Signer,
        transaction::Transaction,
    },
};

// Process instruction to invoke into another program
fn invoker_process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    // if we can call `msg!` successfully, then InvokeContext exists as required
    msg!("Processing invoker instruction before CPI");
    let account_info_iter = &mut accounts.iter();
    let invoked_program_info = next_account_info(account_info_iter)?;
    invoke(
        &Instruction::new_with_bincode(*invoked_program_info.key, &[0], vec![]),
        &[invoked_program_info.clone()],
    )?;
    msg!("Processing invoker instruction after CPI");
    Ok(())
}

// Process instruction to be invoked by another program
#[allow(clippy::unnecessary_wraps)]
fn invoked_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    // if we can call `msg!` successfully, then InvokeContext exists as required
    msg!("Processing invoked instruction");
    Ok(())
}

#[tokio::test]
async fn cpi() {
    let invoker_program_id = Pubkey::new_unique();
    let mut program_test = ProgramTest::new(
        "invoker",
        invoker_program_id,
        processor!(invoker_process_instruction),
    );
    let invoked_program_id = Pubkey::new_unique();
    program_test.add_program(
        "invoked",
        invoked_program_id,
        processor!(invoked_process_instruction),
    );

    let mut context = program_test.start_with_context().await;
    let instructions = vec![Instruction::new_with_bincode(
        invoker_program_id,
        &[0],
        vec![AccountMeta::new_readonly(invoked_program_id, false)],
    )];

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
