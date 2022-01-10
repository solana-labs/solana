#![allow(clippy::integer_arithmetic)]
//! This account contains the serialized transaction instructions

use crate::{
    account_info::AccountInfo, instruction::Instruction, program_error::ProgramError,
    sanitize::SanitizeError,
};

// Instructions Sysvar, dummy type, use the associated helpers instead of the Sysvar trait
pub struct Instructions();

crate::declare_sysvar_id!("Sysvar1nstructions1111111111111111111111111", Instructions);

// Construct the account data for the Instruction sSysvar
#[cfg(not(target_arch = "bpf"))]
pub fn construct_instructions_data(message: &crate::message::SanitizedMessage) -> Vec<u8> {
    let mut data = message.serialize_instructions();
    // add room for current instruction index.
    data.resize(data.len() + 2, 0);

    data
}

/// Load the current `Instruction`'s index in the currently executing
/// `Transaction` from the Instructions Sysvar data
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
/// `Transaction`
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

/// Store the current `Instruction`'s index in the Instructions Sysvar data
pub fn store_current_index(data: &mut [u8], instruction_index: u16) {
    let last_index = data.len() - 2;
    data[last_index..last_index + 2].copy_from_slice(&instruction_index.to_le_bytes());
}

/// Load an `Instruction` in the currently executing `Transaction` at the
/// specified index
#[deprecated(
    since = "1.8.0",
    note = "Unsafe because the sysvar accounts address is not checked, please use `load_instruction_at_checked` instead"
)]
pub fn load_instruction_at(index: usize, data: &[u8]) -> Result<Instruction, SanitizeError> {
    crate::message::Message::deserialize_instruction(index, data)
}

/// Load an `Instruction` in the currently executing `Transaction` at the
/// specified index
pub fn load_instruction_at_checked(
    index: usize,
    instruction_sysvar_account_info: &AccountInfo,
) -> Result<Instruction, ProgramError> {
    if !check_id(instruction_sysvar_account_info.key) {
        return Err(ProgramError::UnsupportedSysvar);
    }

    let instruction_sysvar = instruction_sysvar_account_info.try_borrow_data()?;
    crate::message::Message::deserialize_instruction(index, &instruction_sysvar).map_err(|err| {
        match err {
            SanitizeError::IndexOutOfBounds => ProgramError::InvalidArgument,
            _ => ProgramError::InvalidInstructionData,
        }
    })
}

/// Returns the `Instruction` relative to the current `Instruction` in the
/// currently executing `Transaction`
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
        crate::{instruction::AccountMeta, message::Message, pubkey::Pubkey},
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
        let sanitized_message = crate::message::SanitizedMessage::try_from(Message::new(
            &[instruction0.clone(), instruction1.clone()],
            Some(&Pubkey::new_unique()),
        ))
        .unwrap();

        let key = id();
        let mut lamports = 0;
        let mut data = construct_instructions_data(&sanitized_message);
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
        let sanitized_message = crate::message::SanitizedMessage::try_from(Message::new(
            &[instruction0, instruction1],
            Some(&Pubkey::new_unique()),
        ))
        .unwrap();

        let key = id();
        let mut lamports = 0;
        let mut data = construct_instructions_data(&sanitized_message);
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
        let sanitized_message = crate::message::SanitizedMessage::try_from(Message::new(
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
        let mut data = construct_instructions_data(&sanitized_message);
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
}
