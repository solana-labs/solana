//! @brief Example Rust-based BPF program calling get_packed_len

extern crate solana_program;
use {
    borsh::BorshSchema,
    solana_program::{
        account_info::AccountInfo, borsh::get_packed_len, entrypoint, entrypoint::ProgramResult,
        msg, pubkey::Pubkey,
    },
};

#[derive(BorshSchema)]
struct ExampleStruct {
    _data: u8,
}

entrypoint!(process_instruction);
#[allow(clippy::unnecessary_wraps)]
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let len = get_packed_len::<ExampleStruct>();
    msg!("{}", len);
    Ok(())
}
