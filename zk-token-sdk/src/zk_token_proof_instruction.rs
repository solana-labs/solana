//! Instructions provided by the [`ZK Token proof`] program.
//!
//! There are two types of instructions in the proof program: proof verification instructions
//! and the `CloseContextState` instruction.
//!
//! Each proof verification instruction verifies a certain type of zero-knowledge proof. These
//! instructions are processed by the program in two steps:
//!   1. The program verifies the zero-knowledge proof.
//!   2. The program optionally stores the context component of the instruction data to a
//!      dedicated [`context-state`] account.
//! If no accounts are provided with the instruction, the program simply verifies the proofs. If
//! accounts are provided with the instruction, then the program writes the context data to the
//! specified context-state account.
//!
//! NOTE: A context-state account must be pre-allocated to the exact size of the context data that
//! is expected for a proof type before it is included in a proof verification instruction.
//!
//! The `CloseContextState` instruction closes a context state account. A transaction containing
//! this instruction must be signed by the context account's owner. This instruction can be used by
//! the account owner to reclaim lamports for storage.
//!
//! [`ZK Token proof`]: https://edge.docs.solana.com/developing/runtime-facilities/zk-token-proof
//! [`context-state`]: https://edge.docs.solana.com/developing/runtime-facilities/zk-token-proof#context-data

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
    /// A zero-balance proof certifies that an ElGamal ciphertext encrypts the value zero.
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
    /// This proof is a collection of smaller proofs that are required by the SPL Token 2022
    /// confidential extension `Withdraw` instruction.
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
    /// A ciphertext-ciphertext equality proof certifies that two ElGamal ciphertexts encrypt the
    /// same message.
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
    /// This proof is a collection of smaller proofs that are required by the SPL Token 2022
    /// confidential extension `Transfer` instruction with transfer fees.
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
    /// This proof is a collection of smaller proofs that are required by the SPL Token 2022
    /// confidential extension `Transfer` instruction without transfer fees.
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

    /// Verify a public key validity zero-knowledge proof.
    ///
    /// A public key validity proof certifies that an ElGamal public key is well-formed and the
    /// prover knows the corresponding secret key.
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
    /// A ciphertext-commitment equality proof certifies that an ElGamal ciphertext and a Pedersen
    /// commitment encrypt/encode the same message.
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

    /// Verify a grouped-ciphertext validity proof.
    ///
    /// A grouped-ciphertext validity proof certifies that a grouped ElGamal ciphertext is
    /// well-defined, i.e. the ciphertext can be decrypted by private keys associated with its
    /// decryption handles.
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
    ///   `GroupedCiphertextValidityProofContext`
    ///
    VerifyGroupedCiphertext2HandlesValidity,

    /// Verify a batched grouped-ciphertext validity proof.
    ///
    /// A batched grouped-ciphertext validity proof certifies the validity of two grouped ElGamal
    /// ciphertext that are encrypted using the same set of ElGamal public keys. A batched
    /// grouped-ciphertext validity proof is shorter and more efficient than two individual
    /// grouped-ciphertext validity proofs.
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
    ///   `BatchedGroupedCiphertextValidityProofContext`
    ///
    VerifyBatchedGroupedCiphertext2HandlesValidity,

    /// Verify a fee sigma proof.
    ///
    /// A fee sigma proof certifies that a Pedersen commitment that encodes a transfer fee for SPL
    /// Token 2022 is well-formed.
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
    ///   `FeeSigmaProofData`
    ///
    VerifyFeeSigma,
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
