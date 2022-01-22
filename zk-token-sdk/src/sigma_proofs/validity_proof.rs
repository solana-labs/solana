#[cfg(not(target_arch = "bpf"))]
use {
    crate::encryption::{
        elgamal::{DecryptHandle, ElGamalPubkey},
        pedersen::{PedersenCommitment, PedersenOpening, G, H},
    },
    curve25519_dalek::traits::MultiscalarMul,
    rand::rngs::OsRng,
};
use {
    crate::{sigma_proofs::errors::ValidityProofError, transcript::TranscriptProtocol},
    arrayref::{array_ref, array_refs},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::{IsIdentity, VartimeMultiscalarMul},
    },
    merlin::Transcript,
};

#[allow(non_snake_case)]
#[derive(Clone)]
pub struct ValidityProof {
    pub Y_0: CompressedRistretto,
    pub Y_1: CompressedRistretto,
    pub Y_2: CompressedRistretto,
    pub z_r: Scalar,
    pub z_x: Scalar,
}

#[allow(non_snake_case)]
#[cfg(not(target_arch = "bpf"))]
impl ValidityProof {
    pub fn new(
        elgamal_pubkey_dest: &ElGamalPubkey,
        elgamal_pubkey_auditor: &ElGamalPubkey,
        messages: (u64, u64),
        openings: (&PedersenOpening, &PedersenOpening),
        transcript: &mut Transcript,
    ) -> Self {
        // extract the relevant scalar and Ristretto points from the inputs
        let P_dest = elgamal_pubkey_dest.get_point();
        let P_auditor = elgamal_pubkey_auditor.get_point();

        // generate random masking factors that also serves as a nonce
        let y_r = Scalar::random(&mut OsRng);
        let y_x = Scalar::random(&mut OsRng);

        let Y_0 = RistrettoPoint::multiscalar_mul(vec![y_r, y_x], vec![&(*H), &(*G)]).compress();
        let Y_1 = (y_r * P_dest).compress();
        let Y_2 = (y_r * P_auditor).compress();

        // record masking factors in transcript and get challenges
        transcript.append_point(b"Y_0", &Y_0);
        transcript.append_point(b"Y_1", &Y_1);
        transcript.append_point(b"Y_2", &Y_2);

        let t = transcript.challenge_scalar(b"t");
        let c = transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        // aggregate lo and hi messages and openings
        let x = Scalar::from(messages.0) + t * Scalar::from(messages.1);
        let r = openings.0.get_scalar() + t * openings.1.get_scalar();

        // compute masked message and opening
        let z_r = c * r + y_r;
        let z_x = c * x + y_x;

        Self {
            Y_0,
            Y_1,
            Y_2,
            z_r,
            z_x,
        }
    }

    pub fn verify(
        self,
        elgamal_pubkey_dest: &ElGamalPubkey,
        elgamal_pubkey_auditor: &ElGamalPubkey,
        commitments: (&PedersenCommitment, &PedersenCommitment),
        handle_dest: (&DecryptHandle, &DecryptHandle),
        handle_auditor: (&DecryptHandle, &DecryptHandle),
        transcript: &mut Transcript,
    ) -> Result<(), ValidityProofError> {
        // include Y_0, Y_1, Y_2 to transcript and extract challenges
        transcript.validate_and_append_point(b"Y_0", &self.Y_0)?;
        transcript.validate_and_append_point(b"Y_1", &self.Y_1)?;
        transcript.validate_and_append_point(b"Y_2", &self.Y_2)?;

        let t = transcript.challenge_scalar(b"t");
        let c = transcript.challenge_scalar(b"c");
        let w = transcript.challenge_scalar(b"w");
        let ww = w * w;

        // check the required algebraic conditions
        let Y_0 = self.Y_0.decompress().ok_or(ValidityProofError::Format)?;
        let Y_1 = self.Y_1.decompress().ok_or(ValidityProofError::Format)?;
        let Y_2 = self.Y_2.decompress().ok_or(ValidityProofError::Format)?;

        let P_dest = elgamal_pubkey_dest.get_point();
        let P_auditor = elgamal_pubkey_auditor.get_point();

        let C = commitments.0.get_point() + t * commitments.1.get_point();
        let D_dest = handle_dest.0.get_point() + t * handle_dest.1.get_point();
        let D_auditor = handle_auditor.0.get_point() + t * handle_auditor.1.get_point();

        let check = RistrettoPoint::vartime_multiscalar_mul(
            vec![
                self.z_r,
                self.z_x,
                -c,
                -Scalar::one(),
                w * self.z_r,
                -w * c,
                -w,
                ww * self.z_r,
                -ww * c,
                -ww,
            ],
            vec![
                &(*H),
                &(*G),
                &C,
                &Y_0,
                P_dest,
                &D_dest,
                &Y_1,
                P_auditor,
                &D_auditor,
                &Y_2,
            ],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(ValidityProofError::AlgebraicRelation)
        }
    }

