//! Plain Old Data wrappers for types that need to be sent over the wire

use bytemuck::{Pod, Zeroable};
#[cfg(not(target_arch = "bpf"))]
use {
    crate::{
        encryption::elgamal::{ElGamalCT, ElGamalPK},
        encryption::pedersen::{PedersenComm, PedersenDecHandle},
        errors::ProofError,
        range_proof::RangeProof,
    },
    curve25519_dalek::{
        constants::RISTRETTO_BASEPOINT_COMPRESSED, ristretto::CompressedRistretto, scalar::Scalar,
    },
    std::{
        convert::{TryFrom, TryInto},
        fmt,
    },
};

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct PodScalar([u8; 32]);

#[cfg(not(target_arch = "bpf"))]
impl From<Scalar> for PodScalar {
    fn from(scalar: Scalar) -> Self {
        Self(scalar.to_bytes())
    }
}

#[cfg(not(target_arch = "bpf"))]
impl From<PodScalar> for Scalar {
    fn from(pod: PodScalar) -> Self {
        Scalar::from_bits(pod.0)
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodCompressedRistretto([u8; 32]);

#[cfg(not(target_arch = "bpf"))]
impl From<CompressedRistretto> for PodCompressedRistretto {
    fn from(cr: CompressedRistretto) -> Self {
        Self(cr.to_bytes())
    }
}

#[cfg(not(target_arch = "bpf"))]
impl From<PodCompressedRistretto> for CompressedRistretto {
    fn from(pod: PodCompressedRistretto) -> Self {
        Self(pod.0)
    }
}

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct PodElGamalCT([u8; 64]);

#[cfg(not(target_arch = "bpf"))]
impl From<ElGamalCT> for PodElGamalCT {
    fn from(ct: ElGamalCT) -> Self {
        Self(ct.to_bytes())
    }
}

#[cfg(not(target_arch = "bpf"))]
impl TryFrom<PodElGamalCT> for ElGamalCT {
    type Error = ProofError;

    fn try_from(pod: PodElGamalCT) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0).ok_or(ProofError::InconsistentCTData)
    }
}

impl From<(PodPedersenComm, PodPedersenDecHandle)> for PodElGamalCT {
    fn from((pod_comm, pod_decrypt_handle): (PodPedersenComm, PodPedersenDecHandle)) -> Self {
        let mut buf = [0_u8; 64];
        buf[..32].copy_from_slice(&pod_comm.0);
        buf[32..].copy_from_slice(&pod_decrypt_handle.0);
        PodElGamalCT(buf)
    }
}

#[cfg(not(target_arch = "bpf"))]
impl fmt::Debug for PodElGamalCT {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct PodElGamalPK([u8; 32]);

#[cfg(not(target_arch = "bpf"))]
impl From<ElGamalPK> for PodElGamalPK {
    fn from(pk: ElGamalPK) -> Self {
        Self(pk.to_bytes())
    }
}

#[cfg(not(target_arch = "bpf"))]
impl TryFrom<PodElGamalPK> for ElGamalPK {
    type Error = ProofError;

    fn try_from(pod: PodElGamalPK) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0).ok_or(ProofError::InconsistentCTData)
    }
}

#[cfg(not(target_arch = "bpf"))]
impl fmt::Debug for PodElGamalPK {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct PodPedersenComm([u8; 32]);

#[cfg(not(target_arch = "bpf"))]
impl From<PedersenComm> for PodPedersenComm {
    fn from(comm: PedersenComm) -> Self {
        Self(comm.to_bytes())
    }
}

// For proof verification, interpret PodPedersenComm directly as CompressedRistretto
#[cfg(not(target_arch = "bpf"))]
impl From<PodPedersenComm> for CompressedRistretto {
    fn from(pod: PodPedersenComm) -> Self {
        Self(pod.0)
    }
}

#[cfg(not(target_arch = "bpf"))]
impl TryFrom<PodPedersenComm> for PedersenComm {
    type Error = ProofError;

    fn try_from(pod: PodPedersenComm) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0).ok_or(ProofError::InconsistentCTData)
    }
}

#[cfg(not(target_arch = "bpf"))]
impl fmt::Debug for PodPedersenComm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
#[repr(transparent)]
pub struct PodPedersenDecHandle([u8; 32]);

