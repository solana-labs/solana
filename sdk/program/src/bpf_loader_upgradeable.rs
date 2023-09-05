//! An upgradeable BPF loader native program.
//!
//! The upgradeable BPF loader is responsible for deploying, upgrading, and
//! executing BPF programs. The upgradeable loader allows a program's authority
//! to update the program at any time. This ability breaks the "code is law"
//! contract that once a program is on-chain it is immutable. Because of this,
//! care should be taken before executing upgradeable programs which still have
//! a functioning authority. For more information refer to the
//! [`loader_upgradeable_instruction`] module.
//!
//! The `solana program deploy` CLI command uses the
//! upgradeable BPF loader. Calling `solana program deploy --final` deploys a
//! program that cannot be upgraded, but it does so by revoking the authority to
//! upgrade, not by using the non-upgradeable loader.
//!
//! [`loader_upgradeable_instruction`]: crate::loader_upgradeable_instruction

use crate::{
    instruction::{AccountMeta, Instruction, InstructionError},
    loader_upgradeable_instruction::UpgradeableLoaderInstruction,
    pubkey::Pubkey,
    system_instruction, sysvar,
};

crate::declare_id!("BPFLoaderUpgradeab1e11111111111111111111111");

/// Upgradeable loader account states
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy, AbiExample)]
pub enum UpgradeableLoaderState {
    /// Account is not initialized.
    Uninitialized,
    /// A Buffer account.
    Buffer {
        /// Authority address
        authority_address: Option<Pubkey>,
        // The raw program data follows this serialized structure in the
        // account's data.
    },
    /// An Program account.
    Program {
        /// Address of the ProgramData account.
        programdata_address: Pubkey,
    },
    // A ProgramData account.
    ProgramData {
        /// Slot that the program was last modified.
        slot: u64,
        /// Address of the Program's upgrade authority.
        upgrade_authority_address: Option<Pubkey>,
        // The raw program data follows this serialized structure in the
        // account's data.
    },
}
impl UpgradeableLoaderState {
    /// Size of a serialized program account.
    pub const fn size_of_uninitialized() -> usize {
        4 // see test_state_size_of_uninitialized
    }

    /// Size of a buffer account's serialized metadata.
    pub const fn size_of_buffer_metadata() -> usize {
        37 // see test_state_size_of_buffer_metadata
    }

    /// Size of a programdata account's serialized metadata.
    pub const fn size_of_programdata_metadata() -> usize {
        45 // see test_state_size_of_programdata_metadata
    }

    /// Size of a serialized program account.
    pub const fn size_of_program() -> usize {
        36 // see test_state_size_of_program
    }

    /// Size of a serialized buffer account.
    pub const fn size_of_buffer(program_len: usize) -> usize {
        Self::size_of_buffer_metadata().saturating_add(program_len)
    }

    /// Size of a serialized programdata account.
    pub const fn size_of_programdata(program_len: usize) -> usize {
        Self::size_of_programdata_metadata().saturating_add(program_len)
    }

    /// Length of a Buffer account's data.
    #[deprecated(since = "1.11.0", note = "Please use `size_of_buffer` instead")]
    pub fn buffer_len(program_len: usize) -> Result<usize, InstructionError> {
        Ok(Self::size_of_buffer(program_len))
    }

    /// Offset into the Buffer account's data of the program bits.
    #[deprecated(
        since = "1.11.0",
        note = "Please use `size_of_buffer_metadata` instead"
    )]
    pub fn buffer_data_offset() -> Result<usize, InstructionError> {
        Ok(Self::size_of_buffer_metadata())
    }

    /// Length of a Program account's data.
    #[deprecated(since = "1.11.0", note = "Please use `size_of_program` instead")]
    pub fn program_len() -> Result<usize, InstructionError> {
        Ok(Self::size_of_program())
    }

    /// Length of a ProgramData account's data.
    #[deprecated(since = "1.11.0", note = "Please use `size_of_programdata` instead")]
    pub fn programdata_len(program_len: usize) -> Result<usize, InstructionError> {
        Ok(Self::size_of_programdata(program_len))
    }

    /// Offset into the ProgramData account's data of the program bits.
    #[deprecated(
        since = "1.11.0",
        note = "Please use `size_of_programdata_metadata` instead"
    )]
    pub fn programdata_data_offset() -> Result<usize, InstructionError> {
        Ok(Self::size_of_programdata_metadata())
    }
}

