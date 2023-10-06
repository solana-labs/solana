//! The v4 built-in loader program.
//!
//! This is the loader of the program runtime v2.

use crate::{
    instruction::{AccountMeta, Instruction},
    loader_v4_instruction::LoaderV4Instruction,
    pubkey::Pubkey,
    system_instruction,
};

crate::declare_id!("LoaderV411111111111111111111111111111111111");

/// Cooldown before a program can be un-/redeployed again
pub const DEPLOYMENT_COOLDOWN_IN_SLOTS: u64 = 750;

#[repr(u64)]
#[derive(Debug, PartialEq, Eq, Clone, Copy, AbiExample)]
pub enum LoaderV4Status {
    /// Program is in maintanance
    Retracted,
    /// Program is ready to be executed
    Deployed,
    /// Same as `Deployed`, but can not be retracted anymore
    Finalized,
}

/// LoaderV4 account states
#[repr(C)]
#[derive(Debug, PartialEq, Eq, Clone, Copy, AbiExample)]
pub struct LoaderV4State {
    /// Slot in which the program was last deployed, retracted or initialized.
    pub slot: u64,
    /// Address of signer which can send program management instructions.
    pub authority_address: Pubkey,
    /// Deployment status.
    pub status: LoaderV4Status,
    // The raw program data follows this serialized structure in the
    // account's data.
}

impl LoaderV4State {
    /// Size of a serialized program account.
    pub const fn program_data_offset() -> usize {
        std::mem::size_of::<Self>()
    }
}

pub fn is_write_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 0 == instruction_data[0]
}

pub fn is_truncate_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 1 == instruction_data[0]
}

pub fn is_deploy_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 2 == instruction_data[0]
}

pub fn is_retract_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 3 == instruction_data[0]
}

pub fn is_transfer_authority_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 4 == instruction_data[0]
}

/// Returns the instructions required to initialize a program/buffer account.
pub fn create_buffer(
    payer_address: &Pubkey,
    buffer_address: &Pubkey,
    lamports: u64,
    authority: &Pubkey,
    new_size: u32,
    recipient_address: &Pubkey,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(payer_address, buffer_address, lamports, 0, &id()),
        truncate_uninitialized(buffer_address, authority, new_size, recipient_address),
    ]
}

/// Returns the instructions required to set the length of an uninitialized program account.
/// This instruction will require the program account to also sign the transaction.
pub fn truncate_uninitialized(
    program_address: &Pubkey,
    authority: &Pubkey,
    new_size: u32,
    recipient_address: &Pubkey,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &LoaderV4Instruction::Truncate { new_size },
        vec![
            AccountMeta::new(*program_address, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*recipient_address, false),
        ],
    )
}

/// Returns the instructions required to set the length of the program account.
pub fn truncate(
    program_address: &Pubkey,
    authority: &Pubkey,
    new_size: u32,
    recipient_address: &Pubkey,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &LoaderV4Instruction::Truncate { new_size },
        vec![
            AccountMeta::new(*program_address, false),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*recipient_address, false),
        ],
    )
}

/// Returns the instructions required to write a chunk of program data to a
/// buffer account.
pub fn write(
    program_address: &Pubkey,
    authority: &Pubkey,
    offset: u32,
    bytes: Vec<u8>,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &LoaderV4Instruction::Write { offset, bytes },
        vec![
            AccountMeta::new(*program_address, false),
            AccountMeta::new_readonly(*authority, true),
        ],
    )
}

/// Returns the instructions required to deploy a program.
pub fn deploy(program_address: &Pubkey, authority: &Pubkey) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &LoaderV4Instruction::Deploy,
        vec![
            AccountMeta::new(*program_address, false),
            AccountMeta::new_readonly(*authority, true),
        ],
    )
}

/// Returns the instructions required to deploy a program using a buffer.
pub fn deploy_from_source(
    program_address: &Pubkey,
    authority: &Pubkey,
    source_address: &Pubkey,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &LoaderV4Instruction::Deploy,
        vec![
            AccountMeta::new(*program_address, false),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*source_address, false),
        ],
    )
}

