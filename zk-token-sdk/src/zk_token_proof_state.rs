use {
    crate::zk_token_proof_instruction::{ProofType, ZkProofContext},
    bytemuck::{bytes_of, Pod},
    num_traits::{FromPrimitive, ToPrimitive},
    solana_program::{
        instruction::{InstructionError, InstructionError::InvalidAccountData},
        pubkey::{Pubkey, PUBKEY_BYTES},
    },
};

// TODO
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct ProofContextState<T: Pod + ZkProofContext> {
    /// The zero-knowledge proof type
    pub proof_type: ProofType,
    /// The proof context authority that can close the account
    pub context_state_authority: Pubkey,
    /// The proof context data
    pub proof_context: T,
}

impl<T: Pod + ZkProofContext> ProofContextState<T> {
    pub fn encode(
        proof_type: ProofType,
        context_state_authority: &Pubkey,
        proof_context: &T,
    ) -> Vec<u8> {
        let mut buf = vec![ToPrimitive::to_u8(&proof_type).unwrap()];
        buf.extend_from_slice(&context_state_authority.to_bytes());
        buf.extend_from_slice(bytes_of(proof_context));
        buf
    }

    pub fn try_from_bytes(input: &[u8]) -> Result<Self, InstructionError> {
        let (proof_type, rest) = Self::decode_proof_type(input)?;
        let (context_state_authority, proof_context) = Self::decode_pubkey(rest)?;
        let proof_context =
            bytemuck::try_from_bytes::<T>(proof_context).map_err(|_| InvalidAccountData)?;

        Ok(Self {
            proof_type,
            context_state_authority,
            proof_context: *proof_context,
        })
    }

    fn decode_proof_type(input: &[u8]) -> Result<(ProofType, &[u8]), InstructionError> {
        let proof_type = input
            .first()
            .and_then(|b| FromPrimitive::from_u8(*b))
            .ok_or(InvalidAccountData)?;
        Ok((proof_type, &input[1..]))
    }

    fn decode_pubkey(input: &[u8]) -> Result<(Pubkey, &[u8]), InstructionError> {
        let pubkey = input
            .get(..PUBKEY_BYTES)
            .and_then(|pubkey| Pubkey::try_from(pubkey).ok())
            .ok_or(InvalidAccountData)?;
        Ok((pubkey, &input[PUBKEY_BYTES..]))
    }

    pub fn size() -> usize {
        T::LEN.saturating_add(1).saturating_add(PUBKEY_BYTES)
    }
}
