//! Defines a composable Instruction type and a memory-efficient CompiledInstruction.

use crate::{pubkey::Pubkey, short_vec, system_instruction::SystemError};
use bincode::serialize;
use serde::Serialize;

/// Reasons the runtime might have rejected an instruction.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum InstructionError {
    /// Deprecated! Use CustomError instead!
    /// The program instruction returned an error
    GenericError,

    /// The arguments provided to a program instruction where invalid
    InvalidArgument,

    /// An instruction's data contents was invalid
    InvalidInstructionData,

    /// An account's data contents was invalid
    InvalidAccountData,

    /// An account's data was too small
    AccountDataTooSmall,

    /// An account's balance was too small to complete the instruction
    InsufficientFunds,

    /// The account did not have the expected program id
    IncorrectProgramId,

    /// A signature was required but not found
    MissingRequiredSignature,

    /// An initialize instruction was sent to an account that has already been initialized.
    AccountAlreadyInitialized,

    /// An attempt to operate on an account that hasn't been initialized.
    UninitializedAccount,

    /// Program's instruction lamport balance does not equal the balance after the instruction
    UnbalancedInstruction,

    /// Program modified an account's program id
    ModifiedProgramId,

    /// Program spent the lamports of an account that doesn't belong to it
    ExternalAccountLamportSpend,

    /// Program modified the data of an account that doesn't belong to it
    ExternalAccountDataModified,

    /// Read-only account modified lamports
    ReadonlyLamportChange,

    /// Read-only account modified data
    ReadonlyDataModified,

    /// An account was referenced more than once in a single instruction
    // Deprecated, instructions can now contain duplicate accounts
    DuplicateAccountIndex,

    /// Executable bit on account changed, but shouldn't have
    ExecutableModified,

    /// Rent_epoch account changed, but shouldn't have
    RentEpochModified,

    /// The instruction expected additional account keys
    NotEnoughAccountKeys,

    /// A non-system program changed the size of the account data
    AccountDataSizeChanged,

    /// The instruction expected an executable account
    AccountNotExecutable,

    /// Failed to borrow a reference to account data, already borrowed
    AccountBorrowFailed,

    /// Account data has an outstanding reference after a program's execution
    AccountBorrowOutstanding,

    /// The same account was multiply passed to an on-chain program's entrypoint, but the program
    /// modified them differently.  A program can only modify one instance of the account because
    /// the runtime cannot determine which changes to pick or how to merge them if both are modified
    DuplicateAccountOutOfSync,

    /// The return value from the program was invalid.  Valid errors are either a defined builtin
    /// error value or a user-defined error in the lower 32 bits.
    InvalidError,

    /// Allows on-chain programs to implement program-specific error types and see them returned
    /// by the Solana runtime. A program-specific error may be any type that is represented as
    /// or serialized to a u32 integer.
    CustomError(u32),
}

impl InstructionError {
    pub fn new_result_with_negative_lamports() -> Self {
        InstructionError::CustomError(SystemError::ResultWithNegativeLamports as u32)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Instruction {
    /// Pubkey of the instruction processor that executes this instruction
    pub program_id: Pubkey,
    /// Metadata for what accounts should be passed to the instruction processor
    pub accounts: Vec<AccountMeta>,
    /// Opaque data passed to the instruction processor
    pub data: Vec<u8>,
}

impl Instruction {
    pub fn new<T: Serialize>(program_id: Pubkey, data: &T, accounts: Vec<AccountMeta>) -> Self {
        let data = serialize(data).unwrap();
        Self {
            program_id,
            data,
            accounts,
        }
    }
}

/// Account metadata used to define Instructions
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct AccountMeta {
    /// An account's public key
    pub pubkey: Pubkey,
    /// True if an Instruction requires a Transaction signature matching `pubkey`.
    pub is_signer: bool,
    /// True if the `pubkey` can be loaded as a read-write account.
    pub is_writable: bool,
}

impl AccountMeta {
    pub fn new(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer,
            is_writable: true,
        }
    }

    pub fn new_readonly(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer,
            is_writable: false,
        }
    }
}

/// Trait for adding a signer Pubkey to an existing data structure
pub trait WithSigner {
    /// Add a signer Pubkey
    fn with_signer(self, signer: &Pubkey) -> Self;
}

impl WithSigner for Vec<AccountMeta> {
    fn with_signer(mut self, signer: &Pubkey) -> Self {
        for meta in self.iter_mut() {
            // signer might already appear in parameters
            if &meta.pubkey == signer {
                meta.is_signer = true; // found it, we're done
                return self;
            }
        }

        // signer wasn't in metas, append it after normal parameters
        self.push(AccountMeta::new_readonly(*signer, true));
        self
    }
}

/// An instruction to execute a program
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CompiledInstruction {
    /// Index into the transaction keys array indicating the program account that executes this instruction
    pub program_id_index: u8,
    /// Ordered indices into the transaction keys array indicating which accounts to pass to the program
    #[serde(with = "short_vec")]
    pub accounts: Vec<u8>,
    /// The program input data
    #[serde(with = "short_vec")]
    pub data: Vec<u8>,
}

impl CompiledInstruction {
    pub fn new<T: Serialize>(program_ids_index: u8, data: &T, accounts: Vec<u8>) -> Self {
        let data = serialize(data).unwrap();
        Self {
            program_id_index: program_ids_index,
            data,
            accounts,
        }
    }

    pub fn program_id<'a>(&self, program_ids: &'a [Pubkey]) -> &'a Pubkey {
        &program_ids[self.program_id_index as usize]
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_account_meta_list_with_signer() {
        let account_pubkey = Pubkey::new_rand();
        let signer_pubkey = Pubkey::new_rand();

        let account_meta = AccountMeta::new(account_pubkey, false);
        let signer_account_meta = AccountMeta::new(signer_pubkey, false);

        let metas = vec![].with_signer(&signer_pubkey);
        assert_eq!(metas.len(), 1);
        assert!(metas[0].is_signer);

        let metas = vec![account_meta.clone()].with_signer(&signer_pubkey);
        assert_eq!(metas.len(), 2);
        assert!(!metas[0].is_signer);
        assert!(metas[1].is_signer);
        assert_eq!(metas[1].pubkey, signer_pubkey);

        let metas = vec![signer_account_meta.clone()].with_signer(&signer_pubkey);
        assert_eq!(metas.len(), 1);
        assert!(metas[0].is_signer);
        assert_eq!(metas[0].pubkey, signer_pubkey);

        let metas = vec![account_meta, signer_account_meta].with_signer(&signer_pubkey);
        assert_eq!(metas.len(), 2);
        assert!(!metas[0].is_signer);
        assert!(metas[1].is_signer);
        assert_eq!(metas[1].pubkey, signer_pubkey);
    }
}
