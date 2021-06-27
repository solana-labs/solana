use {
    cpi_budgeted_early_termination::process_instruction,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
    },
    solana_program_test::*,
    solana_sdk::{
        account_info::AccountInfo, entrypoint::ProgramResult, log::sol_log_compute_units,
        signature::Signer, transaction::Transaction,
    },
    std::str::FromStr,
};

pub fn process_instruction_inner(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let mut i = 0u32;
    for _ in 0..10_000 {
        if i % 500 == 0 {
            sol_log_compute_units();
        }
        i += 1;
    }
    Ok(())
}

#[tokio::test]
async fn test_cpi_budgeted_early_termination() {
    let program_id_inner = Pubkey::from_str("inner11111111111111111111111111111111111111").unwrap();
    let mut prog_test = ProgramTest::new(
        "inner",
        program_id_inner,
        processor!(process_instruction_inner),
    );
    let program_id = Pubkey::from_str("outer11111111111111111111111111111111111111").unwrap();
    prog_test.add_program(
        "cpi_budgeted_early_termination",
        program_id,
        processor!(process_instruction),
    );

    let mut context = prog_test.start_with_context().await;

    let transaction = Transaction::new_signed_with_payer(
        &[Instruction::new_with_bincode(
            program_id,
            &[0],
            vec![AccountMeta::new_readonly(program_id_inner, false)],
        )],
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
