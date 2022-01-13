//! Example Rust-based BPF program that queries sibling instructions

#![cfg(feature = "program")]

use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    instruction::{get_invoke_depth, get_processed_sibling_instruction, AccountMeta, Instruction},
    msg,
    program::invoke,
    pubkey::Pubkey,
};

entrypoint!(process_instruction);
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

    // TODO shouldn't this be 1?
    assert_eq!(2, get_invoke_depth());
    assert_eq!(
        get_processed_sibling_instruction(0),
        Some((2, sibling_instruction0))
    );
    assert_eq!(
        get_processed_sibling_instruction(1),
        Some((2, sibling_instruction1))
    );
    assert_eq!(
        get_processed_sibling_instruction(2),
        Some((2, sibling_instruction2))
    );
    assert_eq!(get_processed_sibling_instruction(3), None);

    Ok(())
}
