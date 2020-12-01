//! @brief Example Rust-based BPF program that tests duplicate accounts passed via accounts

extern crate solana_program;
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, msg,
    program_error::ProgramError, pubkey::Pubkey,
};

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data[0] {
        1 => {
            msg!("modify first account data");
            accounts[2].data.borrow_mut()[0] = 1;
        }
        2 => {
            msg!("modify first account data");
            accounts[3].data.borrow_mut()[0] = 2;
        }
        3 => {
            msg!("modify both account data");
            accounts[2].data.borrow_mut()[0] += 1;
            accounts[3].data.borrow_mut()[0] += 2;
        }
        4 => {
            msg!("modify first account lamports");
            **accounts[1].lamports.borrow_mut() -= 1;
            **accounts[2].lamports.borrow_mut() += 1;
        }
        5 => {
            msg!("modify first account lamports");
            **accounts[1].lamports.borrow_mut() -= 2;
            **accounts[3].lamports.borrow_mut() += 2;
        }
        6 => {
            msg!("modify both account lamports");
            **accounts[1].lamports.borrow_mut() -= 3;
            **accounts[2].lamports.borrow_mut() += 1;
            **accounts[3].lamports.borrow_mut() += 2;
        }
        _ => {
            msg!("Unrecognized command");
            return Err(ProgramError::InvalidArgument);
        }
    }
    Ok(())
}
