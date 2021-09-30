#[cfg(not(target_arch = "bpf"))]
use rand::{rngs::OsRng, CryptoRng, RngCore};
use {
    crate::{
        encryption::elgamal::{ElGamalCT, ElGamalPK},
        errors::ProofError,
    },
    core::ops::{Add, Div, Mul, Sub},
    curve25519_dalek::{
        constants::{RISTRETTO_BASEPOINT_COMPRESSED, RISTRETTO_BASEPOINT_POINT},
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::MultiscalarMul,
    },
    serde::{Deserialize, Serialize},
    sha3::Sha3_512,
    std::convert::TryInto,
    subtle::{Choice, ConstantTimeEq},
    zeroize::Zeroize,
};

/// Curve basepoints for which Pedersen commitment is defined over.
///
/// These points should be fixed for the entire system.
/// TODO: Consider setting these points as constants?
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub struct PedersenBase {
    pub G: RistrettoPoint,
    pub H: RistrettoPoint,
}
/// Default PedersenBase. This is set arbitrarily for now, but it should be fixed
/// for the entire system.
///
/// `G` is a constant point in the curve25519_dalek library
/// `H` is the Sha3 hash of `G` interpretted as a RistrettoPoint
impl Default for PedersenBase {
    #[allow(non_snake_case)]
    fn default() -> PedersenBase {
        let G = RISTRETTO_BASEPOINT_POINT;
        let H =
            RistrettoPoint::hash_from_bytes::<Sha3_512>(RISTRETTO_BASEPOINT_COMPRESSED.as_bytes());

        PedersenBase { G, H }
    }
}

/// Handle for the Pedersen commitment scheme
pub struct Pedersen;
impl Pedersen {
    /// Given a number as input, the function returns a Pedersen commitment of
    /// the number and its corresponding opening.
    ///
    /// TODO: Interface that takes a random generator as input
    #[cfg(not(target_arch = "bpf"))]
    pub fn commit<T: Into<Scalar>>(amount: T) -> (PedersenComm, PedersenOpen) {
        let open = PedersenOpen(Scalar::random(&mut OsRng));
        let comm = Pedersen::commit_with(amount, &open);

        (comm, open)
    }

    /// Given a number and an opening as inputs, the function returns their
    /// Pedersen commitment.
    #[allow(non_snake_case)]
    pub fn commit_with<T: Into<Scalar>>(amount: T, open: &PedersenOpen) -> PedersenComm {
        let G = PedersenBase::default().G;
        let H = PedersenBase::default().H;

        let x: Scalar = amount.into();
        let r = open.get_scalar();

        PedersenComm(RistrettoPoint::multiscalar_mul(&[x, r], &[G, H]))
    }

    /// Given a number, opening, and Pedersen commitment, the function verifies
    /// the validity of the commitment with respect to the number and opening.
    ///
    /// This function is included for completeness and is not used for the
    /// c-token program.
    #[allow(non_snake_case)]
    pub fn verify<T: Into<Scalar>>(
        comm: PedersenComm,
        open: PedersenOpen,
        amount: T,
    ) -> Result<(), ProofError> {
        let G = PedersenBase::default().G;
        let H = PedersenBase::default().H;

        let x: Scalar = amount.into();

        let r = open.get_scalar();
        let C = comm.get_point();

        if C == RistrettoPoint::multiscalar_mul(&[x, r], &[G, H]) {
            Ok(())
        } else {
            Err(ProofError::VerificationError)
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Zeroize)]
#[zeroize(drop)]
pub struct PedersenOpen(pub(crate) Scalar);
impl PedersenOpen {
    pub fn get_scalar(&self) -> Scalar {
        self.0
    }

    #[cfg(not(target_arch = "bpf"))]
    pub fn random<T: RngCore + CryptoRng>(rng: &mut T) -> Self {
        PedersenOpen(Scalar::random(rng))
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<PedersenOpen> {
        match bytes.try_into() {
            Ok(bytes) => Scalar::from_canonical_bytes(bytes).map(PedersenOpen),
            _ => None,
        }
    }
}
impl Eq for PedersenOpen {}
impl PartialEq for PedersenOpen {
    fn eq(&self, other: &Self) -> bool {
        self.ct_eq(other).unwrap_u8() == 1u8
    }
}
impl ConstantTimeEq for PedersenOpen {
    fn ct_eq(&self, other: &Self) -> Choice {
        self.0.ct_eq(&other.0)
    }
}

impl Default for PedersenOpen {
    fn default() -> Self {
        PedersenOpen(Scalar::default())
    }
}

impl<'a, 'b> Add<&'b PedersenOpen> for &'a PedersenOpen {
    type Output = PedersenOpen;

    fn add(self, other: &'b PedersenOpen) -> PedersenOpen {
        PedersenOpen(self.get_scalar() + other.get_scalar())
    }
}

define_add_variants!(
    LHS = PedersenOpen,
    RHS = PedersenOpen,
    Output = PedersenOpen
);

impl<'a, 'b> Sub<&'b PedersenOpen> for &'a PedersenOpen {
    type Output = PedersenOpen;

