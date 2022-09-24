use {
    solana_sdk::{
        declare_id,
        instruction::{AccountMeta, Instruction},
        program_error::ProgramError,
        pubkey::Pubkey,
        syscalls::{MAX_CPI_ACCOUNT_INFOS, MAX_CPI_INSTRUCTION_DATA_LEN},
    },
    std::{convert::TryInto, mem::size_of},
};

pub struct InstructionPaddingConfig {
    pub program_id: Pubkey,
    pub data_size: u32,
}

declare_id!("iXpADd6AW1k5FaaXum5qHbSqyd7TtoN6AD7suVa83MF");

pub fn create_padded_instruction(
    program_id: Pubkey,
    instruction: Instruction,
    padding_accounts: Vec<AccountMeta>,
    padding_data: u32,
) -> Result<Instruction, ProgramError> {
    // The format for instruction data goes:
    // * 1 byte for the instruction type
    // * 4 bytes for the number of accounts required
    // * 4 bytes for the size of the data required
    // * the actual instruction data
    // * additional bytes are all padding
    let data_size = size_of::<u8>()
        + size_of::<u32>()
        + size_of::<u32>()
        + instruction.data.len()
        + padding_data as usize;
    // crude, but can find a potential issue right away
    if instruction.data.len() > MAX_CPI_INSTRUCTION_DATA_LEN as usize {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut data = Vec::with_capacity(data_size);
    data.push(1);
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
    // * accounts required for the CPI
    // * program account to call into
    // * additional accounts may be included as padding or to test loading / locks
    let num_accounts = instruction.accounts.len() + 1 + padding_accounts.len();
    if num_accounts > MAX_CPI_ACCOUNT_INFOS {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut accounts = Vec::with_capacity(num_accounts);
    accounts.extend(instruction.accounts.into_iter());
    accounts.push(AccountMeta::new_readonly(instruction.program_id, false));
    accounts.extend(padding_accounts.into_iter());

    Ok(Instruction {
        program_id,
        accounts,
        data,
    })
}
