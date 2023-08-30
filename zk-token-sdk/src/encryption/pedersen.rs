//! Pedersen commitment implementation using the Ristretto prime-order group.

#[cfg(not(target_os = "solana"))]
use rand::rngs::OsRng;
use {
    crate::{RISTRETTO_POINT_LEN, SCALAR_LEN},
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

/// Byte length of a Pedersen opening.
const PEDERSEN_OPENING_LEN: usize = SCALAR_LEN;

/// Byte length of a Pedersen commitment.
pub(crate) const PEDERSEN_COMMITMENT_LEN: usize = RISTRETTO_POINT_LEN;

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
    pub fn with<T: Into<Scalar>>(amount: T, opening: &PedersenOpening) -> PedersenCommitment {
        let x: Scalar = amount.into();
        let r = opening.get_scalar();

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
pub struct PedersenOpening(Scalar);
impl PedersenOpening {
    pub fn new(scalar: Scalar) -> Self {
        Self(scalar)
    }

    pub fn get_scalar(&self) -> &Scalar {
        &self.0
    }

    #[cfg(not(target_os = "solana"))]
    pub fn new_rand() -> Self {
        PedersenOpening(Scalar::random(&mut OsRng))
    }

    pub fn as_bytes(&self) -> &[u8; PEDERSEN_OPENING_LEN] {
        self.0.as_bytes()
    }

    pub fn to_bytes(&self) -> [u8; PEDERSEN_OPENING_LEN] {
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
pub struct PedersenCommitment(RistrettoPoint);
impl PedersenCommitment {
    pub fn new(point: RistrettoPoint) -> Self {
        Self(point)
    }

    pub fn get_point(&self) -> &RistrettoPoint {
        &self.0
    }

    pub fn to_bytes(&self) -> [u8; PEDERSEN_COMMITMENT_LEN] {
        self.0.compress().to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<PedersenCommitment> {
        if bytes.len() != PEDERSEN_COMMITMENT_LEN {
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
        let amount_0: u64 = 77;
        let amount_1: u64 = 57;

        let rng = &mut OsRng;
        let opening_0 = PedersenOpening(Scalar::random(rng));
        let opening_1 = PedersenOpening(Scalar::random(rng));

        let commitment_0 = Pedersen::with(amount_0, &opening_0);
        let commitment_1 = Pedersen::with(amount_1, &opening_1);
        let commitment_addition = Pedersen::with(amount_0 + amount_1, &(opening_0 + opening_1));

        assert_eq!(commitment_addition, commitment_0 + commitment_1);
    }

    #[test]
    fn test_pedersen_homomorphic_subtraction() {
        let amount_0: u64 = 77;
        let amount_1: u64 = 57;

        let rng = &mut OsRng;
        let opening_0 = PedersenOpening(Scalar::random(rng));
        let opening_1 = PedersenOpening(Scalar::random(rng));

        let commitment_0 = Pedersen::with(amount_0, &opening_0);
        let commitment_1 = Pedersen::with(amount_1, &opening_1);
        let commitment_addition = Pedersen::with(amount_0 - amount_1, &(opening_0 - opening_1));

        assert_eq!(commitment_addition, commitment_0 - commitment_1);
    }

    #[test]
    fn test_pedersen_homomorphic_multiplication() {
        let amount_0: u64 = 77;
        let amount_1: u64 = 57;

        let (commitment, opening) = Pedersen::new(amount_0);
        let scalar = Scalar::from(amount_1);
        let commitment_addition = Pedersen::with(amount_0 * amount_1, &(opening * scalar));

        assert_eq!(commitment_addition, commitment * scalar);
        assert_eq!(commitment_addition, scalar * commitment);
    }

    #[test]
    fn test_pedersen_commitment_bytes() {
        let amount: u64 = 77;
        let (commitment, _) = Pedersen::new(amount);

        let encoded = commitment.to_bytes();
        let decoded = PedersenCommitment::from_bytes(&encoded).unwrap();

        assert_eq!(commitment, decoded);

        // incorrect length encoding
        assert_eq!(PedersenCommitment::from_bytes(&[0; 33]), None);
    }

    #[test]
    fn test_pedersen_opening_bytes() {
        let opening = PedersenOpening(Scalar::random(&mut OsRng));

        let encoded = opening.to_bytes();
        let decoded = PedersenOpening::from_bytes(&encoded).unwrap();

        assert_eq!(opening, decoded);

        // incorrect length encoding
        assert_eq!(PedersenOpening::from_bytes(&[0; 33]), None);
    }

    #[test]
    fn test_serde_pedersen_commitment() {
        let amount: u64 = 77;
        let (commitment, _) = Pedersen::new(amount);

        let encoded = bincode::serialize(&commitment).unwrap();
        let decoded: PedersenCommitment = bincode::deserialize(&encoded).unwrap();

        assert_eq!(commitment, decoded);
    }

    #[test]
    fn test_serde_pedersen_opening() {
        let opening = PedersenOpening(Scalar::random(&mut OsRng));

        let encoded = bincode::serialize(&opening).unwrap();
        let decoded: PedersenOpening = bincode::deserialize(&encoded).unwrap();

        assert_eq!(opening, decoded);
    }
}