    fn sub(self, other: &'b PedersenOpen) -> PedersenOpen {
        PedersenOpen(self.get_scalar() - other.get_scalar())
    }
}

define_sub_variants!(
    LHS = PedersenOpen,
    RHS = PedersenOpen,
    Output = PedersenOpen
);

impl<'a, 'b> Mul<&'b Scalar> for &'a PedersenOpen {
    type Output = PedersenOpen;

    fn mul(self, other: &'b Scalar) -> PedersenOpen {
        PedersenOpen(self.get_scalar() * other)
    }
}

define_mul_variants!(LHS = PedersenOpen, RHS = Scalar, Output = PedersenOpen);

impl<'a, 'b> Div<&'b Scalar> for &'a PedersenOpen {
    type Output = PedersenOpen;

    #[allow(clippy::suspicious_arithmetic_impl)]
    fn div(self, other: &'b Scalar) -> PedersenOpen {
        PedersenOpen(self.get_scalar() * other.invert())
    }
}

define_div_variants!(LHS = PedersenOpen, RHS = Scalar, Output = PedersenOpen);

#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, Eq, PartialEq)]
pub struct PedersenComm(pub(crate) RistrettoPoint);
impl PedersenComm {
    pub fn get_point(&self) -> RistrettoPoint {
        self.0
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.compress().to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<PedersenComm> {
        Some(PedersenComm(
            CompressedRistretto::from_slice(bytes).decompress()?,
        ))
    }
}

impl<'a, 'b> Add<&'b PedersenComm> for &'a PedersenComm {
    type Output = PedersenComm;

    fn add(self, other: &'b PedersenComm) -> PedersenComm {
        PedersenComm(self.get_point() + other.get_point())
    }
}

define_add_variants!(
    LHS = PedersenComm,
    RHS = PedersenComm,
    Output = PedersenComm
);

impl<'a, 'b> Sub<&'b PedersenComm> for &'a PedersenComm {
    type Output = PedersenComm;

    fn sub(self, other: &'b PedersenComm) -> PedersenComm {
        PedersenComm(self.get_point() - other.get_point())
    }
}

define_sub_variants!(
    LHS = PedersenComm,
    RHS = PedersenComm,
    Output = PedersenComm
);

impl<'a, 'b> Mul<&'b Scalar> for &'a PedersenComm {
    type Output = PedersenComm;

    fn mul(self, other: &'b Scalar) -> PedersenComm {
        PedersenComm(self.get_point() * other)
    }
}

define_mul_variants!(LHS = PedersenComm, RHS = Scalar, Output = PedersenComm);

impl<'a, 'b> Div<&'b Scalar> for &'a PedersenComm {
    type Output = PedersenComm;

    #[allow(clippy::suspicious_arithmetic_impl)]
    fn div(self, other: &'b Scalar) -> PedersenComm {
        PedersenComm(self.get_point() * other.invert())
    }
}

define_div_variants!(LHS = PedersenComm, RHS = Scalar, Output = PedersenComm);

/// Decryption handle for Pedersen commitment.
///
/// A decryption handle can be combined with Pedersen commitments to form an
/// ElGamal ciphertext.
#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, Eq, PartialEq)]
pub struct PedersenDecHandle(pub(crate) RistrettoPoint);
impl PedersenDecHandle {
    pub fn get_point(&self) -> RistrettoPoint {
        self.0
    }

    pub fn generate_handle(open: &PedersenOpen, pk: &ElGamalPK) -> PedersenDecHandle {
        PedersenDecHandle(open.get_scalar() * pk.get_point())
    }

