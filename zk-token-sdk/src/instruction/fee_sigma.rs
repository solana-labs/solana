//! The fee sigma proof instruction.
//!
//! A fee sigma proof certifies that a Pedersen commitment to a transfer fee for SPL Token 2022 is
//! well-formed.
//!
//! A formal documentation of how transfer fees and fee sigma proof are computed can be found in
//! the [`ZK Token proof`] program documentation.
//!
//! [`ZK Token proof`]: https://edge.docs.solana.com/developing/runtime-facilities/zk-token-proof

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::pedersen::{PedersenCommitment, PedersenOpening},
        errors::ProofError,
        sigma_proofs::fee_proof::FeeSigmaProof,
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

/// The instruction data that is needed for the `ProofInstruction::VerifyFeeSigma` instruction.
///
/// It includes the cryptographic proof as well as the context data information needed to verify
/// the proof.
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeSigmaProofData {
    pub context: FeeSigmaProofContext,

    pub proof: pod::FeeSigmaProof,
}

/// The context data needed to verify a pubkey validity proof.
///
/// We refer to [`ZK Token proof`] for the formal details on how the fee sigma proof is computed.
///
/// [`ZK Token proof`]: https://edge.docs.solana.com/developing/runtime-facilities/zk-token-proof
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct FeeSigmaProofContext {
    /// The Pedersen commitment to the transfer fee
    pub fee_commitment: pod::PedersenCommitment,

    /// The Pedersen commitment to the real delta fee.
    pub delta_commitment: pod::PedersenCommitment,

    /// The Pedersen commitment to the claimed delta fee.
    pub claimed_commitment: pod::PedersenCommitment,

    /// The maximum cap for a transfer fee
    pub max_fee: pod::PodU64,
}

#[cfg(not(target_os = "solana"))]
impl FeeSigmaProofData {
    pub fn new(
        fee_commitment: &PedersenCommitment,
        delta_commitment: &PedersenCommitment,
        claimed_commitment: &PedersenCommitment,
        fee_opening: &PedersenOpening,
        delta_opening: &PedersenOpening,
        claimed_opening: &PedersenOpening,
        fee_amount: u64,
        delta_fee: u64,
        max_fee: u64,
    ) -> Result<Self, ProofError> {
        let pod_fee_commitment = pod::PedersenCommitment(fee_commitment.to_bytes());
        let pod_delta_commitment = pod::PedersenCommitment(delta_commitment.to_bytes());
        let pod_claimed_commitment = pod::PedersenCommitment(claimed_commitment.to_bytes());
        let pod_max_fee = max_fee.into();

        let context = FeeSigmaProofContext {
            fee_commitment: pod_fee_commitment,
            delta_commitment: pod_delta_commitment,
            claimed_commitment: pod_claimed_commitment,
            max_fee: pod_max_fee,
        };

        let mut transcript = context.new_transcript();

        let proof = FeeSigmaProof::new(
            (fee_amount, fee_commitment, fee_opening),
            (delta_fee, delta_commitment, delta_opening),
            (claimed_commitment, claimed_opening),
            max_fee,
            &mut transcript,
        )
        .into();

        Ok(Self { context, proof })
    }
}

impl ZkProofData<FeeSigmaProofContext> for FeeSigmaProofData {
    const PROOF_TYPE: ProofType = ProofType::FeeSigma;

    fn context_data(&self) -> &FeeSigmaProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = self.context.new_transcript();

        let fee_commitment = self.context.fee_commitment.try_into()?;
        let delta_commitment = self.context.delta_commitment.try_into()?;
        let claimed_commitment = self.context.claimed_commitment.try_into()?;
        let max_fee = self.context.max_fee.into();
        let proof: FeeSigmaProof = self.proof.try_into()?;

        proof
            .verify(
                &fee_commitment,
                &delta_commitment,
                &claimed_commitment,
                max_fee,
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

#[cfg(not(target_os = "solana"))]
impl FeeSigmaProofContext {
    fn new_transcript(&self) -> Transcript {
        let mut transcript = Transcript::new(b"FeeSigmaProof");
        transcript.append_commitment(b"fee-commitment", &self.fee_commitment);
        transcript.append_commitment(b"delta-commitment", &self.fee_commitment);
        transcript.append_commitment(b"claimed-commitment", &self.fee_commitment);
        transcript.append_u64(b"max-fee", self.max_fee.into());
        transcript
    }
}

#[cfg(test)]
mod test {
    use {super::*, crate::encryption::pedersen::Pedersen, curve25519_dalek::scalar::Scalar};

    #[test]
    fn test_fee_sigma_instruction_correctness() {
        // transfer fee amount is below max fee
        let transfer_amount: u64 = 1;
        let max_fee: u64 = 3;

        let fee_rate: u16 = 400;
        let fee_amount: u64 = 1;
        let delta_fee: u64 = 9600;

        let (transfer_commitment, transfer_opening) = Pedersen::new(transfer_amount);
        let (fee_commitment, fee_opening) = Pedersen::new(fee_amount);

        let scalar_rate = Scalar::from(fee_rate);
        let delta_commitment =
            &fee_commitment * Scalar::from(10_000_u64) - &transfer_commitment * &scalar_rate;
        let delta_opening =
            &fee_opening * &Scalar::from(10_000_u64) - &transfer_opening * &scalar_rate;

        let (claimed_commitment, claimed_opening) = Pedersen::new(delta_fee);

        let proof_data = FeeSigmaProofData::new(
            &fee_commitment,
            &delta_commitment,
            &claimed_commitment,
            &fee_opening,
            &delta_opening,
            &claimed_opening,
            fee_amount,
            delta_fee,
            max_fee,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());

        // transfer fee amount is equal to max fee
        let transfer_amount: u64 = 55;
        let max_fee: u64 = 3;

        let fee_rate: u16 = 555;
        let fee_amount: u64 = 4;

        let (transfer_commitment, transfer_opening) = Pedersen::new(transfer_amount);
        let (fee_commitment, fee_opening) = Pedersen::new(max_fee);

        let scalar_rate = Scalar::from(fee_rate);
        let delta_commitment =
            &fee_commitment * &Scalar::from(10000_u64) - &transfer_commitment * &scalar_rate;
        let delta_opening =
            &fee_opening * &Scalar::from(10000_u64) - &transfer_opening * &scalar_rate;

        let (claimed_commitment, claimed_opening) = Pedersen::new(0_u64);

        let proof_data = FeeSigmaProofData::new(
            &fee_commitment,
            &delta_commitment,
            &claimed_commitment,
            &fee_opening,
            &delta_opening,
            &claimed_opening,
            fee_amount,
            delta_fee,
            max_fee,
        )
        .unwrap();

        assert!(proof_data.verify_proof().is_ok());
    }
}