    pub fn to_bytes(&self) -> [u8; 160] {
        let mut buf = [0_u8; 160];
        buf[..32].copy_from_slice(self.Y_0.as_bytes());
        buf[32..64].copy_from_slice(self.Y_1.as_bytes());
        buf[64..96].copy_from_slice(self.Y_2.as_bytes());
        buf[96..128].copy_from_slice(self.z_r.as_bytes());
        buf[128..160].copy_from_slice(self.z_x.as_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ValidityProofError> {
        let bytes = array_ref![bytes, 0, 160];
        let (Y_0, Y_1, Y_2, z_r, z_x) = array_refs![bytes, 32, 32, 32, 32, 32];

        let Y_0 = CompressedRistretto::from_slice(Y_0);
        let Y_1 = CompressedRistretto::from_slice(Y_1);
        let Y_2 = CompressedRistretto::from_slice(Y_2);

        let z_r = Scalar::from_canonical_bytes(*z_r).ok_or(ValidityProofError::Format)?;
        let z_x = Scalar::from_canonical_bytes(*z_x).ok_or(ValidityProofError::Format)?;

        Ok(ValidityProof {
            Y_0,
            Y_1,
            Y_2,
            z_r,
            z_x,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::encryption::{elgamal::ElGamalKeypair, pedersen::Pedersen};

    #[test]
    fn test_validity_proof() {
        let elgamal_pubkey_dest = ElGamalKeypair::new_rand().public;
        let elgamal_pubkey_auditor = ElGamalKeypair::new_rand().public;

        let x_lo: u64 = 55;
        let x_hi: u64 = 77;

        let (commitment_lo, open_lo) = Pedersen::new(x_lo);
        let (commitment_hi, open_hi) = Pedersen::new(x_hi);

        let handle_lo_dest = elgamal_pubkey_dest.decrypt_handle(&open_lo);
        let handle_hi_dest = elgamal_pubkey_dest.decrypt_handle(&open_hi);

        let handle_lo_auditor = elgamal_pubkey_auditor.decrypt_handle(&open_lo);
        let handle_hi_auditor = elgamal_pubkey_auditor.decrypt_handle(&open_hi);

        let mut transcript_prover = Transcript::new(b"Test");
        let mut transcript_verifier = Transcript::new(b"Test");

        let proof = ValidityProof::new(
            &elgamal_pubkey_dest,
            &elgamal_pubkey_auditor,
            (x_lo, x_hi),
            (&open_lo, &open_hi),
            &mut transcript_prover,
        );

        assert!(proof
            .verify(
                &elgamal_pubkey_dest,
                &elgamal_pubkey_auditor,
                (&commitment_lo, &commitment_hi),
                (&handle_lo_dest, &handle_hi_dest),
                (&handle_lo_auditor, &handle_hi_auditor),
                &mut transcript_verifier,
            )
            .is_ok());

        // TODO: Test invalid cases

        // TODO: Test serialization, deserialization
    }
}
