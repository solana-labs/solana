//! Example Rust-based BPF program that creates large instructions

use {
    num_enum::{IntoPrimitive, TryFromPrimitive},
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program_error::ProgramError,
        pubkey::Pubkey,
        syscalls::{MAX_CPI_ACCOUNT_INFOS, MAX_CPI_INSTRUCTION_DATA_LEN},
    },
    std::{convert::TryInto, mem::size_of},
};

/// Instructions supported by the padding program, which takes in additional
/// account data or accounts and does nothing with them. It's meant for testing
/// larger transactions with bench-tps.
#[derive(Clone, Copy, Debug, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum PaddingInstruction {
    /// Does no work, but can accept huge data streams
    Noop,
    /// Wraps the provided instruction, calling the provided program via CPI
    ///
    /// Accounts expected by this instruction:
    ///
    /// * All accounts required for the inner instruction
    /// * The program invoked by the inner instruction
    /// * Additional padding accounts
    ///
    /// Data expected by this instruction:
    /// * WrapData
    Wrap,
}

/// Data wrapping any inner instruction
pub struct WrapData<'a> {
    /// Number of accounts required by the inner instruction
    pub num_accounts: u32,
    /// the size of the inner instruction data
    pub instruction_size: u32,
    /// actual inner instruction data
    pub instruction_data: &'a [u8],
    // additional padding bytes come after, not captured in this struct
}

const U32_BYTES: usize = 4;
fn unpack_u32(input: &[u8]) -> Result<(u32, &[u8]), ProgramError> {
    let value = input
        .get(..U32_BYTES)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(ProgramError::InvalidInstructionData)?;
    Ok((value, &input[U32_BYTES..]))
}

impl<'a> WrapData<'a> {
    /// Unpacks instruction data
    pub fn unpack(data: &'a [u8]) -> Result<Self, ProgramError> {
        let (num_accounts, rest) = unpack_u32(data)?;
        let (instruction_size, rest) = unpack_u32(rest)?;

        let (instruction_data, _rest) = rest.split_at(instruction_size as usize);
        Ok(Self {
            num_accounts,
            instruction_size,
            instruction_data,
        })
    }
}

pub fn noop(
    program_id: Pubkey,
    padding_accounts: Vec<AccountMeta>,
    padding_data: u32,
) -> Result<Instruction, ProgramError> {
    let total_data_size = size_of::<u8>() + padding_data as usize;
    // crude, but can find a potential issue right away
    if total_data_size > MAX_CPI_INSTRUCTION_DATA_LEN as usize {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut data = Vec::with_capacity(total_data_size);
    data.push(PaddingInstruction::Noop.into());
    for i in 0..padding_data {
        data.push((i % u8::MAX as u32) as u8);
    }

    let num_accounts = 1 + padding_accounts.len();
    if num_accounts > MAX_CPI_ACCOUNT_INFOS {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut accounts = Vec::with_capacity(num_accounts);
    accounts.extend(padding_accounts.into_iter());

    Ok(Instruction {
        program_id,
        accounts,
        data,
    })
}

pub fn wrap_instruction(
    program_id: Pubkey,
    instruction: Instruction,
    padding_accounts: Vec<AccountMeta>,
    padding_data: u32,
) -> Result<Instruction, ProgramError> {
    let total_data_size = size_of::<u8>()
        + size_of::<u32>()
        + size_of::<u32>()
        + instruction.data.len()
        + padding_data as usize;
    // crude, but can find a potential issue right away
    if total_data_size > MAX_CPI_INSTRUCTION_DATA_LEN as usize {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut data = Vec::with_capacity(total_data_size);
    data.push(PaddingInstruction::Wrap.into());
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
