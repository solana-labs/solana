//! @brief Example Rust-based BPF program that exercises instruction introspection

extern crate solana_program;
use solana_program::{
    account_info::next_account_info, account_info::AccountInfo, entrypoint,
    entrypoint::ProgramResult, instruction::Instruction, log::sol_log_compute_units, msg,
    program::invoke_signed_with_budget, program_error::ProgramError, pubkey::Pubkey,
};

entrypoint!(process_instruction);
pub fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let inner_program_info = next_account_info(account_info_iter)?;
    // 7 * 25_000 = 175_000
    for _ in 0..7 {
        sol_log_compute_units();
        let ix = Instruction::new_with_bincode(*inner_program_info.key, &[0], vec![]);
        let ret =
            invoke_signed_with_budget(&ix, 25_000, &[inner_program_info.clone()], &vec![][..]);
        if let Err(ProgramError::ComputationalBudgetExceeded) = ret {
            msg!("inner CPI exceeded computational budget");
        } else {
            ret?;
        }
    }
    sol_log_compute_units();
    Ok(())
}
