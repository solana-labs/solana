///! Instructions provided by the ZkToken Proof program
pub use crate::instruction::*;
use {
    crate::zk_token_elgamal::pod::PodProofType,
    bytemuck::{bytes_of, Pod, Zeroable},
    num_derive::{FromPrimitive, ToPrimitive},
    num_traits::{FromPrimitive, ToPrimitive},
    solana_program::{
        instruction::{
            AccountMeta, Instruction,
            InstructionError::{self, InvalidInstructionData},
        },
        pubkey::Pubkey,
    },
    std::mem::size_of,
};

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofType {
    /// Empty proof type used to distinguish if a proof context account is initialized
    Uninitialized,
    CloseAccount,
    Withdraw,
    WithdrawWithheldTokens,
    Transfer,
    TransferWithFee,
    PubkeyValidity,
}

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofInstruction {
    /// Verify a zero-knowledge proof data.
    ///
    /// This instruction can be configured to optionally create a proof context state account.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account authority
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `VerifyProofData`
    ///
    VerifyProof,

    /// Close a zero-knowledge proof context state.
    ///
    /// Accounts expected by this instruction:
    ///   0. `[writable]` The proof context account to close
    ///   1. `[writable]` The destination account for lamports
    ///   2. `[signer]` The context account's owner
    ///
    /// Data expected by this instruction:
    ///   `ProofType`
    ///
    CloseContextState,
}

/// Data expected by `ProofInstruction::VerifyProof`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct VerifyProofData<T: ZkProofData> {
    /// The zero-knowledge proof type to verify
    pub proof_type: PodProofType,
    /// The proof data that contains the zero-knowledge proof and its context
    pub proof_data: T,
}

// `bytemuck::Pod` cannot be derived for generic structs unless the struct is marked
// `repr(packed)`, which may cause unnecessary complications when referencing its fields. Directly
// mark `VerifyProofData` as `Zeroable` and `Pod` since since none of its fields has an alignment
// requirement greater than 1 and therefore, guaranteed to be `packed`.
unsafe impl<T: ZkProofData> Zeroable for VerifyProofData<T> {}
unsafe impl<T: ZkProofData> Pod for VerifyProofData<T> {}

impl<T: ZkProofData> VerifyProofData<T> {
    /// Serializes `VerifyProofData`.
    ///
    /// A `VerifyProofData::encode(&self)` syntax could force the caller to make unnecessary copy of
    /// `T: ZkProofData`, which can be quite large. This syntax takes in references to the
    /// individual `VerifyProofData` components to provide flexibility to the caller.
    pub fn encode(proof_type: ProofType, proof_data: &T) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        buf.push(ToPrimitive::to_u8(&proof_type).unwrap());
        buf.extend_from_slice(bytes_of(proof_data));
        buf
    }

    pub fn try_from_bytes(input: &[u8]) -> Result<&Self, InstructionError> {
        bytemuck::try_from_bytes(input).map_err(|_| InvalidInstructionData)
    }
}

/// Pubkeys associated with a context state account to be used as parameters to functions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextStateInfo<'a> {
    pub context_state_account: &'a Pubkey,
    pub context_state_authority: &'a Pubkey,
}

// Create a `VerifyProof` instruction.
pub fn verify_proof<T: ZkProofData>(
    proof_type: ProofType,
    context_state_info: Option<ContextStateInfo>,
    proof_data: &T,
) -> Instruction {
    let accounts = if let Some(context_state_info) = context_state_info {
        vec![
            AccountMeta::new(*context_state_info.context_state_account, false),
            AccountMeta::new_readonly(*context_state_info.context_state_authority, false),
        ]
    } else {
        vec![]
    };

    let data = [
        vec![ToPrimitive::to_u8(&ProofInstruction::VerifyProof).unwrap()],
        VerifyProofData::encode(proof_type, proof_data),
    ]
    .concat();

    Instruction {
        program_id: crate::zk_token_proof_program::id(),
        accounts,
        data,
    }
}

// Create a `CloseContextState` instruction.
pub fn close_context_state(
    context_state_info: ContextStateInfo,
    destination_account: &Pubkey,
    proof_type: ProofType,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new(*context_state_info.context_state_account, false),
        AccountMeta::new(*destination_account, false),
        AccountMeta::new_readonly(*context_state_info.context_state_authority, true),
    ];

    let data = vec![
        ToPrimitive::to_u8(&ProofInstruction::CloseContextState).unwrap(),
        ToPrimitive::to_u8(&proof_type).unwrap(),
    ];

    Instruction {
        program_id: crate::zk_token_proof_program::id(),
        accounts,
        data,
    }
}

impl ProofInstruction {
    pub fn instruction_type(input: &[u8]) -> Option<Self> {
        input
            .first()
            .and_then(|instruction| FromPrimitive::from_u8(*instruction))
    }

    pub fn proof_type(input: &[u8]) -> Option<ProofType> {
        input
            .get(1)
            .and_then(|proof_type| FromPrimitive::from_u8(*proof_type))
    }
}
