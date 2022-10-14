//! The public-key (validity) proof system.
//!
//! A public-key proof is defined with respect to an ElGamal public key. The proof certifies that a
//! given public key is a valid ElGamal public key (i.e. the prover knows a corresponding secret
//! key). To generate the proof, a prover must prove the secret key for the public key.
//!
//! The protocol guarantees computational soundness (by the hardness of discrete log) and perfect
//! zero-knowledge in the random oracle model.

#[cfg(not(target_os = "solana"))]
use {
    crate::encryption::elgamal::{ElGamalKeypair, ElGamalPubkey},
    rand::rngs::OsRng,
    zeroize::Zeroize,
};
use {
    crate::{sigma_proofs::errors::PubkeyProofError, transcript::TranscriptProtocol},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::IsIdentity,
    },
    merlin::Transcript,
};

/// Public-key proof.
///
/// Contains all the elliptic curve and scalar components that make up the sigma protocol.
#[allow(non_snake_case)]
#[derive(Clone)]
pub struct PubkeyProof {
    Y: CompressedRistretto,
    z: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_os = "solana"))]
impl PubkeyProof {
    pub fn new(elgamal_keypair: &ElGamalKeypair, transcript: &mut Transcript) -> Self {
        unimplemented!()
    }

    pub fn verify(
        self,
        elgamal_pubkey: &ElGamalPubkey,
        transcript: &mut Transcript,
    ) -> Result<(), PubkeyProof> {
        unimplemented!()
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        unimplemented!()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PubkeyProofError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_pubkey_proof_correctness() {
        unimplemented!()
    }

    #[test]
    fn test_pubkey_proof_edge_cases() {
        unimplemented!()
    }
}