/// Returns the instructions required to initialize a Buffer account.
pub fn create_buffer(
    payer_address: &Pubkey,
    buffer_address: &Pubkey,
    authority_address: &Pubkey,
    lamports: u64,
    program_len: usize,
) -> Result<Vec<Instruction>, InstructionError> {
    Ok(vec![
        system_instruction::create_account(
            payer_address,
            buffer_address,
            lamports,
            UpgradeableLoaderState::size_of_buffer(program_len) as u64,
            &id(),
        ),
        Instruction::new_with_bincode(
            id(),
            &UpgradeableLoaderInstruction::InitializeBuffer,
            vec![
                AccountMeta::new(*buffer_address, false),
                AccountMeta::new_readonly(*authority_address, false),
            ],
        ),
    ])
}

/// Returns the instructions required to write a chunk of program data to a
/// buffer account.
pub fn write(
    buffer_address: &Pubkey,
    authority_address: &Pubkey,
    offset: u32,
    bytes: Vec<u8>,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &UpgradeableLoaderInstruction::Write { offset, bytes },
        vec![
            AccountMeta::new(*buffer_address, false),
            AccountMeta::new_readonly(*authority_address, true),
        ],
    )
}

/// Returns the instructions required to deploy a program with a specified
/// maximum program length.  The maximum length must be large enough to
/// accommodate any future upgrades.
pub fn deploy_with_max_program_len(
    payer_address: &Pubkey,
    program_address: &Pubkey,
    buffer_address: &Pubkey,
    upgrade_authority_address: &Pubkey,
    program_lamports: u64,
    max_data_len: usize,
) -> Result<Vec<Instruction>, InstructionError> {
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    Ok(vec![
        system_instruction::create_account(
            payer_address,
            program_address,
            program_lamports,
            UpgradeableLoaderState::size_of_program() as u64,
            &id(),
        ),
        Instruction::new_with_bincode(
            id(),
            &UpgradeableLoaderInstruction::DeployWithMaxDataLen { max_data_len },
            vec![
                AccountMeta::new(*payer_address, true),
                AccountMeta::new(programdata_address, false),
                AccountMeta::new(*program_address, false),
                AccountMeta::new(*buffer_address, false),
                AccountMeta::new_readonly(sysvar::rent::id(), false),
                AccountMeta::new_readonly(sysvar::clock::id(), false),
                AccountMeta::new_readonly(crate::system_program::id(), false),
                AccountMeta::new_readonly(*upgrade_authority_address, true),
            ],
        ),
    ])
}

/// Returns the instructions required to upgrade a program.
pub fn upgrade(
    program_address: &Pubkey,
    buffer_address: &Pubkey,
    authority_address: &Pubkey,
    spill_address: &Pubkey,
) -> Instruction {
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    Instruction::new_with_bincode(
        id(),
        &UpgradeableLoaderInstruction::Upgrade,
        vec![
            AccountMeta::new(programdata_address, false),
            AccountMeta::new(*program_address, false),
            AccountMeta::new(*buffer_address, false),
            AccountMeta::new(*spill_address, false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
            AccountMeta::new_readonly(sysvar::clock::id(), false),
            AccountMeta::new_readonly(*authority_address, true),
        ],
    )
}

pub fn is_upgrade_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 3 == instruction_data[0]
}

pub fn is_set_authority_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 4 == instruction_data[0]
}

pub fn is_close_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 5 == instruction_data[0]
}

pub fn is_set_authority_checked_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 7 == instruction_data[0]
}

/// Returns the instructions required to set a buffers's authority.
pub fn set_buffer_authority(
    buffer_address: &Pubkey,
    current_authority_address: &Pubkey,
    new_authority_address: &Pubkey,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &UpgradeableLoaderInstruction::SetAuthority,
        vec![
            AccountMeta::new(*buffer_address, false),
            AccountMeta::new_readonly(*current_authority_address, true),
            AccountMeta::new_readonly(*new_authority_address, false),
        ],
    )
}

