//! Example Rust-based BPF program that creates large instructions

use {
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program_error::ProgramError,
        pubkey::Pubkey,
    },
    std::{convert::TryInto, mem::size_of},
};

pub fn create_padded_instruction(
    program_id: Pubkey,
    instruction: Instruction,
    padding_accounts: Vec<AccountMeta>,
    padding_data: u32,
) -> Result<Instruction, ProgramError> {
    // The format for instruction data goes:
    // * 4 bytes for the number of accounts required
    // * 4 bytes for the size of the data required
    // * the actual instruction data
    // * additional bytes are all padding
    let mut data = Vec::with_capacity(
        size_of::<u32>() + size_of::<u32>() + instruction.data.len() + padding_data as usize,
    );
    let num_accounts: u32 = instruction
        .accounts
        .len()
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    data.extend(num_accounts.to_le_bytes().into_iter());

    let data_size: u32 = instruction
        .data
        .len()
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    data.extend(data_size.to_le_bytes().into_iter());

    data.extend(instruction.data.into_iter());

    for i in 0..padding_data {
        data.push((i % u8::MAX as u32) as u8);
    }

    // The format for account data goes:
    // * first account is the program to call into
    // * following accounts are the one required for the CPI
    // * additional accounts may be included as padding or to test loading / locks
    let mut accounts = Vec::with_capacity(instruction.accounts.len() + 1 + padding_accounts.len());
    accounts.extend(instruction.accounts.into_iter());
    accounts.push(AccountMeta::new_readonly(instruction.program_id, false));
    accounts.extend(padding_accounts.into_iter());

    Ok(Instruction {
        program_id,
        accounts,
        data,
    })
}