/// Returns the instructions required to retract a program.
pub fn retract(program_address: &Pubkey, authority: &Pubkey) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &LoaderV4Instruction::Retract,
        vec![
            AccountMeta::new(*program_address, false),
            AccountMeta::new_readonly(*authority, true),
        ],
    )
}

/// Returns the instructions required to transfer authority over a program.
pub fn transfer_authority(
    program_address: &Pubkey,
    authority: &Pubkey,
    new_authority: Option<&Pubkey>,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(*program_address, false),
        AccountMeta::new_readonly(*authority, true),
    ];

    if let Some(new_auth) = new_authority {
        accounts.push(AccountMeta::new_readonly(*new_auth, true));
    }

    Instruction::new_with_bincode(id(), &LoaderV4Instruction::TransferAuthority, accounts)
}

#[cfg(test)]
mod tests {
    use {super::*, crate::system_program, memoffset::offset_of};

    #[test]
    fn test_layout() {
        assert_eq!(offset_of!(LoaderV4State, slot), 0x00);
        assert_eq!(offset_of!(LoaderV4State, authority_address), 0x08);
        assert_eq!(offset_of!(LoaderV4State, status), 0x28);
        assert_eq!(LoaderV4State::program_data_offset(), 0x30);
    }

    #[test]
    fn test_create_buffer_instruction() {
        let payer = Pubkey::new_unique();
        let program = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let instructions = create_buffer(&payer, &program, 123, &authority, 10, &recipient);
        assert_eq!(instructions.len(), 2);
        let instruction0 = &instructions[0];
        assert_eq!(instruction0.program_id, system_program::id());
        assert_eq!(instruction0.accounts.len(), 2);
        assert_eq!(instruction0.accounts[0].pubkey, payer);
        assert!(instruction0.accounts[0].is_writable);
        assert!(instruction0.accounts[0].is_signer);
        assert_eq!(instruction0.accounts[1].pubkey, program);
        assert!(instruction0.accounts[1].is_writable);
        assert!(instruction0.accounts[1].is_signer);

        let instruction1 = &instructions[1];
        assert!(is_truncate_instruction(&instruction1.data));
        assert_eq!(instruction1.program_id, id());
        assert_eq!(instruction1.accounts.len(), 3);
        assert_eq!(instruction1.accounts[0].pubkey, program);
        assert!(instruction1.accounts[0].is_writable);
        assert!(instruction1.accounts[0].is_signer);
        assert_eq!(instruction1.accounts[1].pubkey, authority);
        assert!(!instruction1.accounts[1].is_writable);
        assert!(instruction1.accounts[1].is_signer);
        assert_eq!(instruction1.accounts[2].pubkey, recipient);
        assert!(instruction1.accounts[2].is_writable);
        assert!(!instruction1.accounts[2].is_signer);
    }

    #[test]
    fn test_write_instruction() {
        let program = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let instruction = write(&program, &authority, 123, vec![1, 2, 3, 4]);
        assert!(is_write_instruction(&instruction.data));
        assert_eq!(instruction.program_id, id());
        assert_eq!(instruction.accounts.len(), 2);
        assert_eq!(instruction.accounts[0].pubkey, program);
        assert!(instruction.accounts[0].is_writable);
        assert!(!instruction.accounts[0].is_signer);
        assert_eq!(instruction.accounts[1].pubkey, authority);
        assert!(!instruction.accounts[1].is_writable);
        assert!(instruction.accounts[1].is_signer);
    }

    #[test]
    fn test_truncate_instruction() {
        let program = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let instruction = truncate(&program, &authority, 10, &recipient);
        assert!(is_truncate_instruction(&instruction.data));
        assert_eq!(instruction.program_id, id());
        assert_eq!(instruction.accounts.len(), 3);
        assert_eq!(instruction.accounts[0].pubkey, program);
        assert!(instruction.accounts[0].is_writable);
        assert!(!instruction.accounts[0].is_signer);
        assert_eq!(instruction.accounts[1].pubkey, authority);
        assert!(!instruction.accounts[1].is_writable);
        assert!(instruction.accounts[1].is_signer);
        assert_eq!(instruction.accounts[2].pubkey, recipient);
        assert!(instruction.accounts[2].is_writable);
        assert!(!instruction.accounts[2].is_signer);
    }

