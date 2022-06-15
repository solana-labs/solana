//! The traits representing the basic elliptic curve operations.
//!
//! These traits are instantiatable by all the commonly used elliptic curves and should help in
//! organizing syscall support for other curves in the future. more complicated or curve-specific
//! functions that are needed in cryptographic applications should be representable by combining
//! the associated functions of these traits.
//!
//! NOTE: This module temporarily lives in zk_token_sdk/curve25519, but it is independent of
//! zk-token-sdk or curve25519. It should be moved to a more general location in the future.
//!

// Functions are organized by the curve traits, which can be instantiated by multiple curve
// representations. The functions take in a `curve_id` (e.g. `CURVE25519_EDWARDS`) and should run
// the associated functions in the appropriate trait instantiation. The `curve_op` function
// additionally takes in an `op_id` (e.g. `ADD`) that controls which associated functions to run in
// `GroupOperations`.

pub trait PointValidation {
    type Point;

    /// Verifies if a byte representation of a curve point lies in the curve.
    fn validate_point(&self) -> bool;
}

pub trait GroupOperations {
    type Point;
    type Scalar;

    /// Adds two curve points: P_0 + P_1.
    fn add(left_point: &Self::Point, right_point: &Self::Point) -> Option<Self::Point>;

    /// Subtracts two curve points: P_0 - P_1.
    ///
    /// NOTE: Altneratively, one can consider replacing this with a `negate` function that maps a
    /// curve point P -> -P. Then subtraction can be computed by combining `negate` and `add`
    /// syscalls. However, `subtract` is a much more widely used function than `negate`.
    fn subtract(left_point: &Self::Point, right_point: &Self::Point) -> Option<Self::Point>;

    /// Multiplies a scalar S with a curve point P: S*P
    fn multiply(scalar: &Self::Scalar, point: &Self::Point) -> Option<Self::Point>;
}

pub trait MultiScalarMultiplication {
    type Scalar;
    type Point;

    /// Given a vector of scalsrs S_1, ..., S_N, and curve points P_1, ..., P_N, computes the
    /// "inner product": S_1*P_1 + ... + S_N*P_N.
    ///
    /// NOTE: This operation can be represented by combining `add` and `multiply` functions in
    /// `GroupOperations`, but computing it in a single batch is significantly cheaper. Given how
    /// commonly used the multiscalar multiplication (MSM) is, it seems to make sense to have a
    /// designated trait for MSM support.
    ///
    /// NOTE: The inputs to the function is a non-fixed size vector and hence, there are some
    /// complications in computing the cost for the syscall. The computational costs should only
    /// depend on the length of the vectors (and the curve), so it would be ideal to support
    /// variable length inputs and compute the syscall cost as is done in eip-197:
    /// <https://github.com/ethereum/EIPs/blob/master/EIPS/eip-197.md#gas-costs>. If not, then we can
    /// consider bounding the length of the input and assigning worst-case cost.
    fn multiscalar_multiply(
        scalars: &[Self::Scalar],
        points: &[Self::Point],
    ) -> Option<Self::Point>;
}

pub trait Pairing {
    type G1Point;
    type G2Point;
    type GTPoint;

    /// Applies the bilinear pairing operation to two curve points P1, P2 -> e(P1, P2). This trait
    /// is only relevant for "pairing-friendly" curves such as BN254 and BLS12-381.
    fn pairing_map(
        left_point: &Self::G1Point,
        right_point: &Self::G2Point,
    ) -> Option<Self::GTPoint>;
}

pub const CURVE25519_EDWARDS: u64 = 0;
pub const CURVE25519_RISTRETTO: u64 = 1;

pub const ADD: u64 = 0;
pub const SUB: u64 = 1;
pub const MUL: u64 = 2;
