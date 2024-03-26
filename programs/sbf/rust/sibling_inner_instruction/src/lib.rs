//! Example Rust-based SBF program that queries sibling instructions

#![cfg(feature = "program")]
#![allow(clippy::arithmetic_side_effects)]

use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    instruction::{
        get_processed_sibling_instruction, get_stack_height, AccountMeta, Instruction,
        TRANSACTION_LEVEL_STACK_HEIGHT,
    },
    msg,
    pubkey::Pubkey,
};

solana_program::entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    msg!("sibling inner");

    // account 0 is mint
    // account 1 is noop
    // account 2 is invoke_and_return

    // Check sibling instructions

    let sibling_instruction2 = Instruction::new_with_bytes(
        *accounts[2].key,
        &[3],
        vec![AccountMeta::new_readonly(*accounts[1].key, false)],
    );
    let sibling_instruction1 = Instruction::new_with_bytes(
        *accounts[1].key,
        &[2],
        vec![
            AccountMeta::new_readonly(*accounts[0].key, true),
            AccountMeta::new_readonly(*accounts[1].key, false),
        ],
    );
    let sibling_instruction0 = Instruction::new_with_bytes(
        *accounts[1].key,
        &[1],
        vec![
            AccountMeta::new_readonly(*accounts[1].key, false),
            AccountMeta::new_readonly(*accounts[0].key, true),
        ],
    );

    assert_eq!(TRANSACTION_LEVEL_STACK_HEIGHT + 1, get_stack_height());
    assert_eq!(
        get_processed_sibling_instruction(0),
        Some(sibling_instruction0)
    );
    assert_eq!(
        get_processed_sibling_instruction(1),
        Some(sibling_instruction1)
    );
    assert_eq!(
        get_processed_sibling_instruction(2),
        Some(sibling_instruction2)
    );
    assert_eq!(get_processed_sibling_instruction(3), None);

    Ok(())
}
