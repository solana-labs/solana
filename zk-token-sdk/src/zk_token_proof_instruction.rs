//! Instructions provided by the ZkToken Proof program
pub use crate::instruction::*;
use {
    bytemuck::bytes_of,
    num_derive::{FromPrimitive, ToPrimitive},
    num_traits::{FromPrimitive, ToPrimitive},
    solana_program::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
    },
};

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofInstruction {
    /// Close a zero-knowledge proof context state.
    ///
    /// Accounts expected by this instruction:
    ///   0. `[writable]` The proof context account to close
    ///   1. `[writable]` The destination account for lamports
    ///   2. `[signer]` The context account's owner
    ///
    /// Data expected by this instruction:
    ///   None
    ///
    CloseContextState,

    /// Verify a zero-balance proof.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<ZeroBalanceProofContext>` and assigned to the ZkToken proof program
    /// prior to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `ZeroBalanceProofData`
    ///
    VerifyZeroBalance,

    /// Verify a withdraw zero-knowledge proof.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<WithdrawProofContext>` and assigned to the ZkToken proof program prior
    /// to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `WithdrawData`
    ///
    VerifyWithdraw,

    /// Verify a ciphertext-ciphertext equality proof.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<CiphertextCiphertextEqualityProofContext>` and assigned to the ZkToken
    /// proof program prior to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `CiphertextCiphertextEqualityProofData`
    ///
    VerifyCiphertextCiphertextEquality,

    /// Verify a transfer zero-knowledge proof.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<TransferProofContext>` and assigned to the ZkToken proof program prior
    /// to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `TransferData`
    ///
    VerifyTransfer,

    /// Verify a transfer with fee zero-knowledge proof.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<TransferWithFeeProofContext>` and assigned to the ZkToken proof program
    /// prior to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `TransferWithFeeData`
    ///
    VerifyTransferWithFee,

    /// Verify a pubkey validity zero-knowledge proof.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<PubkeyValidityProofContext>` and assigned to the ZkToken proof program
    /// prior to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `PubkeyValidityData`
    ///
    VerifyPubkeyValidity,

    /// Verify a 64-bit range proof.
    ///
    /// A range proof is defined with respect to a Pedersen commitment. The 64-bit range proof
    /// certifies that a Pedersen commitment holds an unsigned 64-bit number.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<RangeProofContext>` and assigned to the ZkToken proof program prior to
    /// the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `RangeProofU64Data`
    ///
    VerifyRangeProofU64,

    /// Verify a 64-bit batched range proof.
    ///
    /// A batched range proof is defined with respect to a sequence of Pedersen commitments `[C_1,
    /// ..., C_N]` and bit-lengths `[n_1, ..., n_N]`. It certifies that each commitment `C_i` is a
    /// commitment to a positive number of bit-length `n_i`. Batch verifying range proofs is more
    /// efficient than verifying independent range proofs on commitments `C_1, ..., C_N`
    /// separately.
    ///
    /// The bit-length of a batched range proof specifies the sum of the individual bit-lengths
    /// `n_1, ..., n_N`. For example, this instruction can be used to certify that two commitments
    /// `C_1` and `C_2` each hold positive 32-bit numbers.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<BatchedRangeProofContext>` and assigned to the ZkToken proof program
    /// prior to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `BatchedRangeProof64Data`
    ///
    VerifyBatchedRangeProofU64,

    /// Verify 128-bit batched range proof.
    ///
    /// The bit-length of a batched range proof specifies the sum of the individual bit-lengths
    /// `n_1, ..., n_N`. For example, this instruction can be used to certify that two commitments
    /// `C_1` and `C_2` each hold positive 64-bit numbers.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<BatchedRangeProofContext>` and assigned to the ZkToken proof program
    /// prior to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `BatchedRangeProof128Data`
    ///
    VerifyBatchedRangeProofU128,

    /// Verify 256-bit batched range proof.
    ///
    /// The bit-length of a batched range proof specifies the sum of the individual bit-lengths
    /// `n_1, ..., n_N`. For example, this instruction can be used to certify that four commitments
    /// `[C_1, C_2, C_3, C_4]` each hold positive 64-bit numbers.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<BatchedRangeProofContext>` and assigned to the ZkToken proof program
    /// prior to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `BatchedRangeProof256Data`
    ///
    VerifyBatchedRangeProofU256,

    /// Verify a ciphertext-commitment equality proof.
    ///
    /// This instruction can be configured to optionally initialize a proof context state account.
    /// If creating a context state account, an account must be pre-allocated to the exact size of
    /// `ProofContextState<CiphertextCommitmentEqualityProofContext>` and assigned to the ZkToken
    /// proof program prior to the execution of this instruction.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Creating a proof context account
    ///   0. `[writable]` The proof context account
    ///   1. `[]` The proof context account owner
    ///
    ///   * Otherwise
    ///   None
    ///
    /// Data expected by this instruction:
    ///   `CiphertextCommitmentEqualityProofData`
    ///
    VerifyCiphertextCommitmentEquality,
}