#[cfg(not(target_arch = "bpf"))]
impl From<PedersenDecHandle> for PodPedersenDecHandle {
    fn from(handle: PedersenDecHandle) -> Self {
        Self(handle.to_bytes())
    }
}

// For proof verification, interpret PodPedersenDecHandle as CompressedRistretto
#[cfg(not(target_arch = "bpf"))]
impl From<PodPedersenDecHandle> for CompressedRistretto {
    fn from(pod: PodPedersenDecHandle) -> Self {
        Self(pod.0)
    }
}

#[cfg(not(target_arch = "bpf"))]
impl TryFrom<PodPedersenDecHandle> for PedersenDecHandle {
    type Error = ProofError;

    fn try_from(pod: PodPedersenDecHandle) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0).ok_or(ProofError::InconsistentCTData)
    }
}

#[cfg(not(target_arch = "bpf"))]
impl fmt::Debug for PodPedersenDecHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// Serialization of range proofs for 64-bit numbers (for `Withdraw` instruction)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodRangeProof64([u8; 672]);

// `PodRangeProof64` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for PodRangeProof64 {}
unsafe impl Pod for PodRangeProof64 {}

#[cfg(not(target_arch = "bpf"))]
impl TryFrom<RangeProof> for PodRangeProof64 {
    type Error = ProofError;

    fn try_from(proof: RangeProof) -> Result<Self, Self::Error> {
        if proof.ipp_proof.serialized_size() != 448 {
            return Err(ProofError::VerificationError);
        }

        let mut buf = [0_u8; 672];
        buf[..32].copy_from_slice(proof.A.as_bytes());
        buf[32..64].copy_from_slice(proof.S.as_bytes());
        buf[64..96].copy_from_slice(proof.T_1.as_bytes());
        buf[96..128].copy_from_slice(proof.T_2.as_bytes());
        buf[128..160].copy_from_slice(proof.t_x.as_bytes());
        buf[160..192].copy_from_slice(proof.t_x_blinding.as_bytes());
        buf[192..224].copy_from_slice(proof.e_blinding.as_bytes());
        buf[224..672].copy_from_slice(&proof.ipp_proof.to_bytes());
        Ok(PodRangeProof64(buf))
    }
}

#[cfg(not(target_arch = "bpf"))]
impl TryFrom<PodRangeProof64> for RangeProof {
    type Error = ProofError;

    fn try_from(pod: PodRangeProof64) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

/// Serialization of range proofs for 128-bit numbers (for `TransferRangeProof` instruction)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PodRangeProof128([u8; 736]);

// `PodRangeProof128` is a Pod and Zeroable.
// Add the marker traits manually because `bytemuck` only adds them for some `u8` arrays
unsafe impl Zeroable for PodRangeProof128 {}
unsafe impl Pod for PodRangeProof128 {}

#[cfg(not(target_arch = "bpf"))]
impl TryFrom<RangeProof> for PodRangeProof128 {
    type Error = ProofError;

    fn try_from(proof: RangeProof) -> Result<Self, Self::Error> {
        if proof.ipp_proof.serialized_size() != 512 {
            return Err(ProofError::VerificationError);
        }

        let mut buf = [0_u8; 736];
        buf[..32].copy_from_slice(proof.A.as_bytes());
        buf[32..64].copy_from_slice(proof.S.as_bytes());
        buf[64..96].copy_from_slice(proof.T_1.as_bytes());
        buf[96..128].copy_from_slice(proof.T_2.as_bytes());
        buf[128..160].copy_from_slice(proof.t_x.as_bytes());
        buf[160..192].copy_from_slice(proof.t_x_blinding.as_bytes());
        buf[192..224].copy_from_slice(proof.e_blinding.as_bytes());
        buf[224..736].copy_from_slice(&proof.ipp_proof.to_bytes());
        Ok(PodRangeProof128(buf))
    }
}

#[cfg(not(target_arch = "bpf"))]
impl TryFrom<PodRangeProof128> for RangeProof {
    type Error = ProofError;

    fn try_from(pod: PodRangeProof128) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod.0)
    }
}

pub struct PodElGamalArithmetic;

#[cfg(not(target_arch = "bpf"))]
impl PodElGamalArithmetic {
    const TWO_32: u64 = 4294967296;

