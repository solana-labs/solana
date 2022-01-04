#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::{
        elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
        pedersen::PedersenBase,
    },
    curve25519_dalek::traits::MultiscalarMul,
    rand::rngs::OsRng,
};
use {
    crate::{sigma_proofs::errors::ZeroBalanceProofError, transcript::TranscriptProtocol},
    arrayref::{array_ref, array_refs},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::IsIdentity,
    },
    merlin::Transcript,
};

#[allow(non_snake_case)]
#[derive(Clone)]
pub struct ZeroBalanceProof {
    pub Y_P: CompressedRistretto,
    pub Y_D: CompressedRistretto,
    pub z: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl ZeroBalanceProof {
    pub fn new(
        elgamal_keypair: &ElGamalKeypair,
        elgamal_ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Self {
        // extract the relevant scalar and Ristretto points from the input
        let P = elgamal_keypair.public.get_point();
        let s = elgamal_keypair.secret.get_scalar();

        let C = elgamal_ciphertext.message_comm.get_point();
        let D = elgamal_ciphertext.decrypt_handle.get_point();

        // record ElGamal pubkey and ciphertext in the transcript
        transcript.append_point(b"P", &P.compress());
        transcript.append_point(b"C", &C.compress());
        transcript.append_point(b"D", &D.compress());

        // generate a random masking factor that also serves as a nonce
        let y = Scalar::random(&mut OsRng);
        let Y_P = (y * P).compress();
        let Y_D = (y * D).compress();

        // record Y in transcript and receive a challenge scalar
        transcript.append_point(b"Y_P", &Y_P);
        transcript.append_point(b"Y_D", &Y_D);
        let c = transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        // compute the masked secret key
        let z = c * s + y;

        Self { Y_P, Y_D, z }
    }

    pub fn verify(
        self,
        elgamal_pubkey: &ElGamalPubkey,
        elgamal_ciphertext: &ElGamalCiphertext,
        transcript: &mut Transcript,
    ) -> Result<(), ZeroBalanceProofError> {
        // extract the relevant scalar and Ristretto points from the input
        let P = elgamal_pubkey.get_point();
        let C = elgamal_ciphertext.message_comm.get_point();
        let D = elgamal_ciphertext.decrypt_handle.get_point();

        let H = PedersenBase::default().H;

        // record ElGamal pubkey and ciphertext in the transcript
        transcript.validate_and_append_point(b"P", &P.compress())?;
        transcript.append_point(b"C", &C.compress());
        transcript.append_point(b"D", &D.compress());

        // record Y in transcript and receive challenge scalars
        transcript.validate_and_append_point(b"Y_P", &self.Y_P)?;
        transcript.append_point(b"Y_D", &self.Y_D);

        let c = transcript.challenge_scalar(b"c");
        let w = transcript.challenge_scalar(b"w"); // w used for multiscalar multiplication verification

        // decompress R or return verification error
        let Y_P = self.Y_P.decompress().ok_or(ZeroBalanceProofError::FormatError)?;
        let Y_D = self.Y_D.decompress().ok_or(ZeroBalanceProofError::FormatError)?;
        let z = self.z;

        // check the required algebraic relation
        let check = RistrettoPoint::multiscalar_mul(
            vec![z, -c, -Scalar::one(), w * z, -w * c, -w],
            vec![P, H, Y_P, D, C, Y_D],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(ZeroBalanceProofError::AlgebraicRelationError)
        }
    }

    pub fn to_bytes(&self) -> [u8; 96] {
        let mut buf = [0_u8; 96];
        buf[..32].copy_from_slice(self.Y_P.as_bytes());
        buf[32..64].copy_from_slice(self.Y_D.as_bytes());
        buf[64..96].copy_from_slice(self.z.as_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ZeroBalanceProofError> {
        let bytes = array_ref![bytes, 0, 96];
        let (Y_P, Y_D, z) = array_refs![bytes, 32, 32, 32];

        let Y_P = CompressedRistretto::from_slice(Y_P);
        let Y_D = CompressedRistretto::from_slice(Y_D);

        let z = Scalar::from_canonical_bytes(*z).ok_or(ZeroBalanceProofError::FormatError)?;

        Ok(ZeroBalanceProof { Y_P, Y_D, z })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::encryption::{
        elgamal::ElGamalKeypair,
        pedersen::{Pedersen, PedersenDecryptHandle, PedersenOpening},
    };

    #[test]
    fn test_zero_balance_proof() {
        let source_keypair = ElGamalKeypair::default();

        let mut transcript_prover = Transcript::new(b"test");
        let mut transcript_verifier = Transcript::new(b"test");

        // general case: encryption of 0
        let elgamal_ciphertext = source_keypair.public.encrypt(0_u64);
        let proof =
            ZeroBalanceProof::new(&source_keypair, &elgamal_ciphertext, &mut transcript_prover);
        assert!(proof
            .verify(
                &source_keypair.public,
                &elgamal_ciphertext,
                &mut transcript_verifier
            )
            .is_ok());

        // general case: encryption of > 0
        let elgamal_ciphertext = source_keypair.public.encrypt(1_u64);
        let proof =
            ZeroBalanceProof::new(&source_keypair, &elgamal_ciphertext, &mut transcript_prover);
        assert!(proof
            .verify(
                &source_keypair.public,
                &elgamal_ciphertext,
                &mut transcript_verifier
            )
            .is_err());

        // // edge case: all zero ciphertext - such ciphertext should always be a valid encryption of 0
        let zeroed_ct = ElGamalCiphertext::default();
        let proof = ZeroBalanceProof::new(&source_keypair, &zeroed_ct, &mut transcript_prover);
        assert!(proof
            .verify(&source_keypair.public, &zeroed_ct, &mut transcript_verifier)
            .is_ok());

        // edge cases: only C or D is zero - such ciphertext is always invalid
        let zeroed_comm = Pedersen::with(0_u64, &PedersenOpening::default());
        let handle = elgamal_ciphertext.decrypt_handle;

        let zeroed_comm_ciphertext = ElGamalCiphertext {
            message_comm: zeroed_comm,
            decrypt_handle: handle,
        };

        let proof = ZeroBalanceProof::new(
            &source_keypair,
            &zeroed_comm_ciphertext,
            &mut transcript_prover,
        );
        assert!(proof
            .verify(
                &source_keypair.public,
                &zeroed_comm_ciphertext,
                &mut transcript_verifier
            )
            .is_err());

        let (zero_comm, _) = Pedersen::new(0_u64);
        let zeroed_handle_ciphertext = ElGamalCiphertext {
            message_comm: zero_comm,
            decrypt_handle: PedersenDecryptHandle::default(),
        };

        let proof = ZeroBalanceProof::new(
            &source_keypair,
            &zeroed_handle_ciphertext,
            &mut transcript_prover,
        );
        assert!(proof
            .verify(
                &source_keypair.public,
                &zeroed_handle_ciphertext,
                &mut transcript_verifier
            )
            .is_err());
    }
}