/// Pubkeys associated with a context state account to be used as parameters to functions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextStateInfo<'a> {
    pub context_state_account: &'a Pubkey,
    pub context_state_authority: &'a Pubkey,
}

/// Create a `CloseContextState` instruction.
pub fn close_context_state(
    context_state_info: ContextStateInfo,
    destination_account: &Pubkey,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new(*context_state_info.context_state_account, false),
        AccountMeta::new(*destination_account, false),
        AccountMeta::new_readonly(*context_state_info.context_state_authority, true),
    ];

    let data = vec![ToPrimitive::to_u8(&ProofInstruction::CloseContextState).unwrap()];

    Instruction {
        program_id: crate::zk_token_proof_program::id(),
        accounts,
        data,
    }
}

/// Create a `VerifyZeroBalance` instruction.
pub fn verify_zero_balance(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &ZeroBalanceProofData,
) -> Instruction {
    ProofInstruction::VerifyZeroBalance.encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyWithdraw` instruction.
pub fn verify_withdraw(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &WithdrawData,
) -> Instruction {
    ProofInstruction::VerifyWithdraw.encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyCiphertextCiphertextEquality` instruction.
pub fn verify_ciphertext_ciphertext_equality(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &CiphertextCiphertextEqualityProofData,
) -> Instruction {
    ProofInstruction::VerifyCiphertextCiphertextEquality
        .encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyTransfer` instruction.
pub fn verify_transfer(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &TransferData,
) -> Instruction {
    ProofInstruction::VerifyTransfer.encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyTransferWithFee` instruction.
pub fn verify_transfer_with_fee(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &TransferWithFeeData,
) -> Instruction {
    ProofInstruction::VerifyTransferWithFee.encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyPubkeyValidity` instruction.
pub fn verify_pubkey_validity(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &PubkeyValidityData,
) -> Instruction {
    ProofInstruction::VerifyPubkeyValidity.encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyRangeProofU64` instruction.
pub fn verify_range_proof_u64(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &RangeProofU64Data,
) -> Instruction {
    ProofInstruction::VerifyRangeProofU64.encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyBatchedRangeProofU64` instruction.
pub fn verify_batched_verify_range_proof_u64(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &BatchedRangeProofU64Data,
) -> Instruction {
    ProofInstruction::VerifyBatchedRangeProofU64.encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyBatchedRangeProofU128` instruction.
pub fn verify_batched_verify_range_proof_u128(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &BatchedRangeProofU128Data,
) -> Instruction {
    ProofInstruction::VerifyBatchedRangeProofU128
        .encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyBatchedRangeProofU256` instruction.
pub fn verify_batched_verify_range_proof_u256(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &BatchedRangeProofU256Data,
) -> Instruction {
    ProofInstruction::VerifyBatchedRangeProofU256
        .encode_verify_proof(context_state_info, proof_data)
}

/// Create a `VerifyCiphertextCommitmentEquality` instruction.
pub fn verify_ciphertext_commitment_equality(
    context_state_info: Option<ContextStateInfo>,
    proof_data: &PubkeyValidityData,
) -> Instruction {
    ProofInstruction::VerifyCiphertextCommitmentEquality
        .encode_verify_proof(context_state_info, proof_data)
}

impl ProofInstruction {
    pub fn encode_verify_proof<T, U>(
        &self,
        context_state_info: Option<ContextStateInfo>,
        proof_data: &T,
    ) -> Instruction
    where
        T: Pod + ZkProofData<U>,
        U: Pod,
    {
        let accounts = if let Some(context_state_info) = context_state_info {
            vec![
                AccountMeta::new(*context_state_info.context_state_account, false),
                AccountMeta::new_readonly(*context_state_info.context_state_authority, false),
            ]
        } else {
            vec![]
        };

        let mut data = vec![ToPrimitive::to_u8(self).unwrap()];
        data.extend_from_slice(bytes_of(proof_data));

        Instruction {
            program_id: crate::zk_token_proof_program::id(),
            accounts,
            data,
        }
    }

    pub fn instruction_type(input: &[u8]) -> Option<Self> {
        input
            .first()
            .and_then(|instruction| FromPrimitive::from_u8(*instruction))
    }

    pub fn proof_data<T, U>(input: &[u8]) -> Option<&T>
    where
        T: Pod + ZkProofData<U>,
        U: Pod,
    {
        input
            .get(1..)
            .and_then(|data| bytemuck::try_from_bytes(data).ok())
    }
}
