//! The range proof instruction.
//!
//! A range proof certifies that a committed value in a Pedersen commitment is a number from a
//! certain range. Currently, only 64-bit range proof `VerifyRangeProofU64` is supported in the
//! proof program. It certifies that a committed number is an unsigned 64-bit number.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::pedersen::{PedersenCommitment, PedersenOpening},
        errors::ProofError,
        range_proof::RangeProof,
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
    std::convert::TryInto,
};
use {
    crate::{
        instruction::{ProofType, ZkProofData},
        zk_token_elgamal::pod,
    },
    bytemuck::{Pod, Zeroable},
};

/// The context data needed to verify a range-proof for a committed value in a Pedersen commitment.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct RangeProofContext {
    pub commitment: pod::PedersenCommitment, // 32 bytes
}

/// The instruction data that is needed for the `ProofInstruction::VerifyRangeProofU64` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct RangeProofU64Data {
    /// The context data for a range proof
    pub context: RangeProofContext,

    /// The proof that a committed value in a Pedersen commitment is a 64-bit value
    pub proof: pod::RangeProofU64,
}

#[cfg(not(target_os = "solana"))]
impl RangeProofU64Data {
    pub fn new(
        commitment: &PedersenCommitment,
        amount: u64,
        opening: &PedersenOpening,
    ) -> Result<Self, ProofError> {
        let pod_commitment = pod::PedersenCommitment(commitment.to_bytes());

        let context = RangeProofContext {
            commitment: pod_commitment,
        };

        let mut transcript = context.new_transcript();

        // `u64::BITS` is 64, which fits in a single byte and should not overflow to `usize` for an
        // overwhelming number of platforms. However, to be extra cautious, use `try_from` and
        // `unwrap` here. A simple case `u64::BITS as usize` can silently overflow.
        let bit_size = usize::try_from(u64::BITS).unwrap();

        let proof = RangeProof::new(vec![amount], vec![bit_size], vec![opening], &mut transcript)
            .map_err(|_| ProofError::Generation)?
            .try_into()?;

        Ok(Self { context, proof })
    }
}

impl ZkProofData<RangeProofContext> for RangeProofU64Data {
    const PROOF_TYPE: ProofType = ProofType::RangeProofU64;

    fn context_data(&self) -> &RangeProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = self.context_data().new_transcript();
        let commitment = self.context.commitment.try_into()?;
        let proof: RangeProof = self.proof.try_into()?;

        let bit_size = usize::try_from(u64::BITS).unwrap();
        proof
            .verify(vec![&commitment], vec![bit_size], &mut transcript)
            .map_err(|e| e.into())
    }
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl RangeProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"RangeProof");
        transcript.append_commitment(b"commitment", &self.commitment);
        transcript
    }
}

#[cfg(test)]
mod test {
    use {super::*, crate::encryption::pedersen::Pedersen};

    #[test]
    fn test_range_proof_64_instruction_correctness() {
        let amount = std::u64::MAX;
        let (commitment, opening) = Pedersen::new(amount);

        let proof_data = RangeProofU64Data::new(&commitment, amount, &opening).unwrap();
        assert!(proof_data.verify_proof().is_ok());
    }
}