    // On input two scalars x0, x1 and two ciphertexts ct0, ct1,
    // returns `Some(x0*ct0 + x1*ct1)` or `None` if the input was invalid
    fn add_pod_ciphertexts(
        scalar_0: Scalar,
        pod_ct_0: PodElGamalCT,
        scalar_1: Scalar,
        pod_ct_1: PodElGamalCT,
    ) -> Option<PodElGamalCT> {
        let ct_0: ElGamalCT = pod_ct_0.try_into().ok()?;
        let ct_1: ElGamalCT = pod_ct_1.try_into().ok()?;

        let ct_sum = ct_0 * scalar_0 + ct_1 * scalar_1;
        Some(PodElGamalCT::from(ct_sum))
    }

    fn combine_lo_hi(pod_ct_lo: PodElGamalCT, pod_ct_hi: PodElGamalCT) -> Option<PodElGamalCT> {
        Self::add_pod_ciphertexts(
            Scalar::one(),
            pod_ct_lo,
            Scalar::from(Self::TWO_32),
            pod_ct_hi,
        )
    }

    pub fn add(pod_ct_0: PodElGamalCT, pod_ct_1: PodElGamalCT) -> Option<PodElGamalCT> {
        Self::add_pod_ciphertexts(Scalar::one(), pod_ct_0, Scalar::one(), pod_ct_1)
    }

    pub fn add_with_lo_hi(
        pod_ct_0: PodElGamalCT,
        pod_ct_1_lo: PodElGamalCT,
        pod_ct_1_hi: PodElGamalCT,
    ) -> Option<PodElGamalCT> {
        let pod_ct_1 = Self::combine_lo_hi(pod_ct_1_lo, pod_ct_1_hi)?;
        Self::add_pod_ciphertexts(Scalar::one(), pod_ct_0, Scalar::one(), pod_ct_1)
    }

    pub fn subtract(pod_ct_0: PodElGamalCT, pod_ct_1: PodElGamalCT) -> Option<PodElGamalCT> {
        Self::add_pod_ciphertexts(Scalar::one(), pod_ct_0, -Scalar::one(), pod_ct_1)
    }

    pub fn subtract_with_lo_hi(
        pod_ct_0: PodElGamalCT,
        pod_ct_1_lo: PodElGamalCT,
        pod_ct_1_hi: PodElGamalCT,
    ) -> Option<PodElGamalCT> {
        let pod_ct_1 = Self::combine_lo_hi(pod_ct_1_lo, pod_ct_1_hi)?;
        Self::add_pod_ciphertexts(Scalar::one(), pod_ct_0, -Scalar::one(), pod_ct_1)
    }

    pub fn add_to(pod_ct: PodElGamalCT, amount: u64) -> Option<PodElGamalCT> {
        let mut amount_as_pod_ct = [0_u8; 64];
        amount_as_pod_ct[..32].copy_from_slice(RISTRETTO_BASEPOINT_COMPRESSED.as_bytes());
        Self::add_pod_ciphertexts(
            Scalar::one(),
            pod_ct,
            Scalar::from(amount),
            PodElGamalCT(amount_as_pod_ct),
        )
    }

    pub fn subtract_from(pod_ct: PodElGamalCT, amount: u64) -> Option<PodElGamalCT> {
        let mut amount_as_pod_ct = [0_u8; 64];
        amount_as_pod_ct[..32].copy_from_slice(RISTRETTO_BASEPOINT_COMPRESSED.as_bytes());
        Self::add_pod_ciphertexts(
            Scalar::one(),
            pod_ct,
            -Scalar::from(amount),
            PodElGamalCT(amount_as_pod_ct),
        )
    }
}
#[cfg(target_arch = "bpf")]
#[allow(unused_variables)]
impl PodElGamalArithmetic {
    pub fn add(pod_ct_0: PodElGamalCT, pod_ct_1: PodElGamalCT) -> Option<PodElGamalCT> {
        None
    }

    pub fn add_with_lo_hi(
        pod_ct_0: PodElGamalCT,
        pod_ct_1_lo: PodElGamalCT,
        pod_ct_1_hi: PodElGamalCT,
    ) -> Option<PodElGamalCT> {
        None
    }

    pub fn subtract(pod_ct_0: PodElGamalCT, pod_ct_1: PodElGamalCT) -> Option<PodElGamalCT> {
        None
    }

