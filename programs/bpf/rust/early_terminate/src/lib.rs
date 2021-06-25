//! @brief Example Rust-based BPF program that exercises instruction introspection

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, program_error::ProgramError,
    program_inspection::sol_remaining_compute_units, pubkey::Pubkey,
};
entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let mut i = 0u32;
    for _ in 0..1_000_000 {
        if i % 1000 == 0 {
            if sol_remaining_compute_units() < 25_000 {
                break;
            }
        }
        i += 1;
    }

    if i % 123143 != 0 {
        return Err(ProgramError::Custom(i % 123143 as u32));
    }
    Ok(())
}
