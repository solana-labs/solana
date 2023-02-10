use {
    crate::{zk_token_elgamal::pod::PodBool, zk_token_proof_instruction::ZkProofContext},
    bytemuck::{bytes_of, Pod, Zeroable},
    solana_program::{
        instruction::{InstructionError, InstructionError::InvalidAccountData},
        pubkey::Pubkey,
    },
    std::mem::size_of,
};

/// The proof context account state
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct ProofContextState<T: ZkProofContext> {
    /// The proof context authority that can close the account
    pub context_state_authority: Pubkey,
    /// If `true`, the proof ocntext account is already initialized
    pub is_initialized: PodBool,
    /// The proof context data
    pub proof_context: T,
}

// `bytemuck::Pod` cannot be derived for generic structs unless the struct is marked
// `repr(packed)`, which may cause unnecessary complications when referencing its fields. Directly
// mark `ProofContextState` as `Zeroable` and `Pod` since since none of its fields has an alignment
// requirement greater than 1 and therefore, guaranteed to be `packed`.
unsafe impl<T: ZkProofContext> Zeroable for ProofContextState<T> {}
unsafe impl<T: ZkProofContext> Pod for ProofContextState<T> {}

impl<T: ZkProofContext> ProofContextState<T> {
    pub fn encode(
        context_state_authority: &Pubkey,
        is_initialized: bool,
        proof_context: &T,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        buf.extend_from_slice(context_state_authority.as_ref());
        buf.push(is_initialized as u8);
        buf.extend_from_slice(bytes_of(proof_context));
        buf
    }

    /// Interpret a slice as a `ProofContextState`.
    ///
    /// This function requires a generic parameter. To access only the generic-independent fields
    /// in `ProofContextState` without a generic parameter, use
    /// `ProofContextStateMeta::try_from_bytes` instead.
    pub fn try_from_bytes(input: &[u8]) -> Result<&Self, InstructionError> {
        bytemuck::try_from_bytes(input).map_err(|_| InvalidAccountData)
    }
}

/// The `ProofContextState` without the proof context itself. This struct exists to facilitate the
/// decoding of generic-independent fields in `ProofContextState`.
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct ProofContextStateMeta {
    /// The proof context authority that can close the account
    pub context_state_authority: Pubkey,
    /// If `true`, the proof ocntext account is already initialized
    pub is_initialized: PodBool,
}

impl ProofContextStateMeta {
    pub fn try_from_bytes(input: &[u8]) -> Result<&Self, InstructionError> {
        input
            .get(..size_of::<ProofContextStateMeta>())
            .and_then(|data| bytemuck::try_from_bytes(data).ok())
            .ok_or(InvalidAccountData)
    }
}
