pub trait PointValidation {
    type Point;

    fn validate_point(&self) -> bool;
}

pub trait GroupOperations {
    type Point;
    type Scalar;

    fn add(left_point: &Self::Point, right_point: &Self::Point) -> Option<Self::Point>;
    fn subtract(left_point: &Self::Point, right_point: &Self::Point) -> Option<Self::Point>;
    fn multiply(scalar: &Self::Scalar, point: &Self::Point) -> Option<Self::Point>;
}

pub trait MultiScalarMultiplication {
    type Scalar;
    type Point;

    fn multiscalar_multiply(
        scalars: Vec<&Self::Scalar>,
        points: Vec<&Self::Point>,
    ) -> Option<Self::Point>;
}

pub trait Pairing {
    type G1Point;
    type G2Point;
    type GTPoint;

    fn pairing_map(
        left_point: &Self::G1Point,
        right_point: &Self::G2Point,
    ) -> Option<Self::GTPoint>;
}

pub const CURVE25519_EDWARDS: u64 = 0;
pub const CURVE25519_RISTRETTO: u64 = 1;
// pub const BN254_G1: u64 = 2;
// pub const BN254_G2: u64 = 3;
// pub const BN254_GT: u64 = 4;

pub const ADD: u64 = 0;
pub const SUB: u64 = 1;
pub const MUL: u64 = 2;

extern "C" {
    pub fn sol_validate_point(
        curve_id: u64,
        point: *const u8,
        result: *mut u8,
    ) -> u64;

    pub fn sol_curve_op(
        curve_id: u64,
        op_id: u64,
        point: *const u8,
        result: *mut u8,
    ) -> u64;

    pub fn sol_pairing_map(
        curve_id: u64,
        point: *const u8,
        result: *mut u8,
    ) -> u64;
}