    pub fn subtract_with_lo_hi(
        pod_ct_0: PodElGamalCT,
        pod_ct_1_lo: PodElGamalCT,
        pod_ct_1_hi: PodElGamalCT,
    ) -> Option<PodElGamalCT> {
        None
    }

    pub fn add_to(pod_ct: PodElGamalCT, amount: u64) -> Option<PodElGamalCT> {
        None
    }

    pub fn subtract_from(pod_ct: PodElGamalCT, amount: u64) -> Option<PodElGamalCT> {
        None
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::encryption::{
            elgamal::{ElGamal, ElGamalCT},
            pedersen::{Pedersen, PedersenOpen},
        },
        crate::instruction::transfer::split_u64_into_u32,
        merlin::Transcript,
        rand::rngs::OsRng,
        std::convert::TryInto,
    };

    #[test]
    fn test_zero_ct() {
        let spendable_balance = PodElGamalCT::zeroed();
        let spendable_ct: ElGamalCT = spendable_balance.try_into().unwrap();

        // spendable_ct should be an encryption of 0 for any public key when
        // `PedersenOpen::default()` is used
        let (pk, _) = ElGamal::keygen();
        let balance: u64 = 0;
        assert_eq!(
            spendable_ct,
            pk.encrypt_with(balance, &PedersenOpen::default())
        );

        // homomorphism should work like any other ciphertext
        let open = PedersenOpen::random(&mut OsRng);
        let transfer_amount_ct = pk.encrypt_with(55_u64, &open);
        let transfer_amount_pod: PodElGamalCT = transfer_amount_ct.into();

        let sum = PodElGamalArithmetic::add(spendable_balance, transfer_amount_pod).unwrap();

        let expected: PodElGamalCT = pk.encrypt_with(55_u64, &open).into();
        assert_eq!(expected, sum);
    }

    #[test]
    fn test_add_to() {
        let spendable_balance = PodElGamalCT::zeroed();

        let added_ct = PodElGamalArithmetic::add_to(spendable_balance, 55).unwrap();

        let (pk, _) = ElGamal::keygen();
        let expected: PodElGamalCT = pk.encrypt_with(55_u64, &PedersenOpen::default()).into();

        assert_eq!(expected, added_ct);
    }

    #[test]
    fn test_subtract_from() {
        let amount = 77_u64;
        let (pk, _) = ElGamal::keygen();
        let open = PedersenOpen::random(&mut OsRng);
        let encrypted_amount: PodElGamalCT = pk.encrypt_with(amount, &open).into();

        let subtracted_ct = PodElGamalArithmetic::subtract_from(encrypted_amount, 55).unwrap();

        let expected: PodElGamalCT = pk.encrypt_with(22_u64, &open).into();

        assert_eq!(expected, subtracted_ct);
    }

