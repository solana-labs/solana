//! Example Rust-based BPF program that uses sol_get_processed_inner_instruction syscall

#![cfg(feature = "program")]

use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    instruction::{get_processed_inner_instruction, AccountMeta, Instruction},
    msg,
    program::invoke,
    pubkey::Pubkey,
};

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    msg!("inner");

    // account 0 is mint
    // account 1 is noop
    // account 2 is invoke_and_return

    let instruction3 = Instruction::new_with_bytes(*accounts[1].key, instruction_data, vec![]);
    let instruction2 = Instruction::new_with_bytes(
        *accounts[2].key,
        instruction_data,
        vec![AccountMeta::new_readonly(*accounts[1].key, false)],
    );
    let instruction1 = Instruction::new_with_bytes(
        *accounts[1].key,
        instruction_data,
        vec![
            AccountMeta::new_readonly(*accounts[0].key, true),
            AccountMeta::new_readonly(*accounts[1].key, false),
        ],
    );
    let instruction0 = Instruction::new_with_bytes(
        *accounts[1].key,
        instruction_data,
        vec![
            AccountMeta::new_readonly(*accounts[1].key, false),
            AccountMeta::new_readonly(*accounts[0].key, true),
        ],
    );

    invoke(&instruction2, accounts)?;
    invoke(&instruction1, accounts)?;
    invoke(&instruction0, accounts)?;

    assert_eq!(Some((1, instruction0)), get_processed_inner_instruction(0));
    assert_eq!(Some((1, instruction1)), get_processed_inner_instruction(1));
    assert_eq!(Some((1, instruction2)), get_processed_inner_instruction(2));
    assert_eq!(Some((2, instruction3)), get_processed_inner_instruction(3));
    assert!(get_processed_inner_instruction(4).is_none());

    Ok(())
}
