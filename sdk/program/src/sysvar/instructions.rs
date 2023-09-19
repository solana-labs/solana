//! The serialized instructions of the current transaction.
//!
//! The _instructions sysvar_ provides access to the serialized instruction data
//! for the currently-running transaction. This allows for [instruction
//! introspection][in], which is required for correctly interoperating with
//! native programs like the [secp256k1] and [ed25519] programs.
//!
//! [in]: https://docs.solana.com/implemented-proposals/instruction_introspection
//! [secp256k1]: crate::secp256k1_program
//! [ed25519]: crate::ed25519_program
//!
//! Unlike other sysvars, the data in the instructions sysvar is not accessed
//! through a type that implements the [`Sysvar`] trait. Instead, the
//! instruction sysvar is accessed through several free functions within this
//! module.
//!
//! [`Sysvar`]: crate::sysvar::Sysvar
//!
//! See also the Solana [documentation on the instructions sysvar][sdoc].
//!
//! [sdoc]: https://docs.solana.com/developing/runtime-facilities/sysvars#instructions
//!
//! # Examples
//!
//! For a complete example of how the instructions sysvar is used see the
//! documentation for [`secp256k1_instruction`] in the `solana-sdk` crate.
//!
//! [`secp256k1_instruction`]: https://docs.rs/solana-sdk/latest/solana_sdk/secp256k1_instruction/index.html

#![allow(clippy::arithmetic_side_effects)]

use crate::{
    account_info::AccountInfo,
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
    sanitize::SanitizeError,
    serialize_utils::{read_pubkey, read_slice, read_u16, read_u8},
};
#[cfg(not(target_os = "solana"))]
use {
    crate::serialize_utils::{append_slice, append_u16, append_u8},
    bitflags::bitflags,
};

/// Instructions sysvar, dummy type.
///
/// This type exists for consistency with other sysvar modules, but is a dummy
/// type that does not contain sysvar data. It implements the [`SysvarId`] trait
/// but does not implement the [`Sysvar`] trait.
///
/// [`SysvarId`]: crate::sysvar::SysvarId
/// [`Sysvar`]: crate::sysvar::Sysvar
///
/// Use the free functions in this module to access the instructions sysvar.
pub struct Instructions();

crate::declare_sysvar_id!("Sysvar1nstructions1111111111111111111111111", Instructions);

/// Construct the account data for the instructions sysvar.
///
/// This function is used by the runtime and not available to Solana programs.
#[cfg(not(target_os = "solana"))]
pub fn construct_instructions_data(instructions: &[BorrowedInstruction]) -> Vec<u8> {
    let mut data = serialize_instructions(instructions);
    // add room for current instruction index.
    data.resize(data.len() + 2, 0);

    data
}

/// Borrowed version of `AccountMeta`.
///
/// This struct is used by the runtime when constructing the sysvar. It is not
/// useful to Solana programs.
pub struct BorrowedAccountMeta<'a> {
    pub pubkey: &'a Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

/// Borrowed version of `Instruction`.
///
/// This struct is used by the runtime when constructing the sysvar. It is not
/// useful to Solana programs.
pub struct BorrowedInstruction<'a> {
    pub program_id: &'a Pubkey,
    pub accounts: Vec<BorrowedAccountMeta<'a>>,
    pub data: &'a [u8],
}

#[cfg(not(target_os = "solana"))]
bitflags! {
    struct InstructionsSysvarAccountMeta: u8 {
        const IS_SIGNER = 0b00000001;
        const IS_WRITABLE = 0b00000010;
    }
}

// First encode the number of instructions:
// [0..2 - num_instructions
//
// Then a table of offsets of where to find them in the data
//  3..2 * num_instructions table of instruction offsets
//
// Each instruction is then encoded as:
//   0..2 - num_accounts
//   2 - meta_byte -> (bit 0 signer, bit 1 is_writable)
//   3..35 - pubkey - 32 bytes
//   35..67 - program_id
//   67..69 - data len - u16
//   69..data_len - data
#[cfg(not(target_os = "solana"))]
fn serialize_instructions(instructions: &[BorrowedInstruction]) -> Vec<u8> {
    // 64 bytes is a reasonable guess, calculating exactly is slower in benchmarks
    let mut data = Vec::with_capacity(instructions.len() * (32 * 2));
    append_u16(&mut data, instructions.len() as u16);
    for _ in 0..instructions.len() {
        append_u16(&mut data, 0);
    }

    for (i, instruction) in instructions.iter().enumerate() {
        let start_instruction_offset = data.len() as u16;
        let start = 2 + (2 * i);
        data[start..start + 2].copy_from_slice(&start_instruction_offset.to_le_bytes());
        append_u16(&mut data, instruction.accounts.len() as u16);
        for account_meta in &instruction.accounts {
            let mut account_meta_flags = InstructionsSysvarAccountMeta::empty();
            if account_meta.is_signer {
                account_meta_flags |= InstructionsSysvarAccountMeta::IS_SIGNER;
            }
            if account_meta.is_writable {
                account_meta_flags |= InstructionsSysvarAccountMeta::IS_WRITABLE;
            }
            append_u8(&mut data, account_meta_flags.bits());
            append_slice(&mut data, account_meta.pubkey.as_ref());
        }

        append_slice(&mut data, instruction.program_id.as_ref());
        append_u16(&mut data, instruction.data.len() as u16);
        append_slice(&mut data, instruction.data);
    }
    data
}

