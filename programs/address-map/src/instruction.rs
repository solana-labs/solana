use {
    crate::id,
    serde::{Deserialize, Serialize},
    solana_sdk::{
        clock::Epoch,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        system_program,
    },
};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum AddressMapInstruction {
    /// Initialize an address map account
    ///
    /// # Account references
    ///   0. `[WRITE]` Uninitialized address map account
    ///   1. `[SIGNER, WRITE]` Account that will fund the new address map.
    ///   2. `[]` System program for CPI.
    InitializeAccount {
        bump_seed: u8,
        num_entries: u8,
        authority: Pubkey,
    },

    /// Set a new authority for an address map account
    ///
    /// # Account references
    ///   0. `[WRITE]` Address map account to set authority for
    ///   1. `[SIGNER]` Current authority
    SetAuthority { new_authority: Option<Pubkey> },

    /// Insert entries into an address map account
    ///
    /// # Account references
    ///   0. `[WRITE]` Address map account to insert entries into
    ///   1. `[SIGNER]` Current authority
    InsertEntries { offset: u8, entries: Vec<Pubkey> },

    /// Activate an address map account
    ///
    /// # Account references
    ///   0. `[WRITE]` Address map account to activate
    ///   1. `[SIGNER]` Current authority
    Activate,

    /// Deactivate an address map account
    ///
    /// # Account references
    ///   0. `[WRITE]` Address map account to deactivate
    ///   1. `[SIGNER]` Current authority
    Deactivate,

    /// Close an address map account
    ///
    /// # Account references
    ///   0. `[WRITE]` Address map account to close
    ///   1. `[WRITE]` Recipient of closed account lamports
    ///   2. `[SIGNER]` Current authority, if required.
    CloseAccount,
}

/// Derive an address map address from a wallet address and the current epoch.
pub fn derive_address_map_address(payer_address: &Pubkey, current_epoch: Epoch) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[payer_address.as_ref(), &current_epoch.to_le_bytes()],
        &id(),
    )
}

/// Returns an instruction to initialize a map account as well as the address of
/// the map account that will be created when the instruction is processed.
pub fn initialize_account(
    payer_address: Pubkey,
    authority_address: Pubkey,
    current_epoch: Epoch,
    num_entries: u8,
) -> (Instruction, Pubkey) {
    let (map_address, bump_seed) = derive_address_map_address(&payer_address, current_epoch);
    let instruction = Instruction::new_with_bincode(
        id(),
        &AddressMapInstruction::InitializeAccount {
            bump_seed,
            num_entries,
            authority: authority_address,
        },
        vec![
            AccountMeta::new(map_address, false),
            AccountMeta::new(payer_address, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    (instruction, map_address)
}

/// Returns an instruction that updates the authority of an address map account.
/// If the new authority is `None`, the address map account will be immutable.
/// Inactive address maps cannot be made immutable.
pub fn set_authority(
    map_address: Pubkey,
    current_authority_address: Pubkey,
    new_authority_address: Option<Pubkey>,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &AddressMapInstruction::SetAuthority {
            new_authority: new_authority_address,
        },
        vec![
            AccountMeta::new(map_address, false),
            AccountMeta::new_readonly(current_authority_address, true),
        ],
    )
}

/// Returns an instruction which inserts address entries into an address map
/// account at the specified offset.
pub fn insert_entries(
    map_address: Pubkey,
    authority_address: Pubkey,
    offset: u8,
    entries: Vec<Pubkey>,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &AddressMapInstruction::InsertEntries { offset, entries },
        vec![
            AccountMeta::new(map_address, false),
            AccountMeta::new_readonly(authority_address, true),
        ],
    )
}

/// Returns an instruction that starts the activation phase for an address map
/// account.
pub fn activate(map_address: Pubkey, authority_address: Pubkey) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &AddressMapInstruction::Activate,
        vec![
            AccountMeta::new(map_address, false),
            AccountMeta::new_readonly(authority_address, true),
        ],
    )
}

/// Returns an instruction that starts the deactivation phase for an address map
/// account.
pub fn deactivate(map_address: Pubkey, authority_address: Pubkey) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &AddressMapInstruction::Deactivate,
        vec![
            AccountMeta::new(map_address, false),
            AccountMeta::new_readonly(authority_address, true),
        ],
    )
}

/// Returns an instruction that closes an inactive address map account and
/// transfers its lamport balance to the recipient address.
pub fn close_account(
    map_address: Pubkey,
    recipient_address: Pubkey,
    authority_address: Pubkey,
) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &AddressMapInstruction::CloseAccount,
        vec![
            AccountMeta::new(map_address, false),
            AccountMeta::new(recipient_address, false),
            AccountMeta::new_readonly(authority_address, true),
        ],
    )
}
