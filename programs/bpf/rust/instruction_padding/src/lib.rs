use solana_program::{
    account_info::AccountInfo,
    declare_id,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
};

pub mod instructions;

declare_id!("padVgCLbVUvyfwBwjG2d1Pgy1fkWkpiV5oeHV7dxL52.json");

solana_program::entrypoint!(process_instruction);

const U32_BYTES: usize = 4;
fn unpack_u32_as_usize(input: &[u8]) -> Result<(usize, &[u8]), ProgramError> {
    let value = input
        .get(..U32_BYTES)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(ProgramError::InvalidInstructionData)?;
    Ok((value as usize, &input[U32_BYTES..]))
}

pub fn process_instruction(
    _program_id: &Pubkey,
    account_infos: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let (num_accounts, rest) = unpack_u32_as_usize(instruction_data)?;
    let (data_size, rest) = unpack_u32_as_usize(rest)?;

    let (raw_data, _rest) = rest.split_at(data_size);
    let mut data = Vec::with_capacity(data_size);
    data.extend_from_slice(raw_data);

    let program_id = *account_infos[num_accounts].key;

    let accounts = account_infos
        .iter()
        .take(num_accounts)
        .map(|a| AccountMeta {
            pubkey: *a.key,
            is_signer: a.is_signer,
            is_writable: a.is_writable,
        })
        .collect::<Vec<_>>();

    let instruction = Instruction {
        program_id,
        accounts,
        data,
    };

    invoke(&instruction, &account_infos[..num_accounts])?;

    Ok(())
}