/// Load the current `Instruction`'s index in the currently executing
/// `Transaction`.
///
/// `data` is the instructions sysvar account data.
#[deprecated(
    since = "1.8.0",
    note = "Unsafe because the sysvar accounts address is not checked, please use `load_current_index_checked` instead"
)]
pub fn load_current_index(data: &[u8]) -> u16 {
    let mut instr_fixed_data = [0u8; 2];
    let len = data.len();
    instr_fixed_data.copy_from_slice(&data[len - 2..len]);
    u16::from_le_bytes(instr_fixed_data)
}

/// Load the current `Instruction`'s index in the currently executing
/// `Transaction`.
///
/// # Errors
///
/// Returns [`ProgramError::UnsupportedSysvar`] if the given account's ID is not equal to [`ID`].
pub fn load_current_index_checked(
    instruction_sysvar_account_info: &AccountInfo,
) -> Result<u16, ProgramError> {
    if !check_id(instruction_sysvar_account_info.key) {
        return Err(ProgramError::UnsupportedSysvar);
    }

    let instruction_sysvar = instruction_sysvar_account_info.try_borrow_data()?;
    let mut instr_fixed_data = [0u8; 2];
    let len = instruction_sysvar.len();
    instr_fixed_data.copy_from_slice(&instruction_sysvar[len - 2..len]);
    Ok(u16::from_le_bytes(instr_fixed_data))
}

/// Store the current `Instruction`'s index in the instructions sysvar data.
pub fn store_current_index(data: &mut [u8], instruction_index: u16) {
    let last_index = data.len() - 2;
    data[last_index..last_index + 2].copy_from_slice(&instruction_index.to_le_bytes());
}

fn deserialize_instruction(index: usize, data: &[u8]) -> Result<Instruction, SanitizeError> {
    const IS_SIGNER_BIT: usize = 0;
    const IS_WRITABLE_BIT: usize = 1;

    let mut current = 0;
    let num_instructions = read_u16(&mut current, data)?;
    if index >= num_instructions as usize {
        return Err(SanitizeError::IndexOutOfBounds);
    }

    // index into the instruction byte-offset table.
    current += index * 2;
    let start = read_u16(&mut current, data)?;

    current = start as usize;
    let num_accounts = read_u16(&mut current, data)?;
    let mut accounts = Vec::with_capacity(num_accounts as usize);
    for _ in 0..num_accounts {
        let meta_byte = read_u8(&mut current, data)?;
        let mut is_signer = false;
        let mut is_writable = false;
        if meta_byte & (1 << IS_SIGNER_BIT) != 0 {
            is_signer = true;
        }
        if meta_byte & (1 << IS_WRITABLE_BIT) != 0 {
            is_writable = true;
        }
        let pubkey = read_pubkey(&mut current, data)?;
        accounts.push(AccountMeta {
            pubkey,
            is_signer,
            is_writable,
        });
    }
    let program_id = read_pubkey(&mut current, data)?;
    let data_len = read_u16(&mut current, data)?;
    let data = read_slice(&mut current, data, data_len as usize)?;
    Ok(Instruction {
        program_id,
        accounts,
        data,
    })
}

/// Load an `Instruction` in the currently executing `Transaction` at the
/// specified index.
///
/// `data` is the instructions sysvar account data.
#[deprecated(
    since = "1.8.0",
    note = "Unsafe because the sysvar accounts address is not checked, please use `load_instruction_at_checked` instead"
)]
pub fn load_instruction_at(index: usize, data: &[u8]) -> Result<Instruction, SanitizeError> {
    deserialize_instruction(index, data)
}

