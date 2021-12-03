//! Example Rust-based BPF sanity program that finalizes a BPF program

#![allow(unreachable_code)]

use solana_program::{
    account_info::AccountInfo, bpf_loader, entrypoint, entrypoint::ProgramResult,
    loader_instruction, msg, program::invoke, pubkey::Pubkey,
};

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    _instruction_data: &[u8],
) -> ProgramResult {
    msg!("Finalize a program");
    invoke(
        &loader_instruction::finalize(&accounts[0].key.clone(), &bpf_loader::id()),
        accounts,
    )?;
    msg!("check executable");
    assert!(accounts[0].executable);
    Ok(())
}
