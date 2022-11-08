//! Pedersen commitment implementation using the Ristretto prime-order group.

#[cfg(not(target_os = "solana"))]
use rand::rngs::OsRng;
use {
    core::ops::{Add, Mul, Sub},
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

lazy_static::lazy_static! {
    /// Pedersen base point for encoding messages to be committed.
    pub static ref G: RistrettoPoint = RISTRETTO_BASEPOINT_POINT;
    /// Pedersen base point for encoding the commitment openings.
    pub static ref H: RistrettoPoint =
        RistrettoPoint::hash_from_bytes::<Sha3_512>(RISTRETTO_BASEPOINT_COMPRESSED.as_bytes());
}

/// Algorithm handle for the Pedersen commitment scheme.
pub struct Pedersen;
impl Pedersen {
    /// On input a message (numeric amount), the function returns a Pedersen commitment of the
    /// message and the corresponding opening.
    ///
    /// This function is randomized. It internally samples a Pedersen opening using `OsRng`.
    #[cfg(not(target_os = "solana"))]
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T: Into<Scalar>>(amount: T) -> (PedersenCommitment, PedersenOpening) {
        let opening = PedersenOpening::new_rand();
        let commitment = Pedersen::with(amount, &opening);

        (commitment, opening)
    }

    /// On input a message (numeric amount) and a Pedersen opening, the function returns the
    /// corresponding Pedersen commitment.
    ///
    /// This function is deterministic.
    #[allow(non_snake_case)]
    pub fn with<T: Into<Scalar>>(amount: T, open: &PedersenOpening) -> PedersenCommitment {
        let x: Scalar = amount.into();
        let r = open.get_scalar();

        PedersenCommitment(RistrettoPoint::multiscalar_mul(&[x, *r], &[*G, *H]))
    }

    /// On input a message (numeric amount), the function returns a Pedersen commitment with zero
    /// as the opening.
    ///
    /// This function is deterministic.
    pub fn encode<T: Into<Scalar>>(amount: T) -> PedersenCommitment {
        PedersenCommitment(amount.into() * &(*G))
    }
}

/// Pedersen opening type.
///
/// Instances of Pedersen openings are zeroized on drop.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct PedersenOpening(pub(crate) Scalar);
impl PedersenOpening {
    pub fn get_scalar(&self) -> &Scalar {
        &self.0
    }

    #[cfg(not(target_os = "solana"))]
    pub fn new_rand() -> Self {
        PedersenOpening(Scalar::random(&mut OsRng))
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<PedersenOpening> {
        match bytes.try_into() {
            Ok(bytes) => Scalar::from_canonical_bytes(bytes).map(PedersenOpening),
            _ => None,
        }
    }
}
impl Eq for PedersenOpening {}
impl PartialEq for PedersenOpening {
    fn eq(&self, other: &Self) -> bool {
        self.ct_eq(other).unwrap_u8() == 1u8
    }
}
impl ConstantTimeEq for PedersenOpening {
    fn ct_eq(&self, other: &Self) -> Choice {
        self.0.ct_eq(&other.0)
    }
}

impl<'a, 'b> Add<&'b PedersenOpening> for &'a PedersenOpening {
    type Output = PedersenOpening;

    fn add(self, opening: &'b PedersenOpening) -> PedersenOpening {
        PedersenOpening(&self.0 + &opening.0)
    }
}

define_add_variants!(
    LHS = PedersenOpening,
    RHS = PedersenOpening,
    Output = PedersenOpening
);

impl<'a, 'b> Sub<&'b PedersenOpening> for &'a PedersenOpening {
    type Output = PedersenOpening;

    fn sub(self, opening: &'b PedersenOpening) -> PedersenOpening {
        PedersenOpening(&self.0 - &opening.0)
    }
}

define_sub_variants!(
    LHS = PedersenOpening,
    RHS = PedersenOpening,
    Output = PedersenOpening
);

impl<'a, 'b> Mul<&'b Scalar> for &'a PedersenOpening {
    type Output = PedersenOpening;

    fn mul(self, scalar: &'b Scalar) -> PedersenOpening {
        PedersenOpening(&self.0 * scalar)
    }
}

define_mul_variants!(
    LHS = PedersenOpening,
    RHS = Scalar,
    Output = PedersenOpening
);

impl<'a, 'b> Mul<&'b PedersenOpening> for &'a Scalar {
    type Output = PedersenOpening;

    fn mul(self, opening: &'b PedersenOpening) -> PedersenOpening {
        PedersenOpening(self * &opening.0)
    }
}

define_mul_variants!(
    LHS = Scalar,
    RHS = PedersenOpening,
    Output = PedersenOpening
);

/// Pedersen commitment type.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PedersenCommitment(pub(crate) RistrettoPoint);
impl PedersenCommitment {
    pub fn get_point(&self) -> &RistrettoPoint {
        &self.0
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.compress().to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<PedersenCommitment> {
        if bytes.len() != 32 {
            return None;
        }

        Some(PedersenCommitment(
            CompressedRistretto::from_slice(bytes).decompress()?,
        ))
    }
}

impl<'a, 'b> Add<&'b PedersenCommitment> for &'a PedersenCommitment {
    type Output = PedersenCommitment;

    fn add(self, commitment: &'b PedersenCommitment) -> PedersenCommitment {
        PedersenCommitment(&self.0 + &commitment.0)
    }
}

define_add_variants!(
    LHS = PedersenCommitment,
    RHS = PedersenCommitment,
    Output = PedersenCommitment
);

impl<'a, 'b> Sub<&'b PedersenCommitment> for &'a PedersenCommitment {
    type Output = PedersenCommitment;

    fn sub(self, commitment: &'b PedersenCommitment) -> PedersenCommitment {
        PedersenCommitment(&self.0 - &commitment.0)
    }
}

define_sub_variants!(
    LHS = PedersenCommitment,
    RHS = PedersenCommitment,
    Output = PedersenCommitment
);

impl<'a, 'b> Mul<&'b Scalar> for &'a PedersenCommitment {
    type Output = PedersenCommitment;

    fn mul(self, scalar: &'b Scalar) -> PedersenCommitment {
        PedersenCommitment(scalar * &self.0)
    }
}

define_mul_variants!(
    LHS = PedersenCommitment,
    RHS = Scalar,
    Output = PedersenCommitment
);

impl<'a, 'b> Mul<&'b PedersenCommitment> for &'a Scalar {
    type Output = PedersenCommitment;

    fn mul(self, commitment: &'b PedersenCommitment) -> PedersenCommitment {
        PedersenCommitment(self * &commitment.0)
    }
}

define_mul_variants!(
    LHS = Scalar,
    RHS = PedersenCommitment,
    Output = PedersenCommitment
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pedersen_homomorphic_addition() {
        let amt_0: u64 = 77;
        let amt_1: u64 = 57;

        let rng = &mut OsRng;
        let open_0 = PedersenOpening(Scalar::random(rng));
        let open_1 = PedersenOpening(Scalar::random(rng));

        let comm_0 = Pedersen::with(amt_0, &open_0);
        let comm_1 = Pedersen::with(amt_1, &open_1);
        let comm_addition = Pedersen::with(amt_0 + amt_1, &(open_0 + open_1));

        assert_eq!(comm_addition, comm_0 + comm_1);
    }

    #[test]
    fn test_pedersen_homomorphic_subtraction() {
        let amt_0: u64 = 77;
        let amt_1: u64 = 57;

        let rng = &mut OsRng;
        let open_0 = PedersenOpening(Scalar::random(rng));
        let open_1 = PedersenOpening(Scalar::random(rng));

        let comm_0 = Pedersen::with(amt_0, &open_0);
        let comm_1 = Pedersen::with(amt_1, &open_1);
        let comm_addition = Pedersen::with(amt_0 - amt_1, &(open_0 - open_1));

        assert_eq!(comm_addition, comm_0 - comm_1);
    }

    #[test]
    fn test_pedersen_homomorphic_multiplication() {
        let amt_0: u64 = 77;
        let amt_1: u64 = 57;

        let (comm, open) = Pedersen::new(amt_0);
        let scalar = Scalar::from(amt_1);
        let comm_addition = Pedersen::with(amt_0 * amt_1, &(open * scalar));

        assert_eq!(comm_addition, comm * scalar);
        assert_eq!(comm_addition, scalar * comm);
    }

    #[test]
    fn test_pedersen_commitment_bytes() {
        let amt: u64 = 77;
        let (comm, _) = Pedersen::new(amt);

        let encoded = comm.to_bytes();
        let decoded = PedersenCommitment::from_bytes(&encoded).unwrap();

        assert_eq!(comm, decoded);

        // incorrect length encoding
        assert_eq!(PedersenCommitment::from_bytes(&[0; 33]), None);
    }

    #[test]
    fn test_pedersen_opening_bytes() {
        let open = PedersenOpening(Scalar::random(&mut OsRng));

        let encoded = open.to_bytes();
        let decoded = PedersenOpening::from_bytes(&encoded).unwrap();

        assert_eq!(open, decoded);

        // incorrect length encoding
        assert_eq!(PedersenOpening::from_bytes(&[0; 33]), None);
    }

    #[test]
    fn test_serde_pedersen_commitment() {
        let amt: u64 = 77;
        let (comm, _) = Pedersen::new(amt);

        let encoded = bincode::serialize(&comm).unwrap();
        let decoded: PedersenCommitment = bincode::deserialize(&encoded).unwrap();

        assert_eq!(comm, decoded);
    }

    #[test]
    fn test_serde_pedersen_opening() {
        let open = PedersenOpening(Scalar::random(&mut OsRng));

        let encoded = bincode::serialize(&open).unwrap();
        let decoded: PedersenOpening = bincode::deserialize(&encoded).unwrap();

        assert_eq!(open, decoded);
    }
}