    #[test]
    fn test_pod_range_proof_64() {
        let (comm, open) = Pedersen::commit(55_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::create(vec![55], vec![64], vec![&open], &mut transcript_create);

        let proof_serialized: PodRangeProof64 = proof.try_into().unwrap();
        let proof_deserialized: RangeProof = proof_serialized.try_into().unwrap();

        assert!(proof_deserialized
            .verify(
                vec![&comm.get_point().compress()],
                vec![64],
                &mut transcript_verify
            )
            .is_ok());

        // should fail to serialize to PodRangeProof128
        let proof = RangeProof::create(vec![55], vec![64], vec![&open], &mut transcript_create);

        assert!(TryInto::<PodRangeProof128>::try_into(proof).is_err());
    }

    #[test]
    fn test_pod_range_proof_128() {
        let (comm_1, open_1) = Pedersen::commit(55_u64);
        let (comm_2, open_2) = Pedersen::commit(77_u64);
        let (comm_3, open_3) = Pedersen::commit(99_u64);

        let mut transcript_create = Transcript::new(b"Test");
        let mut transcript_verify = Transcript::new(b"Test");

        let proof = RangeProof::create(
            vec![55, 77, 99],
            vec![64, 32, 32],
            vec![&open_1, &open_2, &open_3],
            &mut transcript_create,
        );

        let comm_1_point = comm_1.get_point().compress();
        let comm_2_point = comm_2.get_point().compress();
        let comm_3_point = comm_3.get_point().compress();

        let proof_serialized: PodRangeProof128 = proof.try_into().unwrap();
        let proof_deserialized: RangeProof = proof_serialized.try_into().unwrap();

        assert!(proof_deserialized
            .verify(
                vec![&comm_1_point, &comm_2_point, &comm_3_point],
                vec![64, 32, 32],
                &mut transcript_verify,
            )
            .is_ok());

        // should fail to serialize to PodRangeProof64
        let proof = RangeProof::create(
            vec![55, 77, 99],
            vec![64, 32, 32],
            vec![&open_1, &open_2, &open_3],
            &mut transcript_create,
        );

        assert!(TryInto::<PodRangeProof64>::try_into(proof).is_err());
    }

    #[test]
    fn test_transfer_arithmetic() {
        // setup

        // transfer amount
        let transfer_amount: u64 = 55;
        let (amount_lo, amount_hi) = split_u64_into_u32(transfer_amount);

        // generate public keys
        let (source_pk, _) = ElGamal::keygen();
        let (dest_pk, _) = ElGamal::keygen();
        let (auditor_pk, _) = ElGamal::keygen();

        // commitments associated with TransferRangeProof
        let (comm_lo, open_lo) = Pedersen::commit(amount_lo);
        let (comm_hi, open_hi) = Pedersen::commit(amount_hi);

        let comm_lo: PodPedersenComm = comm_lo.into();
        let comm_hi: PodPedersenComm = comm_hi.into();

        // decryption handles associated with TransferValidityProof
        let handle_source_lo: PodPedersenDecHandle = source_pk.gen_decrypt_handle(&open_lo).into();
        let handle_dest_lo: PodPedersenDecHandle = dest_pk.gen_decrypt_handle(&open_lo).into();
        let _handle_auditor_lo: PodPedersenDecHandle =
            auditor_pk.gen_decrypt_handle(&open_lo).into();

        let handle_source_hi: PodPedersenDecHandle = source_pk.gen_decrypt_handle(&open_hi).into();
        let handle_dest_hi: PodPedersenDecHandle = dest_pk.gen_decrypt_handle(&open_hi).into();
        let _handle_auditor_hi: PodPedersenDecHandle =
            auditor_pk.gen_decrypt_handle(&open_hi).into();

        // source spendable and recipient pending
        let source_open = PedersenOpen::random(&mut OsRng);
        let dest_open = PedersenOpen::random(&mut OsRng);

        let source_spendable_ct: PodElGamalCT = source_pk.encrypt_with(77_u64, &source_open).into();
        let dest_pending_ct: PodElGamalCT = dest_pk.encrypt_with(77_u64, &dest_open).into();

        // program arithmetic for the source account

        // 1. Combine commitments and handles
        let source_lo_ct: PodElGamalCT = (comm_lo, handle_source_lo).into();
        let source_hi_ct: PodElGamalCT = (comm_hi, handle_source_hi).into();

        // 2. Combine lo and hi ciphertexts
        let source_combined_ct =
            PodElGamalArithmetic::combine_lo_hi(source_lo_ct, source_hi_ct).unwrap();

        // 3. Subtract from available balance
        let final_source_spendable =
            PodElGamalArithmetic::subtract(source_spendable_ct, source_combined_ct).unwrap();

        // test
        let final_source_open = source_open
            - (open_lo.clone() + open_hi.clone() * Scalar::from(PodElGamalArithmetic::TWO_32));
        let expected_source: PodElGamalCT =
            source_pk.encrypt_with(22_u64, &final_source_open).into();
        assert_eq!(expected_source, final_source_spendable);

        // same for the destination account

        // 1. Combine commitments and handles
        let dest_lo_ct: PodElGamalCT = (comm_lo, handle_dest_lo).into();
        let dest_hi_ct: PodElGamalCT = (comm_hi, handle_dest_hi).into();

        // 2. Combine lo and hi ciphertexts
        let dest_combined_ct = PodElGamalArithmetic::combine_lo_hi(dest_lo_ct, dest_hi_ct).unwrap();

        // 3. Add to pending balance
        let final_dest_pending =
            PodElGamalArithmetic::add(dest_pending_ct, dest_combined_ct).unwrap();

        let final_dest_open =
            dest_open + (open_lo + open_hi * Scalar::from(PodElGamalArithmetic::TWO_32));
        let expected_dest_ct: PodElGamalCT = dest_pk.encrypt_with(132_u64, &final_dest_open).into();
        assert_eq!(expected_dest_ct, final_dest_pending);
    }
}