/// Load an `Instruction` in the currently executing `Transaction` at the
/// specified index.
///
/// # Errors
///
/// Returns [`ProgramError::UnsupportedSysvar`] if the given account's ID is not equal to [`ID`].
pub fn load_instruction_at_checked(
    index: usize,
    instruction_sysvar_account_info: &AccountInfo,
) -> Result<Instruction, ProgramError> {
    if !check_id(instruction_sysvar_account_info.key) {
        return Err(ProgramError::UnsupportedSysvar);
    }

    let instruction_sysvar = instruction_sysvar_account_info.try_borrow_data()?;
    deserialize_instruction(index, &instruction_sysvar).map_err(|err| match err {
        SanitizeError::IndexOutOfBounds => ProgramError::InvalidArgument,
        _ => ProgramError::InvalidInstructionData,
    })
}

/// Returns the `Instruction` relative to the current `Instruction` in the
/// currently executing `Transaction`.
///
/// # Errors
///
/// Returns [`ProgramError::UnsupportedSysvar`] if the given account's ID is not equal to [`ID`].
pub fn get_instruction_relative(
    index_relative_to_current: i64,
    instruction_sysvar_account_info: &AccountInfo,
) -> Result<Instruction, ProgramError> {
    if !check_id(instruction_sysvar_account_info.key) {
        return Err(ProgramError::UnsupportedSysvar);
    }

    let instruction_sysvar = instruction_sysvar_account_info.data.borrow();
    #[allow(deprecated)]
    let current_index = load_current_index(&instruction_sysvar) as i64;
    let index = current_index.saturating_add(index_relative_to_current);
    if index < 0 {
        return Err(ProgramError::InvalidArgument);
    }
    #[allow(deprecated)]
    load_instruction_at(
        current_index.saturating_add(index_relative_to_current) as usize,
        &instruction_sysvar,
    )
    .map_err(|err| match err {
        SanitizeError::IndexOutOfBounds => ProgramError::InvalidArgument,
        _ => ProgramError::InvalidInstructionData,
    })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            instruction::AccountMeta,
            message::{Message as LegacyMessage, SanitizedMessage},
            pubkey::Pubkey,
        },
        std::convert::TryFrom,
    };

    #[test]
    fn test_load_store_instruction() {
        let mut data = [4u8; 10];
        store_current_index(&mut data, 3);
        #[allow(deprecated)]
        let index = load_current_index(&data);
        assert_eq!(index, 3);
        assert_eq!([4u8; 8], data[0..8]);
    }

    #[test]
    fn test_load_instruction_at_checked() {
        let instruction0 = Instruction::new_with_bincode(
            Pubkey::new_unique(),
            &0,
            vec![AccountMeta::new(Pubkey::new_unique(), false)],
        );
        let instruction1 = Instruction::new_with_bincode(
            Pubkey::new_unique(),
            &0,
            vec![AccountMeta::new(Pubkey::new_unique(), false)],
        );
        let sanitized_message = SanitizedMessage::try_from(LegacyMessage::new(
            &[instruction0.clone(), instruction1.clone()],
            Some(&Pubkey::new_unique()),
        ))
        .unwrap();

        let key = id();
        let mut lamports = 0;
        let mut data = construct_instructions_data(&sanitized_message.decompile_instructions());
        let owner = crate::sysvar::id();
        let mut account_info = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );

        assert_eq!(
            instruction0,
            load_instruction_at_checked(0, &account_info).unwrap()
        );
        assert_eq!(
            instruction1,
            load_instruction_at_checked(1, &account_info).unwrap()
        );
        assert_eq!(
            Err(ProgramError::InvalidArgument),
            load_instruction_at_checked(2, &account_info)
        );

        let key = Pubkey::new_unique();
        account_info.key = &key;
        assert_eq!(
            Err(ProgramError::UnsupportedSysvar),
            load_instruction_at_checked(2, &account_info)
        );
    }

    #[test]
    fn test_load_current_index_checked() {
        let instruction0 = Instruction::new_with_bincode(
            Pubkey::new_unique(),
            &0,
            vec![AccountMeta::new(Pubkey::new_unique(), false)],
        );
        let instruction1 = Instruction::new_with_bincode(
            Pubkey::new_unique(),
            &0,
            vec![AccountMeta::new(Pubkey::new_unique(), false)],
        );
        let sanitized_message = SanitizedMessage::try_from(LegacyMessage::new(
            &[instruction0, instruction1],
            Some(&Pubkey::new_unique()),
        ))
        .unwrap();

        let key = id();
        let mut lamports = 0;
        let mut data = construct_instructions_data(&sanitized_message.decompile_instructions());
        store_current_index(&mut data, 1);
        let owner = crate::sysvar::id();
        let mut account_info = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );

        assert_eq!(1, load_current_index_checked(&account_info).unwrap());
        {
            let mut data = account_info.try_borrow_mut_data().unwrap();
            store_current_index(&mut data, 0);
        }
        assert_eq!(0, load_current_index_checked(&account_info).unwrap());

        let key = Pubkey::new_unique();
        account_info.key = &key;
        assert_eq!(
            Err(ProgramError::UnsupportedSysvar),
            load_current_index_checked(&account_info)
        );
    }

    #[test]
    fn test_get_instruction_relative() {
        let instruction0 = Instruction::new_with_bincode(
            Pubkey::new_unique(),
            &0,
            vec![AccountMeta::new(Pubkey::new_unique(), false)],
        );
        let instruction1 = Instruction::new_with_bincode(
            Pubkey::new_unique(),
            &0,
            vec![AccountMeta::new(Pubkey::new_unique(), false)],
        );
        let instruction2 = Instruction::new_with_bincode(
            Pubkey::new_unique(),
            &0,
            vec![AccountMeta::new(Pubkey::new_unique(), false)],
        );
        let sanitized_message = SanitizedMessage::try_from(LegacyMessage::new(
            &[
                instruction0.clone(),
                instruction1.clone(),
                instruction2.clone(),
            ],
            Some(&Pubkey::new_unique()),
        ))
        .unwrap();

        let key = id();
        let mut lamports = 0;
        let mut data = construct_instructions_data(&sanitized_message.decompile_instructions());
        store_current_index(&mut data, 1);
        let owner = crate::sysvar::id();
        let mut account_info = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );

        assert_eq!(
            Err(ProgramError::InvalidArgument),
            get_instruction_relative(-2, &account_info)
        );
        assert_eq!(
            instruction0,
            get_instruction_relative(-1, &account_info).unwrap()
        );
        assert_eq!(
            instruction1,
            get_instruction_relative(0, &account_info).unwrap()
        );
        assert_eq!(
            instruction2,
            get_instruction_relative(1, &account_info).unwrap()
        );
        assert_eq!(
            Err(ProgramError::InvalidArgument),
            get_instruction_relative(2, &account_info)
        );
        {
            let mut data = account_info.try_borrow_mut_data().unwrap();
            store_current_index(&mut data, 0);
        }
        assert_eq!(
            Err(ProgramError::InvalidArgument),
            get_instruction_relative(-1, &account_info)
        );
        assert_eq!(
            instruction0,
            get_instruction_relative(0, &account_info).unwrap()
        );
        assert_eq!(
            instruction1,
            get_instruction_relative(1, &account_info).unwrap()
        );
        assert_eq!(
            instruction2,
            get_instruction_relative(2, &account_info).unwrap()
        );
        assert_eq!(
            Err(ProgramError::InvalidArgument),
            get_instruction_relative(3, &account_info)
        );

        let key = Pubkey::new_unique();
        account_info.key = &key;
        assert_eq!(
            Err(ProgramError::UnsupportedSysvar),
            get_instruction_relative(0, &account_info)
        );
    }

    #[test]
    fn test_serialize_instructions() {
        let program_id0 = Pubkey::new_unique();
        let program_id1 = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let id1 = Pubkey::new_unique();
        let id2 = Pubkey::new_unique();
        let id3 = Pubkey::new_unique();
        let instructions = vec![
            Instruction::new_with_bincode(program_id0, &0, vec![AccountMeta::new(id0, false)]),
            Instruction::new_with_bincode(program_id0, &0, vec![AccountMeta::new(id1, true)]),
            Instruction::new_with_bincode(
                program_id1,
                &0,
                vec![AccountMeta::new_readonly(id2, false)],
            ),
            Instruction::new_with_bincode(
                program_id1,
                &0,
                vec![AccountMeta::new_readonly(id3, true)],
            ),
        ];

        let message = LegacyMessage::new(&instructions, Some(&id1));
        let sanitized_message = SanitizedMessage::try_from(message).unwrap();
        let serialized = serialize_instructions(&sanitized_message.decompile_instructions());

        // assert that deserialize_instruction is compatible with SanitizedMessage::serialize_instructions
        for (i, instruction) in instructions.iter().enumerate() {
            assert_eq!(
                deserialize_instruction(i, &serialized).unwrap(),
                *instruction
            );
        }
    }

    #[test]
    fn test_decompile_instructions_out_of_bounds() {
        let program_id0 = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let id1 = Pubkey::new_unique();
        let instructions = vec![
            Instruction::new_with_bincode(program_id0, &0, vec![AccountMeta::new(id0, false)]),
            Instruction::new_with_bincode(program_id0, &0, vec![AccountMeta::new(id1, true)]),
        ];

        let message =
            SanitizedMessage::try_from(LegacyMessage::new(&instructions, Some(&id1))).unwrap();
        let serialized = serialize_instructions(&message.decompile_instructions());
        assert_eq!(
            deserialize_instruction(instructions.len(), &serialized).unwrap_err(),
            SanitizeError::IndexOutOfBounds,
        );
    }
}
