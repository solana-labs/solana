#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::{DecryptHandle, ElGamalPubkey},
            pedersen::{PedersenCommitment, PedersenOpening},
        },
        errors::ProofError,
        sigma_proofs::validity_proof::ValidityProof,
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
    pub context: ValidityProofContext, // 32 bytes

    /// Proof that an ElGamal ciphertext is valid
    pub proof: pod::ValidityProof, // 160 bytes
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ValidityProofContext {
    /// The ciphertext that the proof validates
    pub commitment: pod::PedersenCommitment, // 32 bytes
    /// The destination pubkey
    pub destination_pubkey: pod::ElGamalPubkey, // 32 bytes
    /// The auditor pubkey
    pub auditor_pubkey: pod::ElGamalPubkey, // 32 bytes
    /// The destination decryption handle
    pub destination_handle: pod::DecryptHandle,
    /// The auditor decryption handles
    pub auditor_handle: pod::DecryptHandle,
}

#[cfg(not(target_os = "solana"))]
impl ValidityProofData {
    pub fn new(
        commitment: &PedersenCommitment,
        destination_pubkey: &ElGamalPubkey,
        auditor_pubkey: &ElGamalPubkey,
        destination_handle: &DecryptHandle,
        auditor_handle: &DecryptHandle,
        amount: u64,
        opening: &PedersenOpening,
    ) -> Result<Self, ProofError> {
        let context = ValidityProofContext {
            commitment: pod::PedersenCommitment(commitment.to_bytes()),
            destination_pubkey: pod::ElGamalPubkey(destination_pubkey.to_bytes()),
            auditor_pubkey: pod::ElGamalPubkey(auditor_pubkey.to_bytes()),
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

        let commitment = self.context.commitment.try_into()?;
        let destination_pubkey = self.context.destination_pubkey.try_into()?;
        let auditor_pubkey = self.context.auditor_pubkey.try_into()?;
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

        transcript.append_commitment(b"commitment", &context.commitment);
        transcript.append_pubkey(b"pubkey", &context.destination_pubkey);
        transcript.append_pubkey(b"pubkey", &context.auditor_pubkey);
        transcript.append_handle(b"decrypt_handle", &context.destination_handle);
        transcript.append_handle(b"decrypt_handle", &context.auditor_handle);

        transcript
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::encryption::{elgamal::ElGamalKeypair, pedersen::Pedersen};

    #[test]
    fn test_validity_proof_correctness() {
        let destination_pubkey = ElGamalKeypair::new_rand().public;
        let auditor_pubkey = ElGamalKeypair::new_rand().public;

        let amount: u64 = 55;
        let (commitment, opening) = Pedersen::new(amount);

        let destination_handle = destination_pubkey.decrypt_handle(&opening);
        let auditor_handle = auditor_pubkey.decrypt_handle(&opening);

        let validity_proof_data = ValidityProofData::new(
            &commitment,
            &destination_pubkey,
            &auditor_pubkey,
            &destination_handle,
            &auditor_handle,
            amount,
            &opening,
        )
        .unwrap();
        assert!(validity_proof_data.verify_proof().is_ok());
    }
}
