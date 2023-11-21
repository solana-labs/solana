//! The 64-bit batched range proof instruction.

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::pedersen::{PedersenCommitment, PedersenOpening},
        errors::ProofError,
        instruction::batched_range_proof::MAX_COMMITMENTS,
        range_proof::RangeProof,
    },
    std::convert::TryInto,
};
use {
    crate::{
        instruction::{batched_range_proof::BatchedRangeProofContext, ProofType, ZkProofData},
        zk_token_elgamal::pod,
    },
    bytemuck::{Pod, Zeroable},
};

/// The instruction data that is needed for the
/// `ProofInstruction::VerifyBatchedRangeProofU64` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct BatchedRangeProofU64Data {
    /// The context data for a batched range proof
    pub context: BatchedRangeProofContext,

    /// The batched range proof
    pub proof: pod::RangeProofU64,
}

#[cfg(not(target_os = "solana"))]
impl BatchedRangeProofU64Data {
    pub fn new(
        commitments: Vec<&PedersenCommitment>,
        amounts: Vec<u64>,
        bit_lengths: Vec<usize>,
        openings: Vec<&PedersenOpening>,
    ) -> Result<Self, ProofError> {
        // the sum of the bit lengths must be 64
        let batched_bit_length = bit_lengths
            .iter()
            .try_fold(0_usize, |acc, &x| acc.checked_add(x))
            .ok_or(ProofError::Generation)?;

        // `u64::BITS` is 64, which fits in a single byte and should not overflow to `usize` for an
        // overwhelming number of platforms. However, to be extra cautious, use `try_from` and
        // `unwrap` here. A simple case `u64::BITS as usize` can silently overflow.
        let expected_bit_length = usize::try_from(u64::BITS).unwrap();
        if batched_bit_length != expected_bit_length {
            return Err(ProofError::Generation);
        }

        let context =
            BatchedRangeProofContext::new(&commitments, &amounts, &bit_lengths, &openings)?;

        let mut transcript = context.new_transcript();
        let proof = RangeProof::new(amounts, bit_lengths, openings, &mut transcript)
            .map_err(|_| ProofError::Generation)?
            .try_into()?;

        Ok(Self { context, proof })
    }
}

impl ZkProofData<BatchedRangeProofContext> for BatchedRangeProofU64Data {
    const PROOF_TYPE: ProofType = ProofType::BatchedRangeProofU64;

    fn context_data(&self) -> &BatchedRangeProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let (commitments, bit_lengths) = self.context.try_into()?;
        let num_commitments = commitments.len();

        if num_commitments > MAX_COMMITMENTS || num_commitments != bit_lengths.len() {
            return Err(ProofError::IllegalCommitmentLength);
        }

        let mut transcript = self.context_data().new_transcript();
        let proof: RangeProof = self.proof.try_into()?;

        proof
            .verify(commitments.iter().collect(), bit_lengths, &mut transcript)
            .map_err(|e| e.into())
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::{
            encryption::pedersen::Pedersen,
            errors::{ProofType, ProofVerificationError},
        },
    };

    #[test]
    fn test_batched_range_proof_u64_instruction_correctness() {
        let amount_1 = 255_u64;
        let amount_2 = 77_u64;
        let amount_3 = 99_u64;
        let amount_4 = 99_u64;
        let amount_5 = 11_u64;
        let amount_6 = 33_u64;
        let amount_7 = 99_u64;
        let amount_8 = 99_u64;

        let (commitment_1, opening_1) = Pedersen::new(amount_1);
        let (commitment_2, opening_2) = Pedersen::new(amount_2);
        let (commitment_3, opening_3) = Pedersen::new(amount_3);
        let (commitment_4, opening_4) = Pedersen::new(amount_4);
        let (commitment_5, opening_5) = Pedersen::new(amount_5);
        let (commitment_6, opening_6) = Pedersen::new(amount_6);
        let (commitment_7, opening_7) = Pedersen::new(amount_7);
        let (commitment_8, opening_8) = Pedersen::new(amount_8);

        let proof_data = BatchedRangeProofU64Data::new(
            vec![
                &commitment_1,
                &commitment_2,
                &commitment_3,
                &commitment_4,
                &commitment_5,
                &commitment_6,
                &commitment_7,
                &commitment_8,
            ],
            vec![
                amount_1, amount_2, amount_3, amount_4, amount_5, amount_6, amount_7, amount_8,
            ],
            vec![8, 8, 8, 8, 8, 8, 8, 8],
            vec![
                &opening_1, &opening_2, &opening_3, &opening_4, &opening_5, &opening_6, &opening_7,
                &opening_8,
            ],
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());

        let amount_1 = 256_u64; // not representable as an 8-bit number
        let amount_2 = 77_u64;
        let amount_3 = 99_u64;
        let amount_4 = 99_u64;
        let amount_5 = 11_u64;
        let amount_6 = 33_u64;
        let amount_7 = 99_u64;
        let amount_8 = 99_u64;

        let (commitment_1, opening_1) = Pedersen::new(amount_1);
        let (commitment_2, opening_2) = Pedersen::new(amount_2);
        let (commitment_3, opening_3) = Pedersen::new(amount_3);
        let (commitment_4, opening_4) = Pedersen::new(amount_4);
        let (commitment_5, opening_5) = Pedersen::new(amount_5);
        let (commitment_6, opening_6) = Pedersen::new(amount_6);
        let (commitment_7, opening_7) = Pedersen::new(amount_7);
        let (commitment_8, opening_8) = Pedersen::new(amount_8);

        let proof_data = BatchedRangeProofU64Data::new(
            vec![
                &commitment_1,
                &commitment_2,
                &commitment_3,
                &commitment_4,
                &commitment_5,
                &commitment_6,
                &commitment_7,
                &commitment_8,
            ],
            vec![
                amount_1, amount_2, amount_3, amount_4, amount_5, amount_6, amount_7, amount_8,
            ],
            vec![8, 8, 8, 8, 8, 8, 8, 8],
            vec![
                &opening_1, &opening_2, &opening_3, &opening_4, &opening_5, &opening_6, &opening_7,
                &opening_8,
            ],
        )
        .unwrap();

        assert_eq!(
            proof_data.verify_proof().unwrap_err(),
            ProofError::VerificationError(
                ProofType::RangeProof,
                ProofVerificationError::AlgebraicRelation
            ),
        );
    }
}