    /// Maps a decryption token and Pedersen commitment to ElGamal ciphertext
    pub fn to_elgamal_ctxt(self, comm: PedersenComm) -> ElGamalCT {
        ElGamalCT {
            message_comm: comm,
            decrypt_handle: self,
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.compress().to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<PedersenDecHandle> {
        Some(PedersenDecHandle(
            CompressedRistretto::from_slice(bytes).decompress()?,
        ))
    }
}

impl<'a, 'b> Add<&'b PedersenDecHandle> for &'a PedersenDecHandle {
    type Output = PedersenDecHandle;

    fn add(self, other: &'b PedersenDecHandle) -> PedersenDecHandle {
        PedersenDecHandle(self.get_point() + other.get_point())
    }
}

define_add_variants!(
    LHS = PedersenDecHandle,
    RHS = PedersenDecHandle,
    Output = PedersenDecHandle
);

impl<'a, 'b> Sub<&'b PedersenDecHandle> for &'a PedersenDecHandle {
    type Output = PedersenDecHandle;

    fn sub(self, other: &'b PedersenDecHandle) -> PedersenDecHandle {
        PedersenDecHandle(self.get_point() - other.get_point())
    }
}

define_sub_variants!(
    LHS = PedersenDecHandle,
    RHS = PedersenDecHandle,
    Output = PedersenDecHandle
);

impl<'a, 'b> Mul<&'b Scalar> for &'a PedersenDecHandle {
    type Output = PedersenDecHandle;

    fn mul(self, other: &'b Scalar) -> PedersenDecHandle {
        PedersenDecHandle(self.get_point() * other)
    }
}

define_mul_variants!(
    LHS = PedersenDecHandle,
    RHS = Scalar,
    Output = PedersenDecHandle
);

impl<'a, 'b> Div<&'b Scalar> for &'a PedersenDecHandle {
    type Output = PedersenDecHandle;

    #[allow(clippy::suspicious_arithmetic_impl)]
    fn div(self, other: &'b Scalar) -> PedersenDecHandle {
        PedersenDecHandle(self.get_point() * other.invert())
    }
}

define_div_variants!(
    LHS = PedersenDecHandle,
    RHS = Scalar,
    Output = PedersenDecHandle
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_verification_correctness() {
        let amt: u64 = 57;
        let (comm, open) = Pedersen::commit(amt);

        assert!(Pedersen::verify(comm, open, amt).is_ok());
    }

    #[test]
    fn test_homomorphic_addition() {
        let amt_0: u64 = 77;
        let amt_1: u64 = 57;

        let rng = &mut OsRng;
        let open_0 = PedersenOpen(Scalar::random(rng));
        let open_1 = PedersenOpen(Scalar::random(rng));

        let comm_0 = Pedersen::commit_with(amt_0, &open_0);
        let comm_1 = Pedersen::commit_with(amt_1, &open_1);
        let comm_addition = Pedersen::commit_with(amt_0 + amt_1, &(open_0 + open_1));

        assert_eq!(comm_addition, comm_0 + comm_1);
    }

    #[test]
    fn test_homomorphic_subtraction() {
        let amt_0: u64 = 77;
        let amt_1: u64 = 57;

        let rng = &mut OsRng;
        let open_0 = PedersenOpen(Scalar::random(rng));
        let open_1 = PedersenOpen(Scalar::random(rng));

        let comm_0 = Pedersen::commit_with(amt_0, &open_0);
        let comm_1 = Pedersen::commit_with(amt_1, &open_1);
        let comm_addition = Pedersen::commit_with(amt_0 - amt_1, &(open_0 - open_1));

        assert_eq!(comm_addition, comm_0 - comm_1);
    }

    #[test]
    fn test_homomorphic_multiplication() {
        let amt_0: u64 = 77;
        let amt_1: u64 = 57;

        let (comm, open) = Pedersen::commit(amt_0);
        let scalar = Scalar::from(amt_1);
        let comm_addition = Pedersen::commit_with(amt_0 * amt_1, &(open * scalar));

        assert_eq!(comm_addition, comm * scalar);
    }

    #[test]
    fn test_homomorphic_division() {
        let amt_0: u64 = 77;
        let amt_1: u64 = 7;

        let (comm, open) = Pedersen::commit(amt_0);
        let scalar = Scalar::from(amt_1);
        let comm_addition = Pedersen::commit_with(amt_0 / amt_1, &(open / scalar));

        assert_eq!(comm_addition, comm / scalar);
    }

    #[test]
    fn test_commitment_bytes() {
        let amt: u64 = 77;
        let (comm, _) = Pedersen::commit(amt);

        let encoded = comm.to_bytes();
        let decoded = PedersenComm::from_bytes(&encoded).unwrap();

        assert_eq!(comm, decoded);
    }

    #[test]
    fn test_opening_bytes() {
        let open = PedersenOpen(Scalar::random(&mut OsRng));

        let encoded = open.to_bytes();
        let decoded = PedersenOpen::from_bytes(&encoded).unwrap();

        assert_eq!(open, decoded);
    }

    #[test]
    fn test_decrypt_handle_bytes() {
        let handle = PedersenDecHandle(RistrettoPoint::default());

        let encoded = handle.to_bytes();
        let decoded = PedersenDecHandle::from_bytes(&encoded).unwrap();

        assert_eq!(handle, decoded);
    }

    #[test]
    fn test_serde_commitment() {
        let amt: u64 = 77;
        let (comm, _) = Pedersen::commit(amt);

        let encoded = bincode::serialize(&comm).unwrap();
        let decoded: PedersenComm = bincode::deserialize(&encoded).unwrap();

        assert_eq!(comm, decoded);
    }

    #[test]
    fn test_serde_opening() {
        let open = PedersenOpen(Scalar::random(&mut OsRng));

        let encoded = bincode::serialize(&open).unwrap();
        let decoded: PedersenOpen = bincode::deserialize(&encoded).unwrap();

        assert_eq!(open, decoded);
    }

    #[test]
    fn test_serde_decrypt_handle() {
        let handle = PedersenDecHandle(RistrettoPoint::default());

        let encoded = bincode::serialize(&handle).unwrap();
        let decoded: PedersenDecHandle = bincode::deserialize(&encoded).unwrap();

        assert_eq!(handle, decoded);
    }
}