/// Returns the instructions required to set a buffers's authority. If using this instruction, the new authority
/// must sign.
pub fn set_buffer_authority_checked(
    buffer_address: &Pubkey,
    current_authority_address: &Pubkey,
    new_authority_address: &Pubkey,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &UpgradeableLoaderInstruction::SetAuthorityChecked,
        vec![
            AccountMeta::new(*buffer_address, false),
            AccountMeta::new_readonly(*current_authority_address, true),
            AccountMeta::new_readonly(*new_authority_address, true),
        ],
    )
}

/// Returns the instructions required to set a program's authority.
pub fn set_upgrade_authority(
    program_address: &Pubkey,
    current_authority_address: &Pubkey,
    new_authority_address: Option<&Pubkey>,
) -> Instruction {
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());

    let mut metas = vec![
        AccountMeta::new(programdata_address, false),
        AccountMeta::new_readonly(*current_authority_address, true),
    ];
    if let Some(address) = new_authority_address {
        metas.push(AccountMeta::new_readonly(*address, false));
    }
    Instruction::new_with_bincode(id(), &UpgradeableLoaderInstruction::SetAuthority, metas)
}

/// Returns the instructions required to set a program's authority. If using this instruction, the new authority
/// must sign.
pub fn set_upgrade_authority_checked(
    program_address: &Pubkey,
    current_authority_address: &Pubkey,
    new_authority_address: &Pubkey,
) -> Instruction {
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());

    let metas = vec![
        AccountMeta::new(programdata_address, false),
        AccountMeta::new_readonly(*current_authority_address, true),
        AccountMeta::new_readonly(*new_authority_address, true),
    ];
    Instruction::new_with_bincode(
        id(),
        &UpgradeableLoaderInstruction::SetAuthorityChecked,
        metas,
    )
}

/// Returns the instructions required to close a buffer account
pub fn close(
    close_address: &Pubkey,
    recipient_address: &Pubkey,
    authority_address: &Pubkey,
) -> Instruction {
    close_any(
        close_address,
        recipient_address,
        Some(authority_address),
        None,
    )
}

/// Returns the instructions required to close program, buffer, or uninitialized account
pub fn close_any(
    close_address: &Pubkey,
    recipient_address: &Pubkey,
    authority_address: Option<&Pubkey>,
    program_address: Option<&Pubkey>,
) -> Instruction {
    let mut metas = vec![
        AccountMeta::new(*close_address, false),
        AccountMeta::new(*recipient_address, false),
    ];
    if let Some(authority_address) = authority_address {
        metas.push(AccountMeta::new_readonly(*authority_address, true));
    }
    if let Some(program_address) = program_address {
        metas.push(AccountMeta::new(*program_address, false));
    }
    Instruction::new_with_bincode(id(), &UpgradeableLoaderInstruction::Close, metas)
}

/// Returns the instruction required to extend the size of a program's
/// executable data account
pub fn extend_program(
    program_address: &Pubkey,
    payer_address: Option<&Pubkey>,
    additional_bytes: u32,
) -> Instruction {
    let (program_data_address, _) =
        Pubkey::find_program_address(&[program_address.as_ref()], &id());
    let mut metas = vec![
        AccountMeta::new(program_data_address, false),
        AccountMeta::new(*program_address, false),
    ];
    if let Some(payer_address) = payer_address {
        metas.push(AccountMeta::new_readonly(
            crate::system_program::id(),
            false,
        ));
        metas.push(AccountMeta::new(*payer_address, true));
    }
    Instruction::new_with_bincode(
        id(),
        &UpgradeableLoaderInstruction::ExtendProgram { additional_bytes },
        metas,
    )
}

#[cfg(test)]
mod tests {
    use {super::*, bincode::serialized_size};

    #[test]
    fn test_state_size_of_uninitialized() {
        let buffer_state = UpgradeableLoaderState::Uninitialized;
        let size = serialized_size(&buffer_state).unwrap();
        assert_eq!(UpgradeableLoaderState::size_of_uninitialized() as u64, size);
    }

    #[test]
    fn test_state_size_of_buffer_metadata() {
        let buffer_state = UpgradeableLoaderState::Buffer {
            authority_address: Some(Pubkey::default()),
        };
        let size = serialized_size(&buffer_state).unwrap();
        assert_eq!(
            UpgradeableLoaderState::size_of_buffer_metadata() as u64,
            size
        );
    }

