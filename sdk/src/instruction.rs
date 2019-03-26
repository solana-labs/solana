//! Defines a composable Instruction type and a memory-efficient CompiledInstruction.

use crate::pubkey::Pubkey;
use crate::shortvec::{deserialize_vec_bytes, encode_len, serialize_vec_bytes};
use crate::system_instruction::SystemError;
use bincode::{serialize, Error};
use serde::Serialize;
use std::io::{Cursor, Read, Write};
use std::mem::size_of;

/// Reasons the runtime might have rejected an instruction.
#[derive(Debug, PartialEq, Eq, Clone)]
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
    /// them returned by the Solana runtime. A CustomError may be any type that is serialized
    /// to a Vec of bytes, max length 32 bytes. Any CustomError Vec greater than this length will
    /// be truncated by the runtime.
    CustomError(Vec<u8>),
}

impl InstructionError {
    pub fn new_result_with_negative_lamports() -> Self {
        let serialized_error =
            bincode::serialize(&SystemError::ResultWithNegativeLamports).unwrap();
        InstructionError::CustomError(serialized_error)
    }
}

/// An instruction to execute a program
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct GenericInstruction<P, Q> {
    /// Index into the transaction program ids array indicating the program account that executes this instruction
    pub program_ids_index: P,
    /// Ordered indices into the transaction keys array indicating which accounts to pass to the program
    pub accounts: Vec<Q>,
    /// The program input data
    pub data: Vec<u8>,
}

impl<P, Q> GenericInstruction<P, Q> {
    pub fn new<T: Serialize>(program_ids_index: P, data: &T, accounts: Vec<Q>) -> Self {
        let data = serialize(data).unwrap();
        Self {
            program_ids_index,
            data,
            accounts,
        }
    }
}

/// Account metadata used to define Instructions
#[derive(Debug, PartialEq)]
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

pub type Instruction = GenericInstruction<Pubkey, AccountMeta>;
pub type CompiledInstruction = GenericInstruction<u8, u8>;

impl CompiledInstruction {
    pub fn serialize_with(mut writer: &mut Cursor<&mut [u8]>, ix: &Self) -> Result<(), Error> {
        writer.write_all(&[ix.program_ids_index])?;
        serialize_vec_bytes(&mut writer, &ix.accounts[..])?;
        serialize_vec_bytes(&mut writer, &ix.data[..])?;
        Ok(())
    }

    pub fn deserialize_from(mut reader: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        let mut buf = [0];
        reader.read_exact(&mut buf)?;
        let program_ids_index = buf[0];
        let accounts = deserialize_vec_bytes(&mut reader)?;
        let data = deserialize_vec_bytes(&mut reader)?;
        Ok(CompiledInstruction {
            program_ids_index,
            accounts,
            data,
        })
    }

    pub fn serialized_size(&self) -> Result<u64, Error> {
        let mut buf = [0; size_of::<u64>() + 1];
        let mut wr = Cursor::new(&mut buf[..]);
        let mut size = size_of::<u8>();

        let len = self.accounts.len();
        encode_len(&mut wr, len)?;
        size += wr.position() as usize + (len * size_of::<u8>());

        let len = self.data.len();
        wr.set_position(0);
        encode_len(&mut wr, len)?;
        size += wr.position() as usize + (len * size_of::<u8>());

        Ok(size as u64)
    }
}
