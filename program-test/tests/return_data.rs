use {
    assert_matches::assert_matches,
    solana_banks_client::BanksClientError,
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        account_info::{next_account_info, AccountInfo},
        commitment_config::CommitmentLevel,
        entrypoint::ProgramResult,
        instruction::{AccountMeta, Instruction},
        msg,
        program::{get_return_data, invoke, set_return_data},
        program_error::ProgramError,
        pubkey::Pubkey,
        signature::Signer,
        transaction::Transaction,
        transaction_context::TransactionReturnData,
    },
    std::str::from_utf8,
};

// Process instruction to get return data from another program
fn get_return_data_process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    msg!("Processing get_return_data instruction before CPI");
    let account_info_iter = &mut accounts.iter();
    let invoked_program_info = next_account_info(account_info_iter)?;
    invoke(
        &Instruction {
            program_id: *invoked_program_info.key,
            accounts: vec![],
            data: input.to_vec(),
        },
        &[invoked_program_info.clone()],
    )?;
    let return_data = get_return_data().unwrap();
    msg!("Processing get_return_data instruction after CPI");
    msg!("{}", from_utf8(&return_data.1).unwrap());
    assert_eq!(return_data.1, input.to_vec());
    Ok(())
}

// Process instruction to echo input back to another program
#[allow(clippy::unnecessary_wraps)]
fn set_return_data_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    msg!("Processing invoked instruction before set_return_data");
    set_return_data(input);
    msg!("Processing invoked instruction after set_return_data");
    Ok(())
}

#[tokio::test]
async fn return_data() {
    let get_return_data_program_id = Pubkey::new_unique();
    let mut program_test = ProgramTest::new(
        "get_return_data",
        get_return_data_program_id,
        processor!(get_return_data_process_instruction),
    );
    let set_return_data_program_id = Pubkey::new_unique();
    program_test.add_program(
        "set_return_data",
        set_return_data_program_id,
        processor!(set_return_data_process_instruction),
    );

    let mut context = program_test.start_with_context().await;
    let instructions = vec![Instruction {
        program_id: get_return_data_program_id,
        accounts: vec![AccountMeta::new_readonly(set_return_data_program_id, false)],
        data: vec![240, 159, 166, 150],
    }];

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

// Process instruction to echo input back to another program
#[allow(clippy::unnecessary_wraps)]
fn error_set_return_data_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    set_return_data(input);
    Err(ProgramError::InvalidInstructionData)
}

#[tokio::test]
async fn simulation_return_data() {
    let error_set_return_data_program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "error_set_return_data",
        error_set_return_data_program_id,
        processor!(error_set_return_data_process_instruction),
    );

    let mut context = program_test.start_with_context().await;
    let expected_data = vec![240, 159, 166, 150];
    let instructions = vec![Instruction {
        program_id: error_set_return_data_program_id,
        accounts: vec![],
        data: expected_data.clone(),
    }];

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    let error = context
        .banks_client
        .process_transaction_with_preflight_and_commitment(transaction, CommitmentLevel::Confirmed)
        .await
        .unwrap_err();
    assert_matches!(
        error,
        BanksClientError::SimulationError {
            return_data: Some(TransactionReturnData {
                program_id,
                data,
            }),
            ..
        } if program_id == error_set_return_data_program_id && data == expected_data
    );
}