    #[test]
    fn test_state_size_of_programdata_metadata() {
        let programdata_state = UpgradeableLoaderState::ProgramData {
            upgrade_authority_address: Some(Pubkey::default()),
            slot: 0,
        };
        let size = serialized_size(&programdata_state).unwrap();
        assert_eq!(
            UpgradeableLoaderState::size_of_programdata_metadata() as u64,
            size
        );
    }

    #[test]
    fn test_state_size_of_program() {
        let program_state = UpgradeableLoaderState::Program {
            programdata_address: Pubkey::default(),
        };
        let size = serialized_size(&program_state).unwrap();
        assert_eq!(UpgradeableLoaderState::size_of_program() as u64, size);
    }

    #[test]
    #[allow(deprecated)]
    fn test_account_lengths() {
        assert_eq!(
            4,
            serialized_size(&UpgradeableLoaderState::Uninitialized).unwrap()
        );
        assert_eq!(36, UpgradeableLoaderState::program_len().unwrap());
        assert_eq!(
            45,
            UpgradeableLoaderState::programdata_data_offset().unwrap()
        );
        assert_eq!(
            45 + 42,
            UpgradeableLoaderState::programdata_len(42).unwrap()
        );
    }

    fn assert_is_instruction<F>(
        is_instruction_fn: F,
        expected_instruction: UpgradeableLoaderInstruction,
    ) where
        F: Fn(&[u8]) -> bool,
    {
        let result = is_instruction_fn(
            &bincode::serialize(&UpgradeableLoaderInstruction::InitializeBuffer).unwrap(),
        );
        let expected_result = matches!(
            expected_instruction,
            UpgradeableLoaderInstruction::InitializeBuffer
        );
        assert_eq!(expected_result, result);

        let result = is_instruction_fn(
            &bincode::serialize(&UpgradeableLoaderInstruction::Write {
                offset: 0,
                bytes: vec![],
            })
            .unwrap(),
        );
        let expected_result = matches!(
            expected_instruction,
            UpgradeableLoaderInstruction::Write {
                offset: _,
                bytes: _,
            }
        );
        assert_eq!(expected_result, result);

        let result = is_instruction_fn(
            &bincode::serialize(&UpgradeableLoaderInstruction::DeployWithMaxDataLen {
                max_data_len: 0,
            })
            .unwrap(),
        );
        let expected_result = matches!(
            expected_instruction,
            UpgradeableLoaderInstruction::DeployWithMaxDataLen { max_data_len: _ }
        );
        assert_eq!(expected_result, result);

        let result =
            is_instruction_fn(&bincode::serialize(&UpgradeableLoaderInstruction::Upgrade).unwrap());
        let expected_result = matches!(expected_instruction, UpgradeableLoaderInstruction::Upgrade);
        assert_eq!(expected_result, result);

        let result = is_instruction_fn(
            &bincode::serialize(&UpgradeableLoaderInstruction::SetAuthority).unwrap(),
        );
        let expected_result = matches!(
            expected_instruction,
            UpgradeableLoaderInstruction::SetAuthority
        );
        assert_eq!(expected_result, result);

        let result =
            is_instruction_fn(&bincode::serialize(&UpgradeableLoaderInstruction::Close).unwrap());
        let expected_result = matches!(expected_instruction, UpgradeableLoaderInstruction::Close);
        assert_eq!(expected_result, result);
    }

    #[test]
    fn test_is_set_authority_instruction() {
        assert!(!is_set_authority_instruction(&[]));
        assert_is_instruction(
            is_set_authority_instruction,
            UpgradeableLoaderInstruction::SetAuthority {},
        );
    }

    #[test]
    fn test_is_set_authority_checked_instruction() {
        assert!(!is_set_authority_checked_instruction(&[]));
        assert_is_instruction(
            is_set_authority_checked_instruction,
            UpgradeableLoaderInstruction::SetAuthorityChecked {},
        );
    }

    #[test]
    fn test_is_upgrade_instruction() {
        assert!(!is_upgrade_instruction(&[]));
        assert_is_instruction(
            is_upgrade_instruction,
            UpgradeableLoaderInstruction::Upgrade {},
        );
    }
}
