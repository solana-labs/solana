use {
    crate::zk_token_elgamal::pod,
    bytemuck::{Pod, Zeroable},
};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
        errors::ProofError,
        instruction::Verifiable,
        sigma_proofs::zero_balance_proof::ZeroBalanceProof,
        transcript::TranscriptProtocol,
    },
    merlin::Transcript,
    std::convert::TryInto,
};

/// This struct includes the cryptographic proof *and* the account data information needed to verify
/// the proof
///
/// - The pre-instruction should call CloseAccountData::verify_proof(&self)
/// - The actual program should check that `balance` is consistent with what is
///   currently stored in the confidential token account
///
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct CloseAccountData {
    /// The source account ElGamal pubkey
    pub elgamal_pubkey: pod::ElGamalPubkey, // 32 bytes

    /// The source account available balance in encrypted form
    pub balance: pod::ElGamalCiphertext, // 64 bytes

    /// Proof that the source account available balance is zero
    pub proof: CloseAccountProof, // 64 bytes
}

#[cfg(not(target_arch = "bpf"))]
impl CloseAccountData {
    pub fn new(source_keypair: &ElGamalKeypair, balance: ElGamalCiphertext) -> Self {
        let proof = CloseAccountProof::new(source_keypair, &balance);

        CloseAccountData {
            elgamal_pubkey: source_keypair.public.into(),
            balance: balance.into(),
            proof,
        }
    }
}

#[cfg(not(target_arch = "bpf"))]
impl Verifiable for CloseAccountData {
    fn verify(&self) -> Result<(), ProofError> {
        let elgamal_pubkey = self.elgamal_pubkey.try_into()?;
        let balance = self.balance.try_into()?;
        self.proof.verify(&elgamal_pubkey, &balance)
    }
}

/// This struct represents the cryptographic proof component that certifies that the encrypted
/// balance is zero
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
#[allow(non_snake_case)]
pub struct CloseAccountProof {
    pub proof: pod::ZeroBalanceProof,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl CloseAccountProof {
    fn transcript_new() -> Transcript {
        Transcript::new(b"CloseAccountProof")
    }

    pub fn new(source_keypair: &ElGamalKeypair, balance: &ElGamalCiphertext) -> Self {
        let mut transcript = Self::transcript_new();
        // TODO: Add ciphertext to transcript

        // add a domain separator to record the start of the protocol
        transcript.close_account_proof_domain_sep();

        let proof = ZeroBalanceProof::new(source_keypair, balance, &mut transcript);

        CloseAccountProof {
            proof: proof.into(),
        }
    }

    pub fn verify(
        &self,
        elgamal_pubkey: &ElGamalPubkey,
        balance: &ElGamalCiphertext,
    ) -> Result<(), ProofError> {
        let mut transcript = Self::transcript_new();

        // add a domain separator to record the start of the protocol
        transcript.close_account_proof_domain_sep();

        // verify zero balance proof
        let proof: ZeroBalanceProof = self.proof.try_into()?;
        proof.verify(elgamal_pubkey, balance, &mut transcript)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::encryption::{
            elgamal::{DecryptHandle, ElGamalKeypair},
            pedersen::{Pedersen, PedersenOpening},
        },
    };

    #[test]
    fn test_close_account_correctness() {
        let source_keypair = ElGamalKeypair::new_rand();

        // general case: encryption of 0
        let balance = source_keypair.public.encrypt(0_u64);
        let proof = CloseAccountProof::new(&source_keypair, &balance);
        assert!(proof.verify(&source_keypair.public, &balance).is_ok());

        // general case: encryption of > 0
        let balance = source_keypair.public.encrypt(1_u64);
        let proof = CloseAccountProof::new(&source_keypair, &balance);
        assert!(proof.verify(&source_keypair.public, &balance).is_err());

        // // edge case: all zero ciphertext - such ciphertext should always be a valid encryption of 0
        let zeroed_ct: ElGamalCiphertext = pod::ElGamalCiphertext::zeroed().try_into().unwrap();
        let proof = CloseAccountProof::new(&source_keypair, &zeroed_ct);
        assert!(proof.verify(&source_keypair.public, &zeroed_ct).is_ok());

        // edge cases: only C or D is zero - such ciphertext is always invalid
        let zeroed_comm = Pedersen::with(0_u64, &PedersenOpening::default());
        let handle = balance.handle;

        let zeroed_comm_ciphertext = ElGamalCiphertext {
            commitment: zeroed_comm,
            handle,
        };

        let proof = CloseAccountProof::new(&source_keypair, &zeroed_comm_ciphertext);
        assert!(proof
            .verify(&source_keypair.public, &zeroed_comm_ciphertext)
            .is_err());

        let zeroed_handle_ciphertext = ElGamalCiphertext {
            commitment: balance.commitment,
            handle: DecryptHandle::default(),
        };

        let proof = CloseAccountProof::new(&source_keypair, &zeroed_handle_ciphertext);
        assert!(proof
            .verify(&source_keypair.public, &zeroed_handle_ciphertext)
            .is_err());
    }
}
