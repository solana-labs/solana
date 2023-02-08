use {
    crate::{
        zk_token_elgamal::pod::PodProofType,
        zk_token_proof_instruction::{ProofType, ZkProofContext},
    },
    bytemuck::{bytes_of, Pod, Zeroable},
    num_traits::ToPrimitive,
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
    /// The zero-knowledge proof type
    pub proof_type: PodProofType,
    /// The proof context authority that can close the account
    pub context_state_authority: Pubkey,
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
        proof_type: ProofType,
        context_state_authority: &Pubkey,
        proof_context: &T,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        buf.push(ToPrimitive::to_u8(&proof_type).unwrap());
        buf.extend_from_slice(context_state_authority.as_ref());
        buf.extend_from_slice(bytes_of(proof_context));
        buf
    }

    pub fn try_from_bytes(input: &[u8]) -> Result<&Self, InstructionError> {
        bytemuck::try_from_bytes(input).map_err(|_| InvalidAccountData)
    }
}
