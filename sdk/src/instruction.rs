//! Defines a composable Instruction type and a memory-efficient CompiledInstruction.

use crate::pubkey::Pubkey;
use crate::short_vec;
use crate::system_instruction::SystemError;
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

    /// An account was referenced more than once in a single instruction
    DuplicateAccountIndex,

    /// CustomError allows on-chain programs to implement program-specific error types and see
    /// them returned by the Solana runtime. A CustomError may be any type that is represented
    /// as or serialized to a u32 integer.
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
    pub program_ids_index: Pubkey,
    /// Metadata for what accounts should be passed to the instruction processor
    pub accounts: Vec<AccountMeta>,
    /// Opaque data passed to the instruction processor
    pub data: Vec<u8>,
}

impl Instruction {
    pub fn new<T: Serialize>(
        program_ids_index: Pubkey,
        data: &T,
        accounts: Vec<AccountMeta>,
    ) -> Self {
        let data = serialize(data).unwrap();
        Self {
            program_ids_index,
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
    /// True if an Instruciton requires a Transaction signature matching `pubkey`.
    pub is_signer: bool,
}

impl AccountMeta {
    pub fn new(pubkey: Pubkey, is_signer: bool) -> Self {
        Self { pubkey, is_signer }
    }
}

/// An instruction to execute a program
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct CompiledInstruction {
    /// Index into the transaction program ids array indicating the program account that executes this instruction
    pub program_ids_index: u8,
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
            program_ids_index,
            data,
            accounts,
        }
    }

    pub fn program_id<'a>(&self, program_ids: &'a [Pubkey]) -> &'a Pubkey {
        &program_ids[self.program_ids_index as usize]
    }
}
