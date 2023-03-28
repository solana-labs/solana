#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{DecryptHandle, ElGamalPubkey},
            pedersen::{PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
        sigma_proofs::validity_proof::{AggregatedValidityProof, ValidityProof},
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
};
use {
    crate::{
        instruction::{ProofType, ZkProofData},
        zk_token_elgamal::pod,
    },
    bytemuck::{Pod, Zeroable},
};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ValidityProofData {
    /// The context data for the (ciphertext) validity proof instruction
    pub context: ValidityProofContext, // 160 bytes

    /// Proof that an ElGamal ciphertext is valid
    pub proof: pod::ValidityProof, // 160 bytes
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ValidityProofContext {
    /// The destination pubkey
    pub destination_pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The auditor pubkey
    pub auditor_pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The commitment that the proof validates
    pub commitment: pod::PedersenCommitment, // 32 bytes

    /// The destination decryption handle
    pub destination_handle: pod::DecryptHandle, // 32 bytes

    /// The auditor decryption handles
    pub auditor_handle: pod::DecryptHandle, // 32 bytes
}

#[cfg(not(target_os = "solana"))]
impl ValidityProofData {
    pub fn new(
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        commitment: &PedersenCommitment,
        destination_handle: &DecryptHandle,
        auditor_handle: &DecryptHandle,
        amount: u64,
        opening: &PedersenOpening,
    ) -> Result<Self, ProofError> {
        let context = ValidityProofContext {
            destination_pubkey: pod::ElGamalPubkey(destination_pubkey.to_bytes()),
            auditor_pubkey: pod::ElGamalPubkey(auditor_pubkey.to_bytes()),
            commitment: pod::PedersenCommitment(commitment.to_bytes()),
            destination_handle: pod::DecryptHandle(destination_handle.to_bytes()),
            auditor_handle: pod::DecryptHandle(auditor_handle.to_bytes()),
        };

        let mut transcript = ValidityProof::transcript_new(&context);

        let proof = ValidityProof::new(
            (destination_pubkey, auditor_pubkey),
            amount,
            opening,
            &mut transcript,
        );

        Ok(ValidityProofData {
            context,
            proof: proof.into(),
        })
    }
}

impl ZkProofData<ValidityProofContext> for ValidityProofData {
    const PROOF_TYPE: ProofType = ProofType::ValidityProof;

    fn context_data(&self) -> &ValidityProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = ValidityProof::transcript_new(&self.context);

        let destination_pubkey = self.context.destination_pubkey.try_into()?;
        let auditor_pubkey = self.context.auditor_pubkey.try_into()?;
        let commitment = self.context.commitment.try_into()?;
        let destination_handle = self.context.destination_handle.try_into()?;
        let auditor_handle = self.context.auditor_handle.try_into()?;

        let proof: ValidityProof = self.proof.try_into()?;
        proof
            .verify(
                &commitment,
                (&destination_pubkey, &auditor_pubkey),
                (&destination_handle, &auditor_handle),
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

impl ValidityProof {
    fn transcript_new(context: &ValidityProofContext) -> Transcript {
        let mut transcript = Transcript::new(b"ValidityProof");

        transcript.append_pubkey(b"pubkey", &context.destination_pubkey);
        transcript.append_pubkey(b"pubkey", &context.auditor_pubkey);
        transcript.append_commitment(b"commitment", &context.commitment);
        transcript.append_handle(b"decrypt_handle", &context.destination_handle);
        transcript.append_handle(b"decrypt_handle", &context.auditor_handle);

        transcript
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct AggregatedValidityProofData {
    /// The context data for the aggregate validity proof instruction
    pub context: AggregatedValidityProofContext, // 32 bytes

    /// Proof that a pair of ciphertexts are valid
    pub proof: pod::AggregatedValidityProof, // 256 bytes
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct AggregatedValidityProofContext {
    /// The destination pubkey
    pub destination_pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The auditor pubkey
    pub auditor_pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The first commitment that the proof validates
    pub commitment_lo: pod::PedersenCommitment, // 32 bytes

    /// The second commitment that the proof validates
    pub commitment_hi: pod::PedersenCommitment, // 32 bytes

    /// The destination decryption handle for the first commitment
    pub destination_handle_lo: pod::DecryptHandle, // 32 bytes

    /// The destination decryption handle for the second commitment
    pub destination_handle_hi: pod::DecryptHandle, // 32 bytes

    /// The auditor decryption handle for the first commitment
    pub auditor_handle_lo: pod::DecryptHandle, // 32 bytes

    /// The auditor decryption handle for the second commitment
    pub auditor_handle_hi: pod::DecryptHandle, // 32 bytes
}

#[cfg(not(target_os = "solana"))]
#[allow(clippy::too_many_arguments)]
impl AggregatedValidityProofData {
    pub fn new(
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        commitment_lo: &PedersenCommitment,
        commitment_hi: &PedersenCommitment,
        destination_handle_lo: &DecryptHandle,
        destination_handle_hi: &DecryptHandle,
        auditor_handle_lo: &DecryptHandle,
        auditor_handle_hi: &DecryptHandle,
        amount_lo: u64,
        amount_hi: u64,
        opening_lo: &PedersenOpening,
        opening_hi: &PedersenOpening,
    ) -> Result<Self, ProofError> {
        let context = AggregatedValidityProofContext {
            destination_pubkey: pod::ElGamalPubkey(destination_pubkey.to_bytes()),
            auditor_pubkey: pod::ElGamalPubkey(auditor_pubkey.to_bytes()),
            commitment_lo: pod::PedersenCommitment(commitment_lo.to_bytes()),
            commitment_hi: pod::PedersenCommitment(commitment_hi.to_bytes()),
            destination_handle_lo: pod::DecryptHandle(destination_handle_lo.to_bytes()),
            destination_handle_hi: pod::DecryptHandle(destination_handle_hi.to_bytes()),
            auditor_handle_lo: pod::DecryptHandle(auditor_handle_lo.to_bytes()),
            auditor_handle_hi: pod::DecryptHandle(auditor_handle_hi.to_bytes()),
        };

        let mut transcript = AggregatedValidityProof::transcript_new(&context);

        let proof = AggregatedValidityProof::new(
            (destination_pubkey, auditor_pubkey),
            (amount_lo, amount_hi),
            (opening_lo, opening_hi),
            &mut transcript,
        );

        Ok(AggregatedValidityProofData {
            context,
            proof: proof.into(),
        })
    }
}

impl ZkProofData<AggregatedValidityProofContext> for AggregatedValidityProofData {
    const PROOF_TYPE: ProofType = ProofType::AggregatedValidityProof;

    fn context_data(&self) -> &AggregatedValidityProofContext {
        &self.context
    }

    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofError> {
        let mut transcript = AggregatedValidityProof::transcript_new(&self.context);

        let destination_pubkey = self.context.destination_pubkey.try_into()?;
        let auditor_pubkey = self.context.auditor_pubkey.try_into()?;
        let commitment_lo = self.context.commitment_lo.try_into()?;
        let commitment_hi = self.context.commitment_hi.try_into()?;
        let destination_handle_lo = self.context.destination_handle_lo.try_into()?;
        let destination_handle_hi = self.context.destination_handle_hi.try_into()?;
        let auditor_handle_lo = self.context.auditor_handle_lo.try_into()?;
        let auditor_handle_hi = self.context.auditor_handle_hi.try_into()?;

        let proof: AggregatedValidityProof = self.proof.try_into()?;
        proof
            .verify(
                (&destination_pubkey, &auditor_pubkey),
                (&commitment_lo, &commitment_hi),
                (&destination_handle_lo, &destination_handle_hi),
                (&auditor_handle_lo, &auditor_handle_hi),
                &mut transcript,
            )
            .map_err(|e| e.into())
    }
}

impl AggregatedValidityProof {
    fn transcript_new(context: &AggregatedValidityProofContext) -> Transcript {
        let mut transcript = Transcript::new(b"AggregatedValidityProof");

        transcript.append_pubkey(b"pubkey", &context.destination_pubkey);
        transcript.append_pubkey(b"pubkey", &context.auditor_pubkey);
        transcript.append_commitment(b"commitment", &context.commitment_lo);
        transcript.append_commitment(b"commitment", &context.commitment_hi);
        transcript.append_handle(b"decrypt_handle", &context.destination_handle_lo);
        transcript.append_handle(b"decrypt_handle", &context.destination_handle_hi);
        transcript.append_handle(b"decrypt_handle", &context.auditor_handle_lo);
        transcript.append_handle(b"decrypt_handle", &context.auditor_handle_hi);

        transcript
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::encryption::{elgamal::ElGamalKeypair, pedersen::Pedersen},
    };

    #[test]
    fn test_validity_proof_correctness() {
        let destination_pubkey = ElGamalKeypair::new_rand().public;
        let auditor_pubkey = ElGamalKeypair::new_rand().public;

        let amount: u64 = 55;
        let (commitment, opening) = Pedersen::new(amount);

        let destination_handle = destination_pubkey.decrypt_handle(&opening);
        let auditor_handle = auditor_pubkey.decrypt_handle(&opening);

        let validity_proof_data = ValidityProofData::new(
            &destination_pubkey,
            &auditor_pubkey,
            &commitment,
            &destination_handle,
            &auditor_handle,
            amount,
            &opening,
        )
        .unwrap();
        assert!(validity_proof_data.verify_proof().is_ok());
    }

    #[test]
    fn test_aggregated_validity_proof_correctness() {
        let destination_pubkey = ElGamalKeypair::new_rand().public;
        let auditor_pubkey = ElGamalKeypair::new_rand().public;

        let amount_lo: u64 = 55;
        let amount_hi: u64 = 77;

        let (commitment_lo, opening_lo) = Pedersen::new(amount_lo);
        let (commitment_hi, opening_hi) = Pedersen::new(amount_hi);

        let destination_handle_lo = destination_pubkey.decrypt_handle(&opening_lo);
        let destination_handle_hi = destination_pubkey.decrypt_handle(&opening_hi);

        let auditor_handle_lo = auditor_pubkey.decrypt_handle(&opening_lo);
        let auditor_handle_hi = auditor_pubkey.decrypt_handle(&opening_hi);

        let aggregated_validity_proof_data = AggregatedValidityProofData::new(
            &destination_pubkey,
            &auditor_pubkey,
            &commitment_lo,
            &commitment_hi,
            &destination_handle_lo,
            &destination_handle_hi,
            &auditor_handle_lo,
            &auditor_handle_hi,
            amount_lo,
            amount_hi,
            &opening_lo,
            &opening_hi,
        )
        .unwrap();
        assert!(aggregated_validity_proof_data.verify_proof().is_ok());
    }
}