    #[test]
    fn test_deploy_instruction() {
        let program = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let instruction = deploy(&program, &authority);
        assert!(is_deploy_instruction(&instruction.data));
        assert_eq!(instruction.program_id, id());
        assert_eq!(instruction.accounts.len(), 2);
        assert_eq!(instruction.accounts[0].pubkey, program);
        assert!(instruction.accounts[0].is_writable);
        assert!(!instruction.accounts[0].is_signer);
        assert_eq!(instruction.accounts[1].pubkey, authority);
        assert!(!instruction.accounts[1].is_writable);
        assert!(instruction.accounts[1].is_signer);
    }

    #[test]
    fn test_deploy_from_source_instruction() {
        let program = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let source = Pubkey::new_unique();
        let instruction = deploy_from_source(&program, &authority, &source);
        assert!(is_deploy_instruction(&instruction.data));
        assert_eq!(instruction.program_id, id());
        assert_eq!(instruction.accounts.len(), 3);
        assert_eq!(instruction.accounts[0].pubkey, program);
        assert!(instruction.accounts[0].is_writable);
        assert!(!instruction.accounts[0].is_signer);
        assert_eq!(instruction.accounts[1].pubkey, authority);
        assert!(!instruction.accounts[1].is_writable);
        assert!(instruction.accounts[1].is_signer);
        assert_eq!(instruction.accounts[2].pubkey, source);
        assert!(instruction.accounts[2].is_writable);
        assert!(!instruction.accounts[2].is_signer);
    }

    #[test]
    fn test_retract_instruction() {
        let program = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let instruction = retract(&program, &authority);
        assert!(is_retract_instruction(&instruction.data));
        assert_eq!(instruction.program_id, id());
        assert_eq!(instruction.accounts.len(), 2);
        assert_eq!(instruction.accounts[0].pubkey, program);
        assert!(instruction.accounts[0].is_writable);
        assert!(!instruction.accounts[0].is_signer);
        assert_eq!(instruction.accounts[1].pubkey, authority);
        assert!(!instruction.accounts[1].is_writable);
        assert!(instruction.accounts[1].is_signer);
    }

    #[test]
    fn test_transfer_authority_instruction() {
        let program = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let new_authority = Pubkey::new_unique();
        let instruction = transfer_authority(&program, &authority, Some(&new_authority));
        assert!(is_transfer_authority_instruction(&instruction.data));
        assert_eq!(instruction.program_id, id());
        assert_eq!(instruction.accounts.len(), 3);
        assert_eq!(instruction.accounts[0].pubkey, program);
        assert!(instruction.accounts[0].is_writable);
        assert!(!instruction.accounts[0].is_signer);
        assert_eq!(instruction.accounts[1].pubkey, authority);
        assert!(!instruction.accounts[1].is_writable);
        assert!(instruction.accounts[1].is_signer);
        assert_eq!(instruction.accounts[2].pubkey, new_authority);
        assert!(!instruction.accounts[2].is_writable);
        assert!(instruction.accounts[2].is_signer);
    }

    #[test]
    fn test_transfer_authority_finalize_instruction() {
        let program = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let instruction = transfer_authority(&program, &authority, None);
        assert!(is_transfer_authority_instruction(&instruction.data));
        assert_eq!(instruction.program_id, id());
        assert_eq!(instruction.accounts.len(), 2);
        assert_eq!(instruction.accounts[0].pubkey, program);
        assert!(instruction.accounts[0].is_writable);
        assert!(!instruction.accounts[0].is_signer);
        assert_eq!(instruction.accounts[1].pubkey, authority);
        assert!(!instruction.accounts[1].is_writable);
        assert!(instruction.accounts[1].is_signer);
    }
}
